//! Ahead-of-time build: bake a Perl script into a copy of the running `stryke` binary as
//! a compressed trailer, producing a self-contained executable.
//!
//! Layout (little-endian, appended to the end of a copy of the `stryke` binary):
//!
//! ```text
//!   [elf/mach-o bytes of stryke ...]   (unchanged, still runs as `stryke`)
//!   [zstd-compressed payload ...]
//!   [u64 compressed_len]
//!   [u64 uncompressed_len]
//!   [u32 version]
//!   [u32 reserved (0)]
//!   [8 bytes magic  b"STRYKEAOT"]
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
//! segments, so appending data leaves the original `stryke` fully runnable. On macOS the
//! resulting binary is unsigned — users distributing signed builds must re-`codesign`.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// 8-byte trailer magic (`b"STRK_AOT"`).
pub const AOT_MAGIC: &[u8; 8] = b"STRK_AOT";
/// Trailer format version 1: single script.
pub const AOT_VERSION_V1: u32 = 1;
/// Trailer format version 2: project bundle with multiple files.
pub const AOT_VERSION_V2: u32 = 2;
/// Fixed trailer length in bytes: `8 (cl) + 8 (ul) + 4 (ver) + 4 (rsv) + 8 (magic)`.
pub const TRAILER_LEN: u64 = 32;

#[derive(Debug, Clone)]
pub struct EmbeddedScript {
    /// `__FILE__` / error-reporting name (e.g. `hello.pl`).
    pub name: String,
    /// UTF-8 Perl source.
    pub source: String,
}

/// A bundled project: main entry point + library files.
#[derive(Debug, Clone)]
pub struct EmbeddedBundle {
    /// Entry point script name (e.g. `main.stk`).
    pub entry: String,
    /// All files: path -> source (includes entry + lib files).
    pub files: HashMap<String, String>,
}

/// Serialize `(name, source)` into the v1 pre-compression payload format.
fn encode_payload_v1(name: &str, source: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + name.len() + source.len());
    let name_len = u32::try_from(name.len()).expect("script name length fits in u32");
    out.extend_from_slice(&name_len.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(source.as_bytes());
    out
}

/// Serialize a project bundle into the v2 pre-compression payload format.
fn encode_payload_v2(entry: &str, files: &HashMap<String, String>) -> Vec<u8> {
    let mut out = Vec::new();
    let file_count = u32::try_from(files.len()).expect("file count fits in u32");
    out.extend_from_slice(&file_count.to_le_bytes());
    let entry_len = u32::try_from(entry.len()).expect("entry name length fits in u32");
    out.extend_from_slice(&entry_len.to_le_bytes());
    out.extend_from_slice(entry.as_bytes());
    for (path, source) in files {
        let path_len = u32::try_from(path.len()).expect("path length fits in u32");
        out.extend_from_slice(&path_len.to_le_bytes());
        out.extend_from_slice(path.as_bytes());
        let source_len = u32::try_from(source.len()).expect("source length fits in u32");
        out.extend_from_slice(&source_len.to_le_bytes());
        out.extend_from_slice(source.as_bytes());
    }
    out
}

