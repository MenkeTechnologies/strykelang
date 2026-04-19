//! Ahead-of-time build: bake a Perl script into a copy of the running `fo` binary as
//! a compressed trailer, producing a self-contained executable.
//!
//! Layout (little-endian, appended to the end of a copy of the `fo` binary):
//!
//! ```text
//!   [elf/mach-o bytes of fo ...]   (unchanged, still runs as `fo`)
//!   [zstd-compressed payload ...]
//!   [u64 compressed_len]
//!   [u64 uncompressed_len]
//!   [u32 version]
//!   [u32 reserved (0)]
//!   [8 bytes magic  b"FORGEAOT"]
//! ```
//!
//! Payload (before zstd compression):
//!
//! ```text
//!   [u32 script_name_len]
//!   [script_name utf8]
//!   [source bytes utf8]
//! ```
//!
//! Why source, not bytecode? [`crate::bytecode::Chunk`] holds `Arc<HeapObject>` runtime
//! values (regex objects, strings, closures, …) that are not serde-ready. Re-parsing a
//! typical script adds ~1-2 ms to startup which is negligible for a deployment binary.
//! The trailer format is versioned so a future pre-compiled-bytecode payload can live
//! alongside v1 without breaking already-shipped binaries.
//!
//! ELF (Linux) and Mach-O (macOS) loaders ignore bytes past the program-header-listed
//! segments, so appending data leaves the original `fo` fully runnable. On macOS the
//! resulting binary is unsigned — users distributing signed builds must re-`codesign`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// 8-byte trailer magic (`b"FORGEAOT"`).
pub const AOT_MAGIC: &[u8; 8] = b"FORGEAOT";
/// Trailer format version. Bump when the layout changes in a backward-incompatible way.
pub const AOT_VERSION: u32 = 1;
/// Fixed trailer length in bytes: `8 (cl) + 8 (ul) + 4 (ver) + 4 (rsv) + 8 (magic)`.
pub const TRAILER_LEN: u64 = 32;

#[derive(Debug, Clone)]
pub struct EmbeddedScript {
    /// `__FILE__` / error-reporting name (e.g. `hello.pl`).
    pub name: String,
    /// UTF-8 Perl source.
    pub source: String,
}

/// Serialize `(name, source)` into the pre-compression payload format.
fn encode_payload(name: &str, source: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + name.len() + source.len());
    let name_len = u32::try_from(name.len()).expect("script name length fits in u32");
    out.extend_from_slice(&name_len.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(source.as_bytes());
    out
}

/// Inverse of [`encode_payload`].
fn decode_payload(bytes: &[u8]) -> Option<EmbeddedScript> {
    if bytes.len() < 4 {
        return None;
    }
    let name_len = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
    if 4 + name_len > bytes.len() {
        return None;
    }
    let name = std::str::from_utf8(&bytes[4..4 + name_len])
        .ok()?
        .to_string();
    let source = std::str::from_utf8(&bytes[4 + name_len..])
        .ok()?
        .to_string();
    Some(EmbeddedScript { name, source })
}

/// Build a 32-byte trailer referring to `compressed_len` / `uncompressed_len`.
fn build_trailer(compressed_len: u64, uncompressed_len: u64) -> [u8; 32] {
    let mut trailer = [0u8; 32];
    trailer[0..8].copy_from_slice(&compressed_len.to_le_bytes());
    trailer[8..16].copy_from_slice(&uncompressed_len.to_le_bytes());
    trailer[16..20].copy_from_slice(&AOT_VERSION.to_le_bytes());
    // 20..24 reserved (zeros).
    trailer[24..32].copy_from_slice(AOT_MAGIC);
    trailer
}

/// Append a compressed script payload to an existing file. The file must already be a copy
/// of the `fo` binary; this function only touches the tail.
pub fn append_embedded_script(out_path: &Path, name: &str, source: &str) -> io::Result<()> {
    let payload = encode_payload(name, source);
    let compressed = zstd::stream::encode_all(&payload[..], 3)?;
    let mut f = OpenOptions::new().append(true).open(out_path)?;
    f.write_all(&compressed)?;
    let trailer = build_trailer(compressed.len() as u64, payload.len() as u64);
    f.write_all(&trailer)?;
    f.sync_all()?;
    Ok(())
}

