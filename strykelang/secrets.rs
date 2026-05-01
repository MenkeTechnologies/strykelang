//! Encrypted secrets — AES-256-GCM authenticated encryption.
//!
//! `secrets_encrypt($plain, key => $32_byte_key)` returns a base64
//! string containing nonce(12) || ciphertext || tag(16). The nonce is
//! random per call so encrypting the same plaintext twice produces
//! different ciphertexts (standard AEAD practice).
//!
//! The 32-byte key can come from anywhere — usually `$ENV{STRYKE_SECRET_KEY}`,
//! a kdf'd password (`secrets_kdf("password", salt => "...")`), or a
//! file. `secrets_random_key()` provides a fresh 32-byte key as base64.
//!
//! Per the design rule: never auto-derive a key. The caller must
//! provide one explicitly, so there's no implicit "secrets are
//! protected" lie when no key was set.

use crate::error::PerlError;
use crate::value::PerlValue;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::Engine;

type Result<T> = std::result::Result<T, PerlError>;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

fn parse_opts(args: &[PerlValue]) -> indexmap::IndexMap<String, PerlValue> {
    let mut out = indexmap::IndexMap::new();
    let mut i = 0;
    while i + 1 < args.len() {
        out.insert(args[i].to_string(), args[i + 1].clone());
        i += 2;
    }
    out
}

/// Decode a base64-encoded key into 32 raw bytes. Returns the key
/// without copying when it's already 32 raw bytes (legacy callers).
fn decode_key(s: &str, label: &str, line: usize) -> Result<Vec<u8>> {
    if s.len() == KEY_LEN {
        return Ok(s.as_bytes().to_vec());
    }
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| {
            PerlError::runtime(
                format!("{}: key must be 32 raw bytes or base64 — {}", label, e),
                line,
            )
        })
        .and_then(|bytes| {
            if bytes.len() == KEY_LEN {
                Ok(bytes)
            } else {
                Err(PerlError::runtime(
                    format!(
                        "{}: decoded key is {} bytes, want {}",
                        label,
                        bytes.len(),
                        KEY_LEN
                    ),
                    line,
                ))
            }
        })
}

fn random_bytes(n: usize) -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut buf = vec![0u8; n];
    // Pull entropy from /dev/urandom on Unix; fall back to clock-derived seed elsewhere.
    #[cfg(unix)]
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        if f.read_exact(&mut buf).is_ok() {
            return buf;
        }
    }
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut state = seed.wrapping_add(0xDEAD_BEEF_CAFE_F00D);
    for byte in buf.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }
    buf
}

/// `secrets_encrypt($plaintext, key => $key)` → base64 string of
/// `nonce(12) || ciphertext || tag(16)`. AES-256-GCM under the hood.
pub fn secrets_encrypt(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let plain = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("secrets_encrypt: plaintext required", line))?;
    let opts = parse_opts(&args[1..]);
    let key_str = opts
        .get("key")
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("secrets_encrypt: key => $32byte_key required", line))?;
    let key_bytes = decode_key(&key_str, "secrets_encrypt", line)?;

    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| PerlError::runtime(format!("secrets_encrypt: key init: {}", e), line))?;
    let nonce_bytes = random_bytes(NONCE_LEN);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plain.as_bytes())
        .map_err(|e| PerlError::runtime(format!("secrets_encrypt: encrypt: {}", e), line))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&out),
    ))
}

/// `secrets_decrypt($b64_envelope, key => $key)` → plaintext string.
/// Returns undef on auth failure rather than throwing — secrets code
/// often wants to fall through to a "not yet provisioned" branch.
pub fn secrets_decrypt(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let envelope = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("secrets_decrypt: envelope required", line))?;
    let opts = parse_opts(&args[1..]);
    let key_str = opts
        .get("key")
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("secrets_decrypt: key => $32byte_key required", line))?;
    let key_bytes = decode_key(&key_str, "secrets_decrypt", line)?;

    let raw = match base64::engine::general_purpose::STANDARD.decode(envelope.as_bytes()) {
        Ok(r) => r,
        Err(_) => return Ok(PerlValue::UNDEF),
    };
    if raw.len() < NONCE_LEN + 16 {
        return Ok(PerlValue::UNDEF);
    }
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| PerlError::runtime(format!("secrets_decrypt: key init: {}", e), line))?;
    let nonce = Nonce::from_slice(&raw[..NONCE_LEN]);
    let pt = match cipher.decrypt(nonce, &raw[NONCE_LEN..]) {
        Ok(p) => p,
        Err(_) => return Ok(PerlValue::UNDEF),
    };
    match String::from_utf8(pt) {
        Ok(s) => Ok(PerlValue::string(s)),
        Err(_) => Ok(PerlValue::UNDEF),
    }
}

/// `secrets_random_key()` → fresh 32-byte AES-256 key as base64.
pub fn secrets_random_key(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let bytes = random_bytes(KEY_LEN);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&bytes),
    ))
}

/// `secrets_kdf($password, salt => $salt, iterations => 600_000)` →
/// base64 32-byte key derived from password via PBKDF2-HMAC-SHA256.
/// 600k iterations matches the OWASP 2024 recommendation. Use the
/// same salt every time you derive the same key (the salt is not
/// secret; persist it alongside the encrypted blob).
pub fn secrets_kdf(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    use hmac::Hmac;
    use sha2::Sha256;
    let password = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("secrets_kdf: password required", line))?;
    let opts = parse_opts(&args[1..]);
    let salt = opts
        .get("salt")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "stryke-secrets-default-salt".to_string());
    let iterations = opts
        .get("iterations")
        .map(|v| v.to_int().max(1))
        .unwrap_or(600_000) as u32;

    let mut out = [0u8; KEY_LEN];
    pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), salt.as_bytes(), iterations, &mut out)
        .map_err(|e| PerlError::runtime(format!("secrets_kdf: {}", e), line))?;
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(out),
    ))
}
