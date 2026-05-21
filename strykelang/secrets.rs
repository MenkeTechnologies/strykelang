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

use crate::error::StrykeError;
use crate::value::StrykeValue;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::Engine;

type Result<T> = std::result::Result<T, StrykeError>;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

fn parse_opts(args: &[StrykeValue]) -> indexmap::IndexMap<String, StrykeValue> {
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
            StrykeError::runtime(
                format!("{}: key must be 32 raw bytes or base64 — {}", label, e),
                line,
            )
        })
        .and_then(|bytes| {
            if bytes.len() == KEY_LEN {
                Ok(bytes)
            } else {
                Err(StrykeError::runtime(
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
pub fn secrets_encrypt(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let plain = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| StrykeError::runtime("secrets_encrypt: plaintext required", line))?;
    let opts = parse_opts(&args[1..]);
    let key_str = opts.get("key").map(|v| v.to_string()).ok_or_else(|| {
        StrykeError::runtime("secrets_encrypt: key => $32byte_key required", line)
    })?;
    let key_bytes = decode_key(&key_str, "secrets_encrypt", line)?;

    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| StrykeError::runtime(format!("secrets_encrypt: key init: {}", e), line))?;
    let nonce_bytes = random_bytes(NONCE_LEN);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plain.as_bytes())
        .map_err(|e| StrykeError::runtime(format!("secrets_encrypt: encrypt: {}", e), line))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(StrykeValue::string(
        base64::engine::general_purpose::STANDARD.encode(&out),
    ))
}

/// `secrets_decrypt($b64_envelope, key => $key)` → plaintext string.
/// Returns undef on auth failure rather than throwing — secrets code
/// often wants to fall through to a "not yet provisioned" branch.
pub fn secrets_decrypt(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    let envelope = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| StrykeError::runtime("secrets_decrypt: envelope required", line))?;
    let opts = parse_opts(&args[1..]);
    let key_str = opts.get("key").map(|v| v.to_string()).ok_or_else(|| {
        StrykeError::runtime("secrets_decrypt: key => $32byte_key required", line)
    })?;
    let key_bytes = decode_key(&key_str, "secrets_decrypt", line)?;

    let raw = match base64::engine::general_purpose::STANDARD.decode(envelope.as_bytes()) {
        Ok(r) => r,
        Err(_) => return Ok(StrykeValue::UNDEF),
    };
    if raw.len() < NONCE_LEN + 16 {
        return Ok(StrykeValue::UNDEF);
    }
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| StrykeError::runtime(format!("secrets_decrypt: key init: {}", e), line))?;
    let nonce = Nonce::from_slice(&raw[..NONCE_LEN]);
    let pt = match cipher.decrypt(nonce, &raw[NONCE_LEN..]) {
        Ok(p) => p,
        Err(_) => return Ok(StrykeValue::UNDEF),
    };
    match String::from_utf8(pt) {
        Ok(s) => Ok(StrykeValue::string(s)),
        Err(_) => Ok(StrykeValue::UNDEF),
    }
}

/// `secrets_random_key()` → fresh 32-byte AES-256 key as base64.
pub fn secrets_random_key(_args: &[StrykeValue], _line: usize) -> Result<StrykeValue> {
    let bytes = random_bytes(KEY_LEN);
    Ok(StrykeValue::string(
        base64::engine::general_purpose::STANDARD.encode(&bytes),
    ))
}