/// Fast probe: read the last 32 bytes of `exe` and return the embedded script if one is present.
/// Never panics; returns `None` for any malformed, missing, or version-mismatched trailer.
///
/// Cost: one file open + one 32-byte read (`~50 μs` on SSD) when there is no trailer, plus
/// one seek+read+zstd-decode when there is. Safe to call on every `fo` startup.
pub fn try_load_embedded(exe: &Path) -> Option<EmbeddedScript> {
    let mut f = File::open(exe).ok()?;
    let size = f.metadata().ok()?.len();
    if size < TRAILER_LEN {
        return None;
    }
    f.seek(SeekFrom::End(-(TRAILER_LEN as i64))).ok()?;
    let mut trailer = [0u8; TRAILER_LEN as usize];
    f.read_exact(&mut trailer).ok()?;
    if &trailer[24..32] != AOT_MAGIC {
        return None;
    }
    let compressed_len = u64::from_le_bytes(trailer[0..8].try_into().ok()?);
    let uncompressed_len = u64::from_le_bytes(trailer[8..16].try_into().ok()?);
    let version = u32::from_le_bytes(trailer[16..20].try_into().ok()?);
    if version != AOT_VERSION {
        return None;
    }
    if compressed_len == 0 || compressed_len > size - TRAILER_LEN {
        return None;
    }
    let payload_start = size - TRAILER_LEN - compressed_len;
    f.seek(SeekFrom::Start(payload_start)).ok()?;
    let mut compressed = vec![0u8; compressed_len as usize];
    f.read_exact(&mut compressed).ok()?;
    let payload = zstd::stream::decode_all(&compressed[..]).ok()?;
    if payload.len() != uncompressed_len as usize {
        return None;
    }
    decode_payload(&payload)
}

/// `fo build SCRIPT -o OUT`:
/// 1. Read and parse-validate SCRIPT (surfacing syntax errors at build time, not at user run time).
/// 2. Copy the currently-running `fo` binary to OUT.
/// 3. Append a compressed-source trailer.
/// 4. `chmod +x` the result on unix.
///
/// Errors are returned as human-readable strings; the caller prints and sets an exit code.
pub fn build(script_path: &Path, out_path: &Path) -> Result<PathBuf, String> {
    let source = fs::read_to_string(script_path)
        .map_err(|e| format!("fo build: cannot read {}: {}", script_path.display(), e))?;
    let script_name = script_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("script.pl")
        .to_string();

    // Parse-validate up front so build-time errors are reported here, not at user run time.
    crate::parse_with_file(&source, &script_name).map_err(|e| format!("{}", e))?;

    let exe = std::env::current_exe()
        .map_err(|e| format!("fo build: locating current executable: {}", e))?;

    // If the running `fo` itself already has an embedded trailer (e.g. nested build), strip
    // it first so the output binary does not end up with two trailers stacked. The strip is
    // done implicitly: `copy_exe_without_trailer` writes only the untrimmed prefix.
    copy_exe_without_trailer(&exe, out_path).map_err(|e| {
        format!(
            "fo build: copy {} -> {}: {}",
            exe.display(),
            out_path.display(),
            e
        )
    })?;

    append_embedded_script(out_path, &script_name, &source)
        .map_err(|e| format!("fo build: write trailer: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(out_path) {
            let mut p = meta.permissions();
            // Preserve existing bits, add execute for user/group/other.
            p.set_mode(p.mode() | 0o111);
            let _ = fs::set_permissions(out_path, p);
        }
    }

    Ok(out_path.to_path_buf())
}