/// Inverse of [`encode_payload_v1`].
fn decode_payload_v1(bytes: &[u8]) -> Option<EmbeddedScript> {
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

/// Inverse of [`encode_payload_v2`].
fn decode_payload_v2(bytes: &[u8]) -> Option<EmbeddedBundle> {
    let mut pos = 0usize;
    if bytes.len() < 8 {
        return None;
    }
    let file_count = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
    pos += 4;
    let entry_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
    pos += 4;
    if pos + entry_len > bytes.len() {
        return None;
    }
    let entry = std::str::from_utf8(&bytes[pos..pos + entry_len])
        .ok()?
        .to_string();
    pos += entry_len;
    let mut files = HashMap::with_capacity(file_count);
    for _ in 0..file_count {
        if pos + 4 > bytes.len() {
            return None;
        }
        let path_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
        pos += 4;
        if pos + path_len > bytes.len() {
            return None;
        }
        let path = std::str::from_utf8(&bytes[pos..pos + path_len])
            .ok()?
            .to_string();
        pos += path_len;
        if pos + 4 > bytes.len() {
            return None;
        }
        let source_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
        pos += 4;
        if pos + source_len > bytes.len() {
            return None;
        }
        let source = std::str::from_utf8(&bytes[pos..pos + source_len])
            .ok()?
            .to_string();
        pos += source_len;
        files.insert(path, source);
    }
    Some(EmbeddedBundle { entry, files })
}

/// Build a 32-byte trailer referring to `compressed_len` / `uncompressed_len`.
fn build_trailer(compressed_len: u64, uncompressed_len: u64, version: u32) -> [u8; 32] {
    let mut trailer = [0u8; 32];
    trailer[0..8].copy_from_slice(&compressed_len.to_le_bytes());
    trailer[8..16].copy_from_slice(&uncompressed_len.to_le_bytes());
    trailer[16..20].copy_from_slice(&version.to_le_bytes());
    // 20..24 reserved (zeros).
    trailer[24..32].copy_from_slice(AOT_MAGIC);
    trailer
}

/// Append a compressed v1 script payload to an existing file.
pub fn append_embedded_script(out_path: &Path, name: &str, source: &str) -> io::Result<()> {
    let payload = encode_payload_v1(name, source);
    let compressed = zstd::stream::encode_all(&payload[..], 3)?;
    let mut f = OpenOptions::new().append(true).open(out_path)?;
    f.write_all(&compressed)?;
    let trailer = build_trailer(
        compressed.len() as u64,
        payload.len() as u64,
        AOT_VERSION_V1,
    );
    f.write_all(&trailer)?;
    f.sync_all()?;
    Ok(())
}

/// Append a compressed v2 bundle payload to an existing file.
pub fn append_embedded_bundle(
    out_path: &Path,
    entry: &str,
    files: &HashMap<String, String>,
) -> io::Result<()> {
    let payload = encode_payload_v2(entry, files);
    let compressed = zstd::stream::encode_all(&payload[..], 3)?;
    let mut f = OpenOptions::new().append(true).open(out_path)?;
    f.write_all(&compressed)?;
    let trailer = build_trailer(
        compressed.len() as u64,
        payload.len() as u64,
        AOT_VERSION_V2,
    );
    f.write_all(&trailer)?;
    f.sync_all()?;
    Ok(())
}

/// Result of loading an embedded payload — either a single script (v1) or a bundle (v2).
#[derive(Debug, Clone)]
pub enum EmbeddedPayload {
    Script(EmbeddedScript),
    Bundle(EmbeddedBundle),
}

/// Fast probe: read the last 32 bytes of `exe` and return the embedded payload if present.
/// Supports both v1 (single script) and v2 (project bundle) formats.
pub fn try_load_embedded(exe: &Path) -> Option<EmbeddedPayload> {
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
    match version {
        AOT_VERSION_V1 => decode_payload_v1(&payload).map(EmbeddedPayload::Script),
        AOT_VERSION_V2 => decode_payload_v2(&payload).map(EmbeddedPayload::Bundle),
        _ => None,
    }
}

/// Legacy: load v1 single script only (for backward compat).
pub fn try_load_embedded_script(exe: &Path) -> Option<EmbeddedScript> {
    match try_load_embedded(exe)? {
        EmbeddedPayload::Script(s) => Some(s),
        EmbeddedPayload::Bundle(b) => {
            let source = b.files.get(&b.entry)?.clone();
            Some(EmbeddedScript {
                name: b.entry,
                source,
            })
        }
    }
}

/// `stryke build SCRIPT -o OUT`:
/// 1. Read and parse-validate SCRIPT (surfacing syntax errors at build time, not at user run time).
/// 2. Copy the currently-running `stryke` binary to OUT.
/// 3. Append a compressed-source trailer.
/// 4. `chmod +x` the result on unix.
///
/// Errors are returned as human-readable strings; the caller prints and sets an exit code.
pub fn build(script_path: &Path, out_path: &Path) -> Result<PathBuf, String> {
    let source = fs::read_to_string(script_path)
        .map_err(|e| format!("stryke build: cannot read {}: {}", script_path.display(), e))?;
    let script_name = script_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("script.pl")
        .to_string();

    crate::parse_with_file(&source, &script_name).map_err(|e| format!("{}", e))?;

    let exe = std::env::current_exe()
        .map_err(|e| format!("stryke build: locating current executable: {}", e))?;

    copy_exe_without_trailer(&exe, out_path).map_err(|e| {
        format!(
            "stryke build: copy {} -> {}: {}",
            exe.display(),
            out_path.display(),
            e
        )
    })?;

    append_embedded_script(out_path, &script_name, &source)
        .map_err(|e| format!("stryke build: write trailer: {}", e))?;

    set_executable(out_path);
    Ok(out_path.to_path_buf())
}

/// Collect all `.stk` and `.pl` files from a directory, excluding `t/` (tests).
fn collect_project_files(project_dir: &Path) -> io::Result<HashMap<String, String>> {
    let mut files = HashMap::new();
    fn visit(dir: &Path, base: &Path, files: &mut HashMap<String, String>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap_or(&path);
            let rel_str = rel.to_string_lossy();
            if rel_str.starts_with("t/") || rel_str.starts_with("t\\") || rel_str == "t" {
                continue;
            }
            if path.is_dir() {
                visit(&path, base, files)?;
            } else if let Some(ext) = path.extension() {
                if ext == "stk" || ext == "pl" {
                    let source = fs::read_to_string(&path)?;
                    files.insert(rel.to_string_lossy().replace('\\', "/"), source);
                }
            }
        }
        Ok(())
    }
    visit(project_dir, project_dir, &mut files)?;
    Ok(files)
}

