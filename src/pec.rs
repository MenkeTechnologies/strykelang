//! On-disk bytecode bundles (`.pec`): serialized [`crate::ast::Program`] + [`crate::bytecode::Chunk`]
//! for warm starts without re-parsing or re-compiling when `STRYKE_BC_CACHE=1`.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::ast::Program;
use crate::bytecode::Chunk;
use crate::error::{PerlError, PerlResult};
use crate::value::PerlValue;

/// `STRYKE_BC_CACHE=1` enables read-through `.pec` in [`cache_dir`] / `<sha256>.pec`.
pub fn cache_enabled() -> bool {
    matches!(
        std::env::var("STRYKE_BC_CACHE").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

/// `~/.cache/stryke/bc` (or `$STRYKE_BC_DIR` override).
pub fn cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("STRYKE_BC_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .unwrap_or_default();
    PathBuf::from(home).join(".cache").join("stryke").join("bc")
}

/// Fingerprint for cache key (includes crate version, strict flag, path, and source).
pub fn source_fingerprint(strict_vars: bool, source_file: &str, code: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update([0u8]);
    h.update([strict_vars as u8]);
    h.update([0u8]);
    h.update(source_file.as_bytes());
    h.update([0u8]);
    h.update(code.as_bytes());
    h.finalize().into()
}

pub fn cache_path_for_fingerprint(fp: &[u8; 32]) -> PathBuf {
    cache_dir().join(format!(
        "{:x}.pec",
        u128::from_be_bytes(fp[0..16].try_into().unwrap())
    ))
}

/// Hex form of full32-byte fingerprint (collision-safe filename).
pub fn cache_path_hex(fp: &[u8; 32]) -> PathBuf {
    cache_dir().join(format!("{}_{:x}.pec", hex::encode(fp), fp[0] as u32))
}

fn cache_path(fp: &[u8; 32]) -> PathBuf {
    cache_dir().join(format!("{}.pec", hex::encode(fp)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PecBundle {
    pub format_version: u32,
    pub pointer_width: u8,
    pub strict_vars: bool,
    pub source_fingerprint: [u8; 32],
    pub program: Program,
    pub chunk: Chunk,
}

impl PecBundle {
    /// Bumped from `1` to `2` when zstd compression was added — v1 readers will reject
    /// v2 files and vice versa, so mixed-version caches are a clean miss (re-compile)
    /// rather than a corrupt-decode error.
    pub const FORMAT_VERSION: u32 = 2;
    pub const MAGIC: [u8; 4] = *b"PEC2";
    /// zstd compression level for the embedded payload. Level 3 is the sweet spot for
    /// serialized bytecode: ~10× shrink ratio, compression ~2× faster than level 1 decode.
    const ZSTD_LEVEL: i32 = 3;

    pub fn new(strict_vars: bool, fp: [u8; 32], program: Program, chunk: Chunk) -> Self {
        Self {
            format_version: Self::FORMAT_VERSION,
            pointer_width: std::mem::size_of::<usize>() as u8,
            strict_vars,
            source_fingerprint: fp,
            program,
            chunk,
        }
    }

    pub fn encode(&self) -> PerlResult<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(&Self::MAGIC);
        let payload = bincode::serialize(self)
            .map_err(|e| PerlError::runtime(format!("pec: bincode serialize failed: {e}"), 0))?;
        let compressed = zstd::stream::encode_all(&payload[..], Self::ZSTD_LEVEL)
            .map_err(|e| PerlError::runtime(format!("pec: zstd encode failed: {e}"), 0))?;
        out.extend_from_slice(&compressed);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> PerlResult<Self> {
        if bytes.len() < 4 + 8 {
            return Err(PerlError::runtime("pec: file too small", 0));
        }
        if bytes[0..4] != Self::MAGIC {
            return Err(PerlError::runtime("pec: bad magic", 0));
        }
        let payload = zstd::stream::decode_all(&bytes[4..])
            .map_err(|e| PerlError::runtime(format!("pec: zstd decode failed: {e}"), 0))?;
        let bundle: PecBundle = bincode::deserialize(&payload)
            .map_err(|e| PerlError::runtime(format!("pec: bincode deserialize failed: {e}"), 0))?;
        if bundle.format_version != Self::FORMAT_VERSION {
            return Err(PerlError::runtime(
                format!(
                    "pec: unsupported format_version {} (expected {})",
                    bundle.format_version,
                    Self::FORMAT_VERSION
                ),
                0,
            ));
        }
        if bundle.pointer_width != std::mem::size_of::<usize>() as u8 {
            return Err(PerlError::runtime(
                format!(
                    "pec: pointer_width mismatch (file {} vs host {})",
                    bundle.pointer_width,
                    std::mem::size_of::<usize>()
                ),
                0,
            ));
        }
        Ok(bundle)
    }
}

/// Try load a bundle; `expected_fp` must match embedded fingerprint.
pub fn try_load(expected_fp: &[u8; 32], strict_vars: bool) -> PerlResult<Option<PecBundle>> {
    let path = cache_path(expected_fp);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(PerlError::runtime(
                format!("pec: read {}: {e}", path.display()),
                0,
            ))
        }
    };
    let bundle = PecBundle::decode(&bytes)?;
    if bundle.source_fingerprint != *expected_fp {
        return Ok(None);
    }
    if bundle.strict_vars != strict_vars {
        return Ok(None);
    }
    Ok(Some(bundle))
}

pub fn try_save(bundle: &PecBundle) -> PerlResult<()> {
    let dir = cache_dir();
    fs::create_dir_all(&dir).map_err(|e| {
        PerlError::runtime(format!("pec: create_dir_all {}: {e}", dir.display()), 0)
    })?;
    let path = cache_path(&bundle.source_fingerprint);
    let data = bundle.encode()?;
    let tmp = path.with_extension("pec.tmp");
    let mut f = fs::File::create(&tmp)
        .map_err(|e| PerlError::runtime(format!("pec: create {}: {e}", tmp.display()), 0))?;
    f.write_all(&data)
        .map_err(|e| PerlError::runtime(format!("pec: write {}: {e}", tmp.display()), 0))?;
    drop(f);
    fs::rename(&tmp, &path).map_err(|e| {
        PerlError::runtime(
            format!("pec: rename {} -> {}: {e}", tmp.display(), path.display()),
            0,
        )
    })?;
    Ok(())
}

// ── Constant pool (Chunk.constants): only immediate-ish literals are allowed in .pec ───────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PecConst {
    Undef,
    Int(i64),
    Float(f64),
    Str(String),
}

fn pec_const_from_perl(v: &PerlValue) -> Result<PecConst, String> {
    if v.is_undef() {
        return Ok(PecConst::Undef);
    }
    if let Some(n) = v.as_integer() {
        return Ok(PecConst::Int(n));
    }
    if let Some(f) = v.as_float() {
        return Ok(PecConst::Float(f));
    }
    if let Some(s) = v.as_str() {
        return Ok(PecConst::Str(s.to_string()));
    }
    Err(format!(
        "constant pool value cannot be stored in .pec (type {})",
        v.ref_type()
    ))
}

fn perl_from_pec_const(c: PecConst) -> PerlValue {
    match c {
        PecConst::Undef => PerlValue::UNDEF,
        PecConst::Int(n) => PerlValue::integer(n),
        PecConst::Float(f) => PerlValue::float(f),
        PecConst::Str(s) => PerlValue::string(s),
    }
}

pub mod constants_pool_codec {
    use super::*;

    pub fn serialize<S>(values: &Vec<PerlValue>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut out = Vec::with_capacity(values.len());
        for v in values {
            let c = pec_const_from_perl(v).map_err(serde::ser::Error::custom)?;
            out.push(c);
        }
        out.serialize(ser)
    }

    pub fn deserialize<'de, D>(de: D) -> Result<Vec<PerlValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v: Vec<PecConst> = Vec::deserialize(de)?;
        Ok(v.into_iter().map(perl_from_pec_const).collect())
    }
}

/// Remove mistaken duplicate helper filenames if any (no-op for normal paths).
#[allow(dead_code)]
pub fn pec_paths_legacy(_fp: &[u8; 32]) -> (PathBuf, PathBuf) {
    (cache_path_for_fingerprint(_fp), cache_path_hex(_fp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Compiler;
    use crate::interpreter::Interpreter;

    #[test]
    fn pec_round_trip_bundle_encode_decode() {
        let code = "my $x = 40 + 2; $x";
        let program = crate::parse(code).expect("parse");
        let mut interp = Interpreter::new();
        interp.prepare_program_top_level(&program).expect("prep");
        let chunk = Compiler::new()
            .with_source_file("-e".into())
            .compile_program(&program)
            .expect("compile");
        let fp = source_fingerprint(false, "-e", code);
        let bundle = PecBundle::new(false, fp, program, chunk);
        let bytes = bundle.encode().expect("encode");
        let got = PecBundle::decode(&bytes).expect("decode");
        assert_eq!(got.source_fingerprint, fp);
        assert_eq!(got.chunk.ops.len(), bundle.chunk.ops.len());
    }
}