/// Copy `src` to `dst`, skipping any existing AOT trailer on `src`. Prevents nested builds
/// from stacking trailers: `fo build a.pl -o a && fo --exe a build b.pl -o b` would otherwise
/// embed both scripts, one on top of the other.
fn copy_exe_without_trailer(src: &Path, dst: &Path) -> io::Result<()> {
    let mut sf = File::open(src)?;
    let size = sf.metadata()?.len();
    let keep = if size >= TRAILER_LEN {
        sf.seek(SeekFrom::End(-(TRAILER_LEN as i64)))?;
        let mut trailer = [0u8; TRAILER_LEN as usize];
        if sf.read_exact(&mut trailer).is_ok() && &trailer[24..32] == AOT_MAGIC {
            let compressed_len = u64::from_le_bytes(trailer[0..8].try_into().unwrap());
            if compressed_len > 0 && compressed_len <= size - TRAILER_LEN {
                size - TRAILER_LEN - compressed_len
            } else {
                size
            }
        } else {
            size
        }
    } else {
        size
    };
    sf.seek(SeekFrom::Start(0))?;
    // Remove any existing destination first so `fs::copy`-like behaviour is atomic from the
    // caller's point of view and we never open the running destination for truncation.
    let _ = fs::remove_file(dst);
    let mut df = File::create(dst)?;
    let mut remaining = keep;
    let mut buf = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let n = std::cmp::min(remaining as usize, buf.len());
        sf.read_exact(&mut buf[..n])?;
        df.write_all(&buf[..n])?;
        remaining -= n as u64;
    }
    df.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir();
        dir.join(format!(
            "forge-aot-test-{}-{}-{}",
            std::process::id(),
            tag,
            rand::random::<u32>()
        ))
    }

    #[test]
    fn payload_roundtrips_name_and_source() {
        let payload = encode_payload("hello.pl", "print \"hi\\n\";\n");
        let decoded = decode_payload(&payload).expect("decode");
        assert_eq!(decoded.name, "hello.pl");
        assert_eq!(decoded.source, "print \"hi\\n\";\n");
    }

    #[test]
    fn append_and_load_trailer_roundtrips_on_plain_file() {
        let path = tmp_path("roundtrip");
        // Pretend this is a `fo` binary: write a non-empty prefix so trailer math is exercised.
        fs::write(
            &path,
            b"not really an ELF, but good enough for trailer tests",
        )
        .unwrap();
        append_embedded_script(&path, "script.pl", "my $x = 1 + 2;").unwrap();
        let loaded = try_load_embedded(&path).expect("load");
        assert_eq!(loaded.name, "script.pl");
        assert_eq!(loaded.source, "my $x = 1 + 2;");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn load_returns_none_for_file_without_trailer() {
        let path = tmp_path("no-trailer");
        fs::write(&path, b"plain binary, no magic").unwrap();
        assert!(try_load_embedded(&path).is_none());
        fs::remove_file(&path).ok();
    }

    #[test]
    fn load_returns_none_for_short_file() {
        let path = tmp_path("short");
        fs::write(&path, b"abc").unwrap();
        assert!(try_load_embedded(&path).is_none());
        fs::remove_file(&path).ok();
    }

    #[test]
    fn copy_without_trailer_strips_embedded_script() {
        let src = tmp_path("src");
        let mid = tmp_path("mid");
        let dst = tmp_path("dst");
        fs::write(&src, b"pretend fo binary bytes").unwrap();
        // Layer 1: embed script_a.
        fs::copy(&src, &mid).unwrap();
        append_embedded_script(&mid, "a.pl", "say 1;").unwrap();
        // Layer 2: strip + embed script_b — should yield only script_b.
        copy_exe_without_trailer(&mid, &dst).unwrap();
        append_embedded_script(&dst, "b.pl", "say 2;").unwrap();
        let loaded = try_load_embedded(&dst).expect("load layer 2");
        assert_eq!(loaded.name, "b.pl");
        assert_eq!(loaded.source, "say 2;");
        // Compare stripped prefix to original: they must match byte-for-byte.
        let original = fs::read(&src).unwrap();
        let mut stripped_dst = fs::read(&dst).unwrap();
        stripped_dst.truncate(original.len());
        assert_eq!(stripped_dst, original);
        fs::remove_file(&src).ok();
        fs::remove_file(&mid).ok();
        fs::remove_file(&dst).ok();
    }

    #[test]
    fn bad_magic_is_ignored() {
        let path = tmp_path("bad-magic");
        let mut bytes = vec![0u8; 200];
        // Write 32 bytes that look like a trailer but with wrong magic at the end.
        let tail = &mut bytes[200 - 32..];
        tail[0..8].copy_from_slice(&10u64.to_le_bytes()); // compressed_len claims 10
        tail[8..16].copy_from_slice(&20u64.to_le_bytes());
        tail[16..20].copy_from_slice(&1u32.to_le_bytes());
        tail[24..32].copy_from_slice(b"NOTPERLZ");
        fs::write(&path, &bytes).unwrap();
        assert!(try_load_embedded(&path).is_none());
        fs::remove_file(&path).ok();
    }
}