/// `stryke build --project DIR -o OUT`:
/// Bundle main.stk + lib/*.stk (excluding t/) into a single executable.
pub fn build_project(project_dir: &Path, out_path: &Path) -> Result<PathBuf, String> {
    let entry_path = project_dir.join("main.stk");
    if !entry_path.exists() {
        return Err(format!(
            "stryke build: project directory {} has no main.stk",
            project_dir.display()
        ));
    }

    let files = collect_project_files(project_dir)
        .map_err(|e| format!("stryke build: scanning project: {}", e))?;

    eprintln!(
        "stryke build: bundling {} files from {}",
        files.len(),
        project_dir.display()
    );
    for path in files.keys() {
        eprintln!("  {}", path);
    }

    for (path, source) in &files {
        crate::parse_with_file(source, path).map_err(|e| format!("{}", e))?;
    }

    let exe = std::env::current_exe()
        .map_err(|e| format!("stryke build: locating current executable: {}", e))?;

    copy_exe_without_trailer(&exe, out_path).map_err(|e| {
        format!(
            "stryke build: copy {} -> {}: {}",
            exe.display(),
            out_path.display(),
            e
        )
    })?;

    append_embedded_bundle(out_path, "main.stk", &files)
        .map_err(|e| format!("stryke build: write trailer: {}", e))?;

    set_executable(out_path);
    Ok(out_path.to_path_buf())
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut p = meta.permissions();
        p.set_mode(p.mode() | 0o111);
        let _ = fs::set_permissions(path, p);
    }
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}

/// Copy `src` to `dst`, skipping any existing AOT trailer on `src`. Prevents nested builds
/// from stacking trailers: `stryke build a.pl -o a && stryke --exe a build b.pl -o b` would otherwise
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
            "stryke-aot-test-{}-{}-{}",
            std::process::id(),
            tag,
            rand::random::<u32>()
        ))
    }

    #[test]
    fn payload_roundtrips_name_and_source() {
        let payload = encode_payload_v1("hello.pl", "print \"hi\\n\";\n");
        let decoded = decode_payload_v1(&payload).expect("decode");
        assert_eq!(decoded.name, "hello.pl");
        assert_eq!(decoded.source, "print \"hi\\n\";\n");
    }

    #[test]
    fn append_and_load_trailer_roundtrips_on_plain_file() {
        let path = tmp_path("roundtrip");
        // Pretend this is a `stryke` binary: write a non-empty prefix so trailer math is exercised.
        fs::write(
            &path,
            b"not really an ELF, but good enough for trailer tests",
        )
        .unwrap();
        append_embedded_script(&path, "script.pl", "my $x = 1 + 2;").unwrap();
        let loaded = try_load_embedded(&path).expect("load");
        match loaded {
            EmbeddedPayload::Script(s) => {
                assert_eq!(s.name, "script.pl");
                assert_eq!(s.source, "my $x = 1 + 2;");
            }
            EmbeddedPayload::Bundle(_) => panic!("expected Script, got Bundle"),
        }
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
        fs::write(&src, b"pretend stryke binary bytes").unwrap();
        // Layer 1: embed script_a.
        fs::copy(&src, &mid).unwrap();
        append_embedded_script(&mid, "a.pl", "p 1;").unwrap();
        // Layer 2: strip + embed script_b — should yield only script_b.
        copy_exe_without_trailer(&mid, &dst).unwrap();
        append_embedded_script(&dst, "b.pl", "p 2;").unwrap();
        let loaded = try_load_embedded(&dst).expect("load layer 2");
        match loaded {
            EmbeddedPayload::Script(s) => {
                assert_eq!(s.name, "b.pl");
                assert_eq!(s.source, "p 2;");
            }
            EmbeddedPayload::Bundle(_) => panic!("expected Script, got Bundle"),
        }
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