/// `secrets_kdf($password, salt => $salt, iterations => 600_000)` →
/// base64 32-byte key derived from password via PBKDF2-HMAC-SHA256.
/// 600k iterations matches the OWASP 2024 recommendation. Use the
/// same salt every time you derive the same key (the salt is not
/// secret; persist it alongside the encrypted blob).
pub fn secrets_kdf(args: &[StrykeValue], line: usize) -> Result<StrykeValue> {
    use hmac::Hmac;
    use sha2::Sha256;
    let password = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| StrykeError::runtime("secrets_kdf: password required", line))?;
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
        .map_err(|e| StrykeError::runtime(format!("secrets_kdf: {}", e), line))?;
    Ok(StrykeValue::string(
        base64::engine::general_purpose::STANDARD.encode(out),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> StrykeValue {
        StrykeValue::string(x.to_string())
    }

    fn args_with_key(plain: &str, key: &StrykeValue) -> Vec<StrykeValue> {
        vec![s(plain), s("key"), key.clone()]
    }

    #[test]
    fn random_key_is_44_char_base64_for_32_raw_bytes() {
        let k = secrets_random_key(&[], 0).expect("random key");
        let kstr = k.to_string();
        // 32 bytes base64 = 44 chars including '=' padding.
        assert_eq!(kstr.len(), 44, "got {:?}", kstr);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&kstr)
            .expect("valid base64");
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn random_key_is_random() {
        let a = secrets_random_key(&[], 0).unwrap().to_string();
        let b = secrets_random_key(&[], 0).unwrap().to_string();
        assert_ne!(a, b, "two random keys should differ");
    }

    #[test]
    fn encrypt_then_decrypt_roundtrip() {
        let key = secrets_random_key(&[], 0).unwrap();
        let env = secrets_encrypt(&args_with_key("hello world", &key), 0).expect("enc");
        let pt = secrets_decrypt(&args_with_key(&env.to_string(), &key), 0).expect("dec");
        assert_eq!(pt.to_string(), "hello world");
    }

    #[test]
    fn encrypt_same_plaintext_twice_yields_different_envelopes() {
        let key = secrets_random_key(&[], 0).unwrap();
        let a = secrets_encrypt(&args_with_key("same", &key), 0)
            .unwrap()
            .to_string();
        let b = secrets_encrypt(&args_with_key("same", &key), 0)
            .unwrap()
            .to_string();
        assert_ne!(a, b, "AEAD nonce must randomize each call");
    }

    #[test]
    fn decrypt_with_wrong_key_returns_undef_not_error() {
        let k1 = secrets_random_key(&[], 0).unwrap();
        let k2 = secrets_random_key(&[], 0).unwrap();
        let env = secrets_encrypt(&args_with_key("topsecret", &k1), 0).unwrap();
        let pt = secrets_decrypt(&args_with_key(&env.to_string(), &k2), 0).expect("no error");
        assert!(pt.is_undef(), "wrong key must yield undef");
    }

    #[test]
    fn decrypt_garbage_envelope_returns_undef() {
        let key = secrets_random_key(&[], 0).unwrap();
        let pt = secrets_decrypt(&args_with_key("not-base64-$$$", &key), 0).expect("no error");
        assert!(pt.is_undef());
        let pt = secrets_decrypt(&args_with_key("aGk=", &key), 0).expect("no error");
        assert!(pt.is_undef(), "truncated envelope must be undef");
    }

    #[test]
    fn encrypt_requires_key() {
        let err = secrets_encrypt(&[s("plain")], 7).unwrap_err();
        assert!(err.to_string().contains("key"));
    }

    #[test]
    fn encrypt_rejects_bad_key_length() {
        let bad_key = s("too-short");
        let err = secrets_encrypt(&args_with_key("x", &bad_key), 0).unwrap_err();
        assert!(err.to_string().contains("key"));
    }

    #[test]
    fn kdf_is_deterministic_for_same_password_and_salt() {
        let pw = s("hunter2");
        let salt = s("salt");
        let opts = [
            s("salt"),
            salt.clone(),
            s("iterations"),
            StrykeValue::integer(1000),
        ];
        let mut a_args = vec![pw.clone()];
        a_args.extend(opts.iter().cloned());
        let a = secrets_kdf(&a_args, 0).unwrap().to_string();
        let b = secrets_kdf(&a_args, 0).unwrap().to_string();
        assert_eq!(a, b, "PBKDF2 must be deterministic");
    }

    #[test]
    fn kdf_differs_when_salt_differs() {
        let pw = s("hunter2");
        let mk = |salt: &str| {
            let args = vec![
                pw.clone(),
                s("salt"),
                s(salt),
                s("iterations"),
                StrykeValue::integer(1000),
            ];
            secrets_kdf(&args, 0).unwrap().to_string()
        };
        assert_ne!(mk("a"), mk("b"));
    }

    #[test]
    fn kdf_output_is_44_char_base64() {
        let args = vec![s("pw"), s("iterations"), StrykeValue::integer(1000)];
        let k = secrets_kdf(&args, 0).unwrap().to_string();
        assert_eq!(k.len(), 44);
        let key = secrets_random_key(&[], 0).unwrap();
        // Sanity: KDF output should be a usable AES key.
        let env =
            secrets_encrypt(&args_with_key("ok", &StrykeValue::string(k.clone())), 0).unwrap();
        let _ = key; // touch
        let pt =
            secrets_decrypt(&args_with_key(&env.to_string(), &StrykeValue::string(k)), 0).unwrap();
        assert_eq!(pt.to_string(), "ok");
    }

    #[test]
    fn encrypt_accepts_legacy_32_raw_byte_key() {
        // decode_key short-circuits when input is exactly 32 bytes.
        let raw = "0123456789abcdef0123456789abcdef";
        assert_eq!(raw.len(), 32);
        let env = secrets_encrypt(&args_with_key("payload", &s(raw)), 0).unwrap();
        let pt = secrets_decrypt(&args_with_key(&env.to_string(), &s(raw)), 0).unwrap();
        assert_eq!(pt.to_string(), "payload");
    }
}
