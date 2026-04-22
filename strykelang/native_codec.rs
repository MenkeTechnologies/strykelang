//! Cryptographic digests, compression, config decoders, and datetime helpers (UTC epoch + IANA zones via `chrono-tz`).

use std::io::{Read, Write};

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit as AesKeyInit, Nonce as AesNonce};
use base64::Engine;
use blake2::{Blake2b512, Blake2s256, Digest as Blake2Digest};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit as ChachaKeyInit, Nonce as ChachaNonce};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use ecdsa::signature::{Signer as EcdsaSigner, Verifier as EcdsaVerifier};
use ed25519_dalek::{SigningKey, VerifyingKey};
use elliptic_curve::sec1::FromEncodedPoint;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use indexmap::IndexMap;
use md5::{Digest as Md5Digest, Md5};
use parking_lot::RwLock;
use pbkdf2::pbkdf2_hmac_array;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use rand::RngCore;
use ripemd::{Digest as RipemdDigest, Ripemd160};
use rsa::pkcs1v15::{SigningKey as RsaSigningKey, VerifyingKey as RsaVerifyingKey};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use rsa::signature::SignatureEncoding;
use rsa::{Oaep, Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Sha224, Sha256, Sha384, Sha512};
use sha3::{Digest as Sha3Digest, Sha3_256, Sha3_512, Shake128, Shake256};
use siphasher::sip::SipHasher24;
use std::hash::Hasher;
use std::sync::Arc;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

use crate::error::{PerlError, PerlResult};
use crate::value::PerlValue;

type HmacSha256 = Hmac<Sha256>;
type HmacSha1 = Hmac<Sha1>;
type HmacSha384 = Hmac<Sha384>;
type HmacSha512 = Hmac<Sha512>;
type HmacMd5 = Hmac<Md5>;

fn bytes_from_value(v: &PerlValue) -> Vec<u8> {
    if let Some(b) = v.as_bytes_arc() {
        return b.as_ref().clone();
    }
    let s = v.to_string();
    // File-aware: if the string is a path to an existing file, hash its contents
    if !s.is_empty() && !s.contains('\n') {
        let p = std::path::Path::new(&s);
        if p.is_file() {
            if let Ok(data) = std::fs::read(p) {
                return data;
            }
        }
    }
    s.into_bytes()
}

/// SHA-256 digest of the argument as UTF-8 bytes; returns lowercase hex (64 chars).
pub(crate) fn sha256(v: &PerlValue) -> PerlResult<PerlValue> {
    let d = Sha256::digest(bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(d)))
}

/// SHA-224 digest; lowercase hex (56 chars).
pub(crate) fn sha224(v: &PerlValue) -> PerlResult<PerlValue> {
    let d = Sha224::digest(bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(d)))
}

/// SHA-384 digest; lowercase hex (96 chars).
pub(crate) fn sha384(v: &PerlValue) -> PerlResult<PerlValue> {
    let d = Sha384::digest(bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(d)))
}

/// SHA-512 digest; lowercase hex (128 chars).
pub(crate) fn sha512(v: &PerlValue) -> PerlResult<PerlValue> {
    let d = Sha512::digest(bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(d)))
}

/// MD5 digest; lowercase hex (32 chars).
pub(crate) fn md5_digest(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = Md5::new();
    Md5Digest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

/// SHA-1 digest; lowercase hex (40 chars).
pub(crate) fn sha1_digest(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = Sha1::new();
    Sha1Digest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

/// HMAC-SHA256(key, message); both taken as bytes from string values; returns lowercase hex.
pub(crate) fn hmac_sha256(key: &PerlValue, msg: &PerlValue) -> PerlResult<PerlValue> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha256: {}", e), 0))?;
    Mac::update(&mut mac, &bytes_from_value(msg));
    let out = mac.finalize().into_bytes();
    Ok(PerlValue::string(hex::encode(out)))
}

// ── BLAKE2 / BLAKE3 ──────────────────────────────────────────────────────────

/// BLAKE2b-512 digest; lowercase hex (128 chars).
pub(crate) fn blake2b(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = Blake2b512::new();
    Blake2Digest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

/// BLAKE2s-256 digest; lowercase hex (64 chars).
pub(crate) fn blake2s(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = Blake2s256::new();
    Blake2Digest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

/// BLAKE3 digest; lowercase hex (64 chars, 256-bit).
pub(crate) fn blake3_hash(v: &PerlValue) -> PerlResult<PerlValue> {
    let h = blake3::hash(&bytes_from_value(v));
    Ok(PerlValue::string(h.to_hex().to_string()))
}

// ── SHA-3 / Keccak ───────────────────────────────────────────────────────────

/// SHA3-256 digest; lowercase hex (64 chars).
pub(crate) fn sha3_256(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = Sha3_256::new();
    Sha3Digest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

/// SHA3-512 digest; lowercase hex (128 chars).
pub(crate) fn sha3_512(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = Sha3_512::new();
    Sha3Digest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

/// SHAKE128 extendable-output function; returns `len` bytes as hex.
pub(crate) fn shake128(v: &PerlValue, len: &PerlValue) -> PerlResult<PerlValue> {
    use sha3::digest::{ExtendableOutput, Update, XofReader};
    let out_len = len.to_int().max(1) as usize;
    let mut h = Shake128::default();
    Update::update(&mut h, &bytes_from_value(v));
    let mut out = vec![0u8; out_len];
    let mut reader = h.finalize_xof();
    XofReader::read(&mut reader, &mut out);
    Ok(PerlValue::string(hex::encode(out)))
}

/// SHAKE256 extendable-output function; returns `len` bytes as hex.
pub(crate) fn shake256(v: &PerlValue, len: &PerlValue) -> PerlResult<PerlValue> {
    use sha3::digest::{ExtendableOutput, Update, XofReader};
    let out_len = len.to_int().max(1) as usize;
    let mut h = Shake256::default();
    Update::update(&mut h, &bytes_from_value(v));
    let mut out = vec![0u8; out_len];
    let mut reader = h.finalize_xof();
    XofReader::read(&mut reader, &mut out);
    Ok(PerlValue::string(hex::encode(out)))
}

// ── RIPEMD-160 ───────────────────────────────────────────────────────────────

/// RIPEMD-160 digest; lowercase hex (40 chars). Used in Bitcoin addresses.
pub(crate) fn ripemd160(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = Ripemd160::new();
    RipemdDigest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

/// MD4 digest; lowercase hex (32 chars). Legacy, broken — only for compatibility.
pub(crate) fn md4_digest(v: &PerlValue) -> PerlResult<PerlValue> {
    use md4::{Digest as Md4Digest, Md4};
    let mut h = Md4::new();
    Md4Digest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

// ── xxHash ───────────────────────────────────────────────────────────────────

/// xxHash32 — fast non-cryptographic hash. Returns 8 hex chars.
pub(crate) fn xxh32(v: &PerlValue, seed: &PerlValue) -> PerlResult<PerlValue> {
    let s = if seed.is_undef() {
        0
    } else {
        seed.to_int() as u32
    };
    let hash = xxhash_rust::xxh32::xxh32(&bytes_from_value(v), s);
    Ok(PerlValue::string(format!("{:08x}", hash)))
}

/// xxHash64 — fast non-cryptographic hash. Returns 16 hex chars.
pub(crate) fn xxh64(v: &PerlValue, seed: &PerlValue) -> PerlResult<PerlValue> {
    let s = if seed.is_undef() {
        0
    } else {
        seed.to_int() as u64
    };
    let hash = xxhash_rust::xxh64::xxh64(&bytes_from_value(v), s);
    Ok(PerlValue::string(format!("{:016x}", hash)))
}

/// xxHash3-64 — newest xxHash variant, very fast. Returns 16 hex chars.
pub(crate) fn xxh3(v: &PerlValue) -> PerlResult<PerlValue> {
    let hash = xxhash_rust::xxh3::xxh3_64(&bytes_from_value(v));
    Ok(PerlValue::string(format!("{:016x}", hash)))
}

/// xxHash3-128 — 128-bit variant. Returns 32 hex chars.
pub(crate) fn xxh3_128(v: &PerlValue) -> PerlResult<PerlValue> {
    let hash = xxhash_rust::xxh3::xxh3_128(&bytes_from_value(v));
    Ok(PerlValue::string(format!("{:032x}", hash)))
}

// ── MurmurHash ───────────────────────────────────────────────────────────────

/// MurmurHash3 32-bit. Fast non-cryptographic hash. Returns 8 hex chars.
pub(crate) fn murmur3_32(v: &PerlValue, seed: &PerlValue) -> PerlResult<PerlValue> {
    let s = if seed.is_undef() {
        0
    } else {
        seed.to_int() as u32
    };
    let hash = murmur3::murmur3_32(&mut std::io::Cursor::new(bytes_from_value(v)), s)
        .map_err(|e| PerlError::runtime(format!("murmur3: {}", e), 0))?;
    Ok(PerlValue::string(format!("{:08x}", hash)))
}

/// MurmurHash3 128-bit (x64). Returns 32 hex chars.
pub(crate) fn murmur3_128(v: &PerlValue, seed: &PerlValue) -> PerlResult<PerlValue> {
    let s = if seed.is_undef() {
        0
    } else {
        seed.to_int() as u32
    };
    let hash = murmur3::murmur3_x64_128(&mut std::io::Cursor::new(bytes_from_value(v)), s)
        .map_err(|e| PerlError::runtime(format!("murmur3: {}", e), 0))?;
    Ok(PerlValue::string(format!("{:032x}", hash)))
}

// ── SipHash ──────────────────────────────────────────────────────────────────

/// SipHash-2-4 with default key (0,0). Returns 64-bit hash as hex (16 chars).
pub(crate) fn siphash(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = SipHasher24::new();
    h.write(&bytes_from_value(v));
    Ok(PerlValue::string(format!("{:016x}", h.finish())))
}

/// SipHash-2-4 with custom 128-bit key (two u64s). Returns 64-bit hash as hex.
pub(crate) fn siphash_keyed(
    v: &PerlValue,
    k0: &PerlValue,
    k1: &PerlValue,
) -> PerlResult<PerlValue> {
    let mut h = SipHasher24::new_with_keys(k0.to_int() as u64, k1.to_int() as u64);
    h.write(&bytes_from_value(v));
    Ok(PerlValue::string(format!("{:016x}", h.finish())))
}

// ── HMAC Variants ────────────────────────────────────────────────────────────

/// HMAC-SHA1; returns lowercase hex (40 chars).
pub(crate) fn hmac_sha1(key: &PerlValue, msg: &PerlValue) -> PerlResult<PerlValue> {
    let mut mac = <HmacSha1 as Mac>::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha1: {}", e), 0))?;
    Mac::update(&mut mac, &bytes_from_value(msg));
    Ok(PerlValue::string(hex::encode(mac.finalize().into_bytes())))
}

/// HMAC-SHA384; returns lowercase hex (96 chars).
pub(crate) fn hmac_sha384(key: &PerlValue, msg: &PerlValue) -> PerlResult<PerlValue> {
    let mut mac = <HmacSha384 as Mac>::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha384: {}", e), 0))?;
    Mac::update(&mut mac, &bytes_from_value(msg));
    Ok(PerlValue::string(hex::encode(mac.finalize().into_bytes())))
}

/// HMAC-SHA512; returns lowercase hex (128 chars).
pub(crate) fn hmac_sha512(key: &PerlValue, msg: &PerlValue) -> PerlResult<PerlValue> {
    let mut mac = <HmacSha512 as Mac>::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha512: {}", e), 0))?;
    Mac::update(&mut mac, &bytes_from_value(msg));
    Ok(PerlValue::string(hex::encode(mac.finalize().into_bytes())))
}

/// HMAC-MD5; returns lowercase hex (32 chars). Legacy, avoid for new code.
pub(crate) fn hmac_md5(key: &PerlValue, msg: &PerlValue) -> PerlResult<PerlValue> {
    let mut mac = <HmacMd5 as Mac>::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_md5: {}", e), 0))?;
    Mac::update(&mut mac, &bytes_from_value(msg));
    Ok(PerlValue::string(hex::encode(mac.finalize().into_bytes())))
}

// ── HKDF Key Derivation ──────────────────────────────────────────────────────

/// HKDF-SHA256 extract and expand. ikm=input key material, salt (optional), info (optional), len (output bytes).
/// Returns derived key as hex.
pub(crate) fn hkdf_sha256(
    ikm: &PerlValue,
    salt: &PerlValue,
    info: &PerlValue,
    len: &PerlValue,
) -> PerlResult<PerlValue> {
    let salt_bytes = if salt.is_undef() || salt.to_string().is_empty() {
        None
    } else {
        Some(bytes_from_value(salt))
    };
    let hk = Hkdf::<Sha256>::new(salt_bytes.as_deref(), &bytes_from_value(ikm));
    let info_bytes = bytes_from_value(info);
    let out_len = len.to_int().max(1) as usize;
    let mut okm = vec![0u8; out_len];
    hk.expand(&info_bytes, &mut okm)
        .map_err(|e| PerlError::runtime(format!("hkdf_sha256: {}", e), 0))?;
    Ok(PerlValue::string(hex::encode(okm)))
}

/// HKDF-SHA512 extract and expand. Returns derived key as hex.
pub(crate) fn hkdf_sha512(
    ikm: &PerlValue,
    salt: &PerlValue,
    info: &PerlValue,
    len: &PerlValue,
) -> PerlResult<PerlValue> {
    let salt_bytes = if salt.is_undef() || salt.to_string().is_empty() {
        None
    } else {
        Some(bytes_from_value(salt))
    };
    let hk = Hkdf::<Sha512>::new(salt_bytes.as_deref(), &bytes_from_value(ikm));
    let info_bytes = bytes_from_value(info);
    let out_len = len.to_int().max(1) as usize;
    let mut okm = vec![0u8; out_len];
    hk.expand(&info_bytes, &mut okm)
        .map_err(|e| PerlError::runtime(format!("hkdf_sha512: {}", e), 0))?;
    Ok(PerlValue::string(hex::encode(okm)))
}

// ── Base32 Encoding ──────────────────────────────────────────────────────────

/// Base32 encode (RFC 4648). Used in TOTP secrets, onion addresses, etc.
pub(crate) fn base32_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(base32::encode(
        base32::Alphabet::Rfc4648 { padding: true },
        &bytes_from_value(v),
    )))
}

/// Base32 decode (RFC 4648). Returns decoded bytes as string.
pub(crate) fn base32_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    let decoded = base32::decode(base32::Alphabet::Rfc4648 { padding: true }, s.trim())
        .or_else(|| base32::decode(base32::Alphabet::Rfc4648 { padding: false }, s.trim()))
        .ok_or_else(|| PerlError::runtime("base32_decode: invalid base32", 0))?;
    Ok(PerlValue::bytes(Arc::new(decoded)))
}

// ── Base58 Encoding ──────────────────────────────────────────────────────────

const BASE58_ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Base58 encode (Bitcoin alphabet). Used in Bitcoin addresses, IPFS CIDs.
pub(crate) fn base58_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let input = bytes_from_value(v);
    if input.is_empty() {
        return Ok(PerlValue::string(String::new()));
    }
    let mut digits: Vec<u8> = vec![0];
    for byte in &input {
        let mut carry = *byte as usize;
        for digit in &mut digits {
            carry += (*digit as usize) * 256;
            *digit = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let leading_zeros = input.iter().take_while(|&&b| b == 0).count();
    let mut result = String::with_capacity(leading_zeros + digits.len());
    for _ in 0..leading_zeros {
        result.push('1');
    }
    for &d in digits.iter().rev() {
        result.push(BASE58_ALPHABET[d as usize] as char);
    }
    Ok(PerlValue::string(result))
}

/// Base58 decode (Bitcoin alphabet). Returns decoded bytes.
pub(crate) fn base58_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    let s = s.trim();
    if s.is_empty() {
        return Ok(PerlValue::bytes(Arc::new(vec![])));
    }
    let mut digits: Vec<u8> = vec![0];
    for c in s.chars() {
        let val = BASE58_ALPHABET
            .iter()
            .position(|&b| b == c as u8)
            .ok_or_else(|| {
                PerlError::runtime(format!("base58_decode: invalid character '{}'", c), 0)
            })?;
        let mut carry = val;
        for digit in &mut digits {
            carry += (*digit as usize) * 58;
            *digit = (carry % 256) as u8;
            carry /= 256;
        }
        while carry > 0 {
            digits.push((carry % 256) as u8);
            carry /= 256;
        }
    }
    let leading_ones = s.chars().take_while(|&c| c == '1').count();
    let mut result = vec![0u8; leading_ones];
    result.extend(digits.into_iter().rev().skip_while(|&d| d == 0));
    Ok(PerlValue::bytes(Arc::new(result)))
}

// ── TOTP/HOTP ────────────────────────────────────────────────────────────────

/// Generate TOTP code (RFC 6238). secret=base32 encoded, digits=6, period=30.
pub(crate) fn totp_generate(
    secret_b32: &PerlValue,
    digits: &PerlValue,
    period: &PerlValue,
) -> PerlResult<PerlValue> {
    let secret_str = secret_b32.to_string();
    let secret = base32::decode(
        base32::Alphabet::Rfc4648 { padding: false },
        secret_str.trim(),
    )
    .or_else(|| {
        base32::decode(
            base32::Alphabet::Rfc4648 { padding: true },
            secret_str.trim(),
        )
    })
    .ok_or_else(|| PerlError::runtime("totp: invalid base32 secret", 0))?;
    let num_digits = if digits.is_undef() {
        6
    } else {
        digits.to_int() as u32
    };
    let period_secs = if period.is_undef() {
        30
    } else {
        period.to_int() as u64
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let code = totp_lite::totp_custom::<totp_lite::Sha1>(period_secs, num_digits, &secret, now);
    Ok(PerlValue::string(code))
}

/// Verify TOTP code with optional window (default ±1 period).
pub(crate) fn totp_verify(
    secret_b32: &PerlValue,
    code: &PerlValue,
    window: &PerlValue,
) -> PerlResult<PerlValue> {
    let secret_str = secret_b32.to_string();
    let secret = base32::decode(
        base32::Alphabet::Rfc4648 { padding: false },
        secret_str.trim(),
    )
    .or_else(|| {
        base32::decode(
            base32::Alphabet::Rfc4648 { padding: true },
            secret_str.trim(),
        )
    })
    .ok_or_else(|| PerlError::runtime("totp_verify: invalid base32 secret", 0))?;
    let user_code = code.to_string();
    let win = if window.is_undef() {
        1i64
    } else {
        window.to_int()
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for offset in -win..=win {
        let time = (now as i64 + offset * 30) as u64;
        let expected = totp_lite::totp_custom::<totp_lite::Sha1>(30, 6, &secret, time);
        if expected == user_code.trim() {
            return Ok(PerlValue::integer(1));
        }
    }
    Ok(PerlValue::integer(0))
}

/// Generate HOTP code (RFC 4226). secret=base32 encoded, counter, digits=6.
pub(crate) fn hotp_generate(
    secret_b32: &PerlValue,
    counter: &PerlValue,
    digits: &PerlValue,
) -> PerlResult<PerlValue> {
    let secret_str = secret_b32.to_string();
    let secret = base32::decode(
        base32::Alphabet::Rfc4648 { padding: false },
        secret_str.trim(),
    )
    .or_else(|| {
        base32::decode(
            base32::Alphabet::Rfc4648 { padding: true },
            secret_str.trim(),
        )
    })
    .ok_or_else(|| PerlError::runtime("hotp: invalid base32 secret", 0))?;
    let count = counter.to_int() as u64;
    let num_digits = if digits.is_undef() {
        6
    } else {
        digits.to_int() as u32
    };
    let code = totp_lite::totp_custom::<totp_lite::Sha1>(1, num_digits, &secret, count);
    Ok(PerlValue::string(code))
}

// ── AES-CBC ──────────────────────────────────────────────────────────────────

/// AES-256-CBC encrypt with PKCS7 padding. key=32 bytes, iv=16 bytes (or auto-generated).
/// Returns base64(iv || ciphertext).
pub(crate) fn aes_cbc_encrypt(
    key: &PerlValue,
    plaintext: &PerlValue,
    iv: &PerlValue,
) -> PerlResult<PerlValue> {
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!(
                "aes_cbc_encrypt: key must be 32 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let iv_bytes = if iv.is_undef() {
        let mut iv = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut iv);
        iv.to_vec()
    } else {
        let iv = bytes_from_value(iv);
        if iv.len() != 16 {
            return Err(PerlError::runtime(
                format!("aes_cbc_encrypt: iv must be 16 bytes, got {}", iv.len()),
                0,
            ));
        }
        iv
    };
    let plaintext_bytes = bytes_from_value(plaintext);
    let block_size = 16;
    let padded_len = ((plaintext_bytes.len() / block_size) + 1) * block_size;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext_bytes.len()].copy_from_slice(&plaintext_bytes);
    let cipher = Aes256CbcEnc::new(key_bytes.as_slice().into(), iv_bytes.as_slice().into());
    let ciphertext = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_bytes.len())
        .map_err(|e| PerlError::runtime(format!("aes_cbc_encrypt: {}", e), 0))?;
    let mut result = iv_bytes;
    result.extend_from_slice(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// AES-256-CBC decrypt. key=32 bytes, ciphertext=base64(iv || ciphertext).
pub(crate) fn aes_cbc_decrypt(
    key: &PerlValue,
    ciphertext_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!(
                "aes_cbc_decrypt: key must be 32 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let mut data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("aes_cbc_decrypt: invalid base64: {}", e), 0))?;
    if data.len() < 16 {
        return Err(PerlError::runtime(
            "aes_cbc_decrypt: ciphertext too short",
            0,
        ));
    }
    let iv: [u8; 16] = data[..16].try_into().unwrap();
    let ciphertext = &mut data[16..];
    let cipher = Aes256CbcDec::new(key_bytes.as_slice().into(), (&iv).into());
    let plaintext = cipher
        .decrypt_padded_mut::<Pkcs7>(ciphertext)
        .map_err(|e| PerlError::runtime(format!("aes_cbc_decrypt: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(plaintext).into_owned(),
    ))
}

// ── Blowfish Encryption ──────────────────────────────────────────────────────

/// Blowfish-CBC encrypt. key=4-56 bytes, iv=8 bytes (auto-generated if omitted).
/// Returns base64(iv || ciphertext). Legacy cipher — use AES for new code.
pub(crate) fn blowfish_encrypt(
    key: &PerlValue,
    plaintext: &PerlValue,
    iv: &PerlValue,
) -> PerlResult<PerlValue> {
    use blowfish::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type BlowfishCbcEnc = cbc::Encryptor<blowfish::Blowfish>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() < 4 || key_bytes.len() > 56 {
        return Err(PerlError::runtime(
            format!(
                "blowfish_encrypt: key must be 4-56 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let iv_bytes = if iv.is_undef() {
        let mut iv = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut iv);
        iv.to_vec()
    } else {
        let iv = bytes_from_value(iv);
        if iv.len() != 8 {
            return Err(PerlError::runtime(
                format!("blowfish_encrypt: iv must be 8 bytes, got {}", iv.len()),
                0,
            ));
        }
        iv
    };
    let plaintext_bytes = bytes_from_value(plaintext);
    let block_size = 8;
    let padded_len = ((plaintext_bytes.len() / block_size) + 1) * block_size;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext_bytes.len()].copy_from_slice(&plaintext_bytes);
    let cipher = BlowfishCbcEnc::new_from_slices(&key_bytes, &iv_bytes)
        .map_err(|e| PerlError::runtime(format!("blowfish_encrypt: {}", e), 0))?;
    let ciphertext = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_bytes.len())
        .map_err(|e| PerlError::runtime(format!("blowfish_encrypt: {}", e), 0))?;
    let mut result = iv_bytes;
    result.extend_from_slice(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// Blowfish-CBC decrypt. key=4-56 bytes, ciphertext=base64(iv || ciphertext).
pub(crate) fn blowfish_decrypt(
    key: &PerlValue,
    ciphertext_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    use blowfish::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type BlowfishCbcDec = cbc::Decryptor<blowfish::Blowfish>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() < 4 || key_bytes.len() > 56 {
        return Err(PerlError::runtime(
            format!(
                "blowfish_decrypt: key must be 4-56 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let mut data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("blowfish_decrypt: invalid base64: {}", e), 0))?;
    if data.len() < 8 {
        return Err(PerlError::runtime(
            "blowfish_decrypt: ciphertext too short",
            0,
        ));
    }
    let iv: [u8; 8] = data[..8].try_into().unwrap();
    let ciphertext = &mut data[8..];
    let cipher = BlowfishCbcDec::new_from_slices(&key_bytes, &iv)
        .map_err(|e| PerlError::runtime(format!("blowfish_decrypt: {}", e), 0))?;
    let plaintext = cipher
        .decrypt_padded_mut::<Pkcs7>(ciphertext)
        .map_err(|e| PerlError::runtime(format!("blowfish_decrypt: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(plaintext).into_owned(),
    ))
}

// ── Triple DES (3DES) Encryption ─────────────────────────────────────────────

/// 3DES-CBC encrypt. key=24 bytes (3 DES keys), iv=8 bytes (auto-generated).
/// Returns base64(iv || ciphertext). Legacy cipher for PCI-DSS compliance.
pub(crate) fn des3_encrypt(
    key: &PerlValue,
    plaintext: &PerlValue,
    iv: &PerlValue,
) -> PerlResult<PerlValue> {
    use des::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type Des3CbcEnc = cbc::Encryptor<des::TdesEde3>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 24 {
        return Err(PerlError::runtime(
            format!(
                "des3_encrypt: key must be 24 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let iv_bytes = if iv.is_undef() {
        let mut iv = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut iv);
        iv.to_vec()
    } else {
        let iv = bytes_from_value(iv);
        if iv.len() != 8 {
            return Err(PerlError::runtime(
                format!("des3_encrypt: iv must be 8 bytes, got {}", iv.len()),
                0,
            ));
        }
        iv
    };
    let plaintext_bytes = bytes_from_value(plaintext);
    let block_size = 8;
    let padded_len = ((plaintext_bytes.len() / block_size) + 1) * block_size;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext_bytes.len()].copy_from_slice(&plaintext_bytes);
    let cipher = Des3CbcEnc::new_from_slices(&key_bytes, &iv_bytes)
        .map_err(|e| PerlError::runtime(format!("des3_encrypt: {}", e), 0))?;
    let ciphertext = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_bytes.len())
        .map_err(|e| PerlError::runtime(format!("des3_encrypt: {}", e), 0))?;
    let mut result = iv_bytes;
    result.extend_from_slice(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// 3DES-CBC decrypt. key=24 bytes, ciphertext=base64(iv || ciphertext).
pub(crate) fn des3_decrypt(key: &PerlValue, ciphertext_b64: &PerlValue) -> PerlResult<PerlValue> {
    use des::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type Des3CbcDec = cbc::Decryptor<des::TdesEde3>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 24 {
        return Err(PerlError::runtime(
            format!(
                "des3_decrypt: key must be 24 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let mut data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("des3_decrypt: invalid base64: {}", e), 0))?;
    if data.len() < 8 {
        return Err(PerlError::runtime("des3_decrypt: ciphertext too short", 0));
    }
    let iv: [u8; 8] = data[..8].try_into().unwrap();
    let ciphertext = &mut data[8..];
    let cipher = Des3CbcDec::new_from_slices(&key_bytes, &iv)
        .map_err(|e| PerlError::runtime(format!("des3_decrypt: {}", e), 0))?;
    let plaintext = cipher
        .decrypt_padded_mut::<Pkcs7>(ciphertext)
        .map_err(|e| PerlError::runtime(format!("des3_decrypt: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(plaintext).into_owned(),
    ))
}

// ── Twofish Encryption ───────────────────────────────────────────────────────

/// Twofish-CBC encrypt. key=16/24/32 bytes, iv=16 bytes (auto-generated).
/// Returns base64(iv || ciphertext). AES finalist, still secure.
pub(crate) fn twofish_encrypt(
    key: &PerlValue,
    plaintext: &PerlValue,
    iv: &PerlValue,
) -> PerlResult<PerlValue> {
    use twofish::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type TwofishCbcEnc = cbc::Encryptor<twofish::Twofish>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 16 && key_bytes.len() != 24 && key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!(
                "twofish_encrypt: key must be 16/24/32 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let iv_bytes = if iv.is_undef() {
        let mut iv = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut iv);
        iv.to_vec()
    } else {
        let iv = bytes_from_value(iv);
        if iv.len() != 16 {
            return Err(PerlError::runtime(
                format!("twofish_encrypt: iv must be 16 bytes, got {}", iv.len()),
                0,
            ));
        }
        iv
    };
    let plaintext_bytes = bytes_from_value(plaintext);
    let block_size = 16;
    let padded_len = ((plaintext_bytes.len() / block_size) + 1) * block_size;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext_bytes.len()].copy_from_slice(&plaintext_bytes);
    let cipher = TwofishCbcEnc::new_from_slices(&key_bytes, &iv_bytes)
        .map_err(|e| PerlError::runtime(format!("twofish_encrypt: {}", e), 0))?;
    let ciphertext = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_bytes.len())
        .map_err(|e| PerlError::runtime(format!("twofish_encrypt: {}", e), 0))?;
    let mut result = iv_bytes;
    result.extend_from_slice(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// Twofish-CBC decrypt.
pub(crate) fn twofish_decrypt(
    key: &PerlValue,
    ciphertext_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    use twofish::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type TwofishCbcDec = cbc::Decryptor<twofish::Twofish>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 16 && key_bytes.len() != 24 && key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!(
                "twofish_decrypt: key must be 16/24/32 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let mut data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("twofish_decrypt: invalid base64: {}", e), 0))?;
    if data.len() < 16 {
        return Err(PerlError::runtime(
            "twofish_decrypt: ciphertext too short",
            0,
        ));
    }
    let iv: [u8; 16] = data[..16].try_into().unwrap();
    let ciphertext = &mut data[16..];
    let cipher = TwofishCbcDec::new_from_slices(&key_bytes, &iv)
        .map_err(|e| PerlError::runtime(format!("twofish_decrypt: {}", e), 0))?;
    let plaintext = cipher
        .decrypt_padded_mut::<Pkcs7>(ciphertext)
        .map_err(|e| PerlError::runtime(format!("twofish_decrypt: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(plaintext).into_owned(),
    ))
}

// ── Camellia Encryption ──────────────────────────────────────────────────────

/// Camellia-CBC encrypt. key=16/24/32 bytes. Japanese/EU standard cipher.
pub(crate) fn camellia_encrypt(
    key: &PerlValue,
    plaintext: &PerlValue,
    iv: &PerlValue,
) -> PerlResult<PerlValue> {
    use camellia::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};

    let key_bytes = bytes_from_value(key);
    let iv_bytes = if iv.is_undef() {
        let mut iv = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut iv);
        iv.to_vec()
    } else {
        let iv = bytes_from_value(iv);
        if iv.len() != 16 {
            return Err(PerlError::runtime(
                format!("camellia_encrypt: iv must be 16 bytes, got {}", iv.len()),
                0,
            ));
        }
        iv
    };
    let plaintext_bytes = bytes_from_value(plaintext);
    let block_size = 16;
    let padded_len = ((plaintext_bytes.len() / block_size) + 1) * block_size;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext_bytes.len()].copy_from_slice(&plaintext_bytes);

    let ciphertext = match key_bytes.len() {
        16 => {
            type Enc = cbc::Encryptor<camellia::Camellia128>;
            let cipher = Enc::new_from_slices(&key_bytes, &iv_bytes)
                .map_err(|e| PerlError::runtime(format!("camellia_encrypt: {}", e), 0))?;
            cipher
                .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_bytes.len())
                .map_err(|e| PerlError::runtime(format!("camellia_encrypt: {}", e), 0))?
                .to_vec()
        }
        24 => {
            type Enc = cbc::Encryptor<camellia::Camellia192>;
            let cipher = Enc::new_from_slices(&key_bytes, &iv_bytes)
                .map_err(|e| PerlError::runtime(format!("camellia_encrypt: {}", e), 0))?;
            cipher
                .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_bytes.len())
                .map_err(|e| PerlError::runtime(format!("camellia_encrypt: {}", e), 0))?
                .to_vec()
        }
        32 => {
            type Enc = cbc::Encryptor<camellia::Camellia256>;
            let cipher = Enc::new_from_slices(&key_bytes, &iv_bytes)
                .map_err(|e| PerlError::runtime(format!("camellia_encrypt: {}", e), 0))?;
            cipher
                .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_bytes.len())
                .map_err(|e| PerlError::runtime(format!("camellia_encrypt: {}", e), 0))?
                .to_vec()
        }
        _ => {
            return Err(PerlError::runtime(
                format!(
                    "camellia_encrypt: key must be 16/24/32 bytes, got {}",
                    key_bytes.len()
                ),
                0,
            ))
        }
    };

    let mut result = iv_bytes;
    result.extend(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// Camellia-CBC decrypt.
pub(crate) fn camellia_decrypt(
    key: &PerlValue,
    ciphertext_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    use camellia::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};

    let key_bytes = bytes_from_value(key);
    let mut data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("camellia_decrypt: invalid base64: {}", e), 0))?;
    if data.len() < 16 {
        return Err(PerlError::runtime(
            "camellia_decrypt: ciphertext too short",
            0,
        ));
    }
    let iv: [u8; 16] = data[..16].try_into().unwrap();
    let ciphertext = &mut data[16..];

    let plaintext = match key_bytes.len() {
        16 => {
            type Dec = cbc::Decryptor<camellia::Camellia128>;
            let cipher = Dec::new_from_slices(&key_bytes, &iv)
                .map_err(|e| PerlError::runtime(format!("camellia_decrypt: {}", e), 0))?;
            cipher
                .decrypt_padded_mut::<Pkcs7>(ciphertext)
                .map_err(|e| PerlError::runtime(format!("camellia_decrypt: {}", e), 0))?
                .to_vec()
        }
        24 => {
            type Dec = cbc::Decryptor<camellia::Camellia192>;
            let cipher = Dec::new_from_slices(&key_bytes, &iv)
                .map_err(|e| PerlError::runtime(format!("camellia_decrypt: {}", e), 0))?;
            cipher
                .decrypt_padded_mut::<Pkcs7>(ciphertext)
                .map_err(|e| PerlError::runtime(format!("camellia_decrypt: {}", e), 0))?
                .to_vec()
        }
        32 => {
            type Dec = cbc::Decryptor<camellia::Camellia256>;
            let cipher = Dec::new_from_slices(&key_bytes, &iv)
                .map_err(|e| PerlError::runtime(format!("camellia_decrypt: {}", e), 0))?;
            cipher
                .decrypt_padded_mut::<Pkcs7>(ciphertext)
                .map_err(|e| PerlError::runtime(format!("camellia_decrypt: {}", e), 0))?
                .to_vec()
        }
        _ => {
            return Err(PerlError::runtime(
                format!(
                    "camellia_decrypt: key must be 16/24/32 bytes, got {}",
                    key_bytes.len()
                ),
                0,
            ))
        }
    };

    Ok(PerlValue::string(
        String::from_utf8_lossy(&plaintext).into_owned(),
    ))
}

// ── CAST5 Encryption ─────────────────────────────────────────────────────────

/// CAST5-CBC encrypt. key=5-16 bytes. Used in PGP.
pub(crate) fn cast5_encrypt(
    key: &PerlValue,
    plaintext: &PerlValue,
    iv: &PerlValue,
) -> PerlResult<PerlValue> {
    use cast5::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type Cast5CbcEnc = cbc::Encryptor<cast5::Cast5>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() < 5 || key_bytes.len() > 16 {
        return Err(PerlError::runtime(
            format!(
                "cast5_encrypt: key must be 5-16 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let iv_bytes = if iv.is_undef() {
        let mut iv = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut iv);
        iv.to_vec()
    } else {
        let iv = bytes_from_value(iv);
        if iv.len() != 8 {
            return Err(PerlError::runtime(
                format!("cast5_encrypt: iv must be 8 bytes, got {}", iv.len()),
                0,
            ));
        }
        iv
    };
    let plaintext_bytes = bytes_from_value(plaintext);
    let block_size = 8;
    let padded_len = ((plaintext_bytes.len() / block_size) + 1) * block_size;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext_bytes.len()].copy_from_slice(&plaintext_bytes);
    let cipher = Cast5CbcEnc::new_from_slices(&key_bytes, &iv_bytes)
        .map_err(|e| PerlError::runtime(format!("cast5_encrypt: {}", e), 0))?;
    let ciphertext = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_bytes.len())
        .map_err(|e| PerlError::runtime(format!("cast5_encrypt: {}", e), 0))?;
    let mut result = iv_bytes;
    result.extend_from_slice(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// CAST5-CBC decrypt.
pub(crate) fn cast5_decrypt(key: &PerlValue, ciphertext_b64: &PerlValue) -> PerlResult<PerlValue> {
    use cast5::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type Cast5CbcDec = cbc::Decryptor<cast5::Cast5>;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() < 5 || key_bytes.len() > 16 {
        return Err(PerlError::runtime(
            format!(
                "cast5_decrypt: key must be 5-16 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let mut data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("cast5_decrypt: invalid base64: {}", e), 0))?;
    if data.len() < 8 {
        return Err(PerlError::runtime("cast5_decrypt: ciphertext too short", 0));
    }
    let iv: [u8; 8] = data[..8].try_into().unwrap();
    let ciphertext = &mut data[8..];
    let cipher = Cast5CbcDec::new_from_slices(&key_bytes, &iv)
        .map_err(|e| PerlError::runtime(format!("cast5_decrypt: {}", e), 0))?;
    let plaintext = cipher
        .decrypt_padded_mut::<Pkcs7>(ciphertext)
        .map_err(|e| PerlError::runtime(format!("cast5_decrypt: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(plaintext).into_owned(),
    ))
}

// ── Salsa20 / XSalsa20 Stream Cipher ─────────────────────────────────────────

/// Salsa20 encrypt/decrypt (stream cipher). key=32 bytes, nonce=8 bytes.
/// Returns base64(nonce || ciphertext).
pub(crate) fn salsa20_crypt(key: &PerlValue, data: &PerlValue) -> PerlResult<PerlValue> {
    use salsa20::cipher::{KeyIvInit, StreamCipher};
    use salsa20::Salsa20;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!("salsa20: key must be 32 bytes, got {}", key_bytes.len()),
            0,
        ));
    }
    let mut nonce = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut nonce);
    let mut cipher = Salsa20::new(key_bytes.as_slice().into(), (&nonce).into());
    let mut buf = bytes_from_value(data);
    cipher.apply_keystream(&mut buf);
    let mut result = nonce.to_vec();
    result.extend(buf);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// Salsa20 decrypt.
pub(crate) fn salsa20_decrypt(
    key: &PerlValue,
    ciphertext_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    use salsa20::cipher::{KeyIvInit, StreamCipher};
    use salsa20::Salsa20;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!("salsa20: key must be 32 bytes, got {}", key_bytes.len()),
            0,
        ));
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("salsa20: invalid base64: {}", e), 0))?;
    if data.len() < 8 {
        return Err(PerlError::runtime("salsa20: ciphertext too short", 0));
    }
    let nonce: [u8; 8] = data[..8].try_into().unwrap();
    let mut buf = data[8..].to_vec();
    let mut cipher = Salsa20::new(key_bytes.as_slice().into(), (&nonce).into());
    cipher.apply_keystream(&mut buf);
    Ok(PerlValue::string(
        String::from_utf8_lossy(&buf).into_owned(),
    ))
}

/// XSalsa20 encrypt (extended 24-byte nonce). key=32 bytes.
/// Returns base64(nonce || ciphertext).
pub(crate) fn xsalsa20_crypt(key: &PerlValue, data: &PerlValue) -> PerlResult<PerlValue> {
    use salsa20::cipher::{KeyIvInit, StreamCipher};
    use salsa20::XSalsa20;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!("xsalsa20: key must be 32 bytes, got {}", key_bytes.len()),
            0,
        ));
    }
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let mut cipher = XSalsa20::new(key_bytes.as_slice().into(), (&nonce).into());
    let mut buf = bytes_from_value(data);
    cipher.apply_keystream(&mut buf);
    let mut result = nonce.to_vec();
    result.extend(buf);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// XSalsa20 decrypt.
pub(crate) fn xsalsa20_decrypt(
    key: &PerlValue,
    ciphertext_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    use salsa20::cipher::{KeyIvInit, StreamCipher};
    use salsa20::XSalsa20;

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!("xsalsa20: key must be 32 bytes, got {}", key_bytes.len()),
            0,
        ));
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("xsalsa20: invalid base64: {}", e), 0))?;
    if data.len() < 24 {
        return Err(PerlError::runtime("xsalsa20: ciphertext too short", 0));
    }
    let nonce: [u8; 24] = data[..24].try_into().unwrap();
    let mut buf = data[24..].to_vec();
    let mut cipher = XSalsa20::new(key_bytes.as_slice().into(), (&nonce).into());
    cipher.apply_keystream(&mut buf);
    Ok(PerlValue::string(
        String::from_utf8_lossy(&buf).into_owned(),
    ))
}

// ── NaCl secretbox (XSalsa20-Poly1305) ───────────────────────────────────────

/// NaCl secretbox seal. Symmetric authenticated encryption (XSalsa20-Poly1305).
/// key=32 bytes. Returns base64(nonce || ciphertext || tag).
pub(crate) fn secretbox_seal(key: &PerlValue, plaintext: &PerlValue) -> PerlResult<PerlValue> {
    use crypto_secretbox::{aead::Aead, KeyInit, XSalsa20Poly1305};

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!("secretbox: key must be 32 bytes, got {}", key_bytes.len()),
            0,
        ));
    }
    let cipher = XSalsa20Poly1305::new(key_bytes.as_slice().into());
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt((&nonce).into(), bytes_from_value(plaintext).as_ref())
        .map_err(|e| PerlError::runtime(format!("secretbox: {}", e), 0))?;
    let mut result = nonce.to_vec();
    result.extend(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// NaCl secretbox open. Decrypt and verify.
pub(crate) fn secretbox_open(key: &PerlValue, ciphertext_b64: &PerlValue) -> PerlResult<PerlValue> {
    use crypto_secretbox::{aead::Aead, KeyInit, XSalsa20Poly1305};

    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!("secretbox: key must be 32 bytes, got {}", key_bytes.len()),
            0,
        ));
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("secretbox: invalid base64: {}", e), 0))?;
    if data.len() < 24 + 16 {
        return Err(PerlError::runtime("secretbox: ciphertext too short", 0));
    }
    let nonce: [u8; 24] = data[..24].try_into().unwrap();
    let cipher = XSalsa20Poly1305::new(key_bytes.as_slice().into());
    let plaintext = cipher
        .decrypt((&nonce).into(), &data[24..])
        .map_err(|_| PerlError::runtime("secretbox: decryption failed (bad key or tampered)", 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&plaintext).into_owned(),
    ))
}

// ── NaCl box (X25519 + XSalsa20-Poly1305) ────────────────────────────────────

/// NaCl box keypair generation. Returns [secret_key_hex, public_key_hex].
pub(crate) fn nacl_box_keygen() -> PerlResult<PerlValue> {
    use crypto_box::SecretKey;

    let sk = SecretKey::generate(&mut rand::thread_rng());
    let pk = sk.public_key();
    Ok(PerlValue::array(vec![
        PerlValue::string(hex::encode(sk.to_bytes())),
        PerlValue::string(hex::encode(pk.to_bytes())),
    ]))
}

/// NaCl box seal. Asymmetric authenticated encryption.
/// Takes recipient's public key, sender's secret key, and plaintext.
pub(crate) fn nacl_box_seal(
    recipient_pk_hex: &PerlValue,
    sender_sk_hex: &PerlValue,
    plaintext: &PerlValue,
) -> PerlResult<PerlValue> {
    use crypto_box::{aead::Aead, PublicKey, SalsaBox, SecretKey};

    let pk_bytes = hex::decode(recipient_pk_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("nacl_box: invalid public key hex: {}", e), 0))?;
    let sk_bytes = hex::decode(sender_sk_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("nacl_box: invalid secret key hex: {}", e), 0))?;
    if pk_bytes.len() != 32 || sk_bytes.len() != 32 {
        return Err(PerlError::runtime("nacl_box: keys must be 32 bytes", 0));
    }
    let pk = PublicKey::from_slice(&pk_bytes)
        .map_err(|e| PerlError::runtime(format!("nacl_box: {}", e), 0))?;
    let sk = SecretKey::from_slice(&sk_bytes)
        .map_err(|e| PerlError::runtime(format!("nacl_box: {}", e), 0))?;
    let salsa_box = SalsaBox::new(&pk, &sk);
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ciphertext = salsa_box
        .encrypt((&nonce).into(), bytes_from_value(plaintext).as_ref())
        .map_err(|e| PerlError::runtime(format!("nacl_box: {}", e), 0))?;
    let mut result = nonce.to_vec();
    result.extend(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&result),
    ))
}

/// NaCl box open. Decrypt with sender's public key and recipient's secret key.
pub(crate) fn nacl_box_open(
    sender_pk_hex: &PerlValue,
    recipient_sk_hex: &PerlValue,
    ciphertext_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    use crypto_box::{aead::Aead, PublicKey, SalsaBox, SecretKey};

    let pk_bytes = hex::decode(sender_pk_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("nacl_box: invalid public key hex: {}", e), 0))?;
    let sk_bytes = hex::decode(recipient_sk_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("nacl_box: invalid secret key hex: {}", e), 0))?;
    if pk_bytes.len() != 32 || sk_bytes.len() != 32 {
        return Err(PerlError::runtime("nacl_box: keys must be 32 bytes", 0));
    }
    let pk = PublicKey::from_slice(&pk_bytes)
        .map_err(|e| PerlError::runtime(format!("nacl_box: {}", e), 0))?;
    let sk = SecretKey::from_slice(&sk_bytes)
        .map_err(|e| PerlError::runtime(format!("nacl_box: {}", e), 0))?;
    let data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("nacl_box: invalid base64: {}", e), 0))?;
    if data.len() < 24 + 16 {
        return Err(PerlError::runtime("nacl_box: ciphertext too short", 0));
    }
    let nonce: [u8; 24] = data[..24].try_into().unwrap();
    let salsa_box = SalsaBox::new(&pk, &sk);
    let plaintext = salsa_box
        .decrypt((&nonce).into(), &data[24..])
        .map_err(|_| PerlError::runtime("nacl_box: decryption failed (bad key or tampered)", 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&plaintext).into_owned(),
    ))
}

// ── QR Code Generation ───────────────────────────────────────────────────────

/// Generate QR code as ASCII art. Returns multi-line string.
pub(crate) fn qr_ascii(data: &PerlValue) -> PerlResult<PerlValue> {
    use qrcode::QrCode;
    let code = QrCode::new(data.to_string().as_bytes())
        .map_err(|e| PerlError::runtime(format!("qr_ascii: {}", e), 0))?;
    let string = code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(2, 1)
        .build();
    Ok(PerlValue::string(string))
}

/// Generate QR code as PNG bytes (base64 encoded). Optional size parameter.
pub(crate) fn qr_png(data: &PerlValue, size: &PerlValue) -> PerlResult<PerlValue> {
    use image::{ImageEncoder, Luma};
    use qrcode::QrCode;

    let code = QrCode::new(data.to_string().as_bytes())
        .map_err(|e| PerlError::runtime(format!("qr_png: {}", e), 0))?;
    let scale = if size.is_undef() {
        8u32
    } else {
        size.to_int().max(1) as u32
    };
    let img = code
        .render::<Luma<u8>>()
        .quiet_zone(true)
        .min_dimensions(scale, scale)
        .build();
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(
            &img,
            img.width(),
            img.height(),
            image::ExtendedColorType::L8,
        )
        .map_err(|e| PerlError::runtime(format!("qr_png: {}", e), 0))?;
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&png_bytes),
    ))
}

/// Generate QR code as SVG string.
pub(crate) fn qr_svg(data: &PerlValue) -> PerlResult<PerlValue> {
    use qrcode::render::svg;
    use qrcode::QrCode;

    let code = QrCode::new(data.to_string().as_bytes())
        .map_err(|e| PerlError::runtime(format!("qr_svg: {}", e), 0))?;
    let svg_string = code
        .render()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(PerlValue::string(svg_string))
}

// ── Barcode Generation ───────────────────────────────────────────────────────

/// Generate Code128 barcode as ASCII. Returns multi-line string.
pub(crate) fn barcode_code128(data: &PerlValue) -> PerlResult<PerlValue> {
    use barcoders::sym::code128::Code128;
    let barcode = Code128::new(data.to_string())
        .map_err(|e| PerlError::runtime(format!("barcode_code128: {}", e), 0))?;
    let encoded = barcode.encode();
    let ascii: String = encoded
        .iter()
        .map(|&b| if b == 1 { '█' } else { ' ' })
        .collect();
    Ok(PerlValue::string(ascii))
}

/// Generate Code39 barcode as ASCII.
pub(crate) fn barcode_code39(data: &PerlValue) -> PerlResult<PerlValue> {
    use barcoders::sym::code39::Code39;
    let barcode = Code39::new(data.to_string())
        .map_err(|e| PerlError::runtime(format!("barcode_code39: {}", e), 0))?;
    let encoded = barcode.encode();
    let ascii: String = encoded
        .iter()
        .map(|&b| if b == 1 { '█' } else { ' ' })
        .collect();
    Ok(PerlValue::string(ascii))
}

/// Generate EAN-13 barcode as ASCII.
pub(crate) fn barcode_ean13(data: &PerlValue) -> PerlResult<PerlValue> {
    use barcoders::sym::ean13::EAN13;
    let barcode = EAN13::new(data.to_string())
        .map_err(|e| PerlError::runtime(format!("barcode_ean13: {}", e), 0))?;
    let encoded = barcode.encode();
    let ascii: String = encoded
        .iter()
        .map(|&b| if b == 1 { '█' } else { ' ' })
        .collect();
    Ok(PerlValue::string(ascii))
}

/// Generate barcode as SVG. type = "code128", "code39", "ean13", "upca".
pub(crate) fn barcode_svg(data: &PerlValue, barcode_type: &PerlValue) -> PerlResult<PerlValue> {
    use barcoders::generators::svg::SVG;

    let data_str = data.to_string();
    let bc_type = barcode_type.to_string();
    let bc_type = bc_type.trim().to_lowercase();

    let encoded: Vec<u8> = match bc_type.as_str() {
        "code128" | "" => {
            use barcoders::sym::code128::Code128;
            Code128::new(&data_str)
                .map_err(|e| PerlError::runtime(format!("barcode_svg: {}", e), 0))?
                .encode()
        }
        "code39" => {
            use barcoders::sym::code39::Code39;
            Code39::new(&data_str)
                .map_err(|e| PerlError::runtime(format!("barcode_svg: {}", e), 0))?
                .encode()
        }
        "ean13" => {
            use barcoders::sym::ean13::EAN13;
            EAN13::new(&data_str)
                .map_err(|e| PerlError::runtime(format!("barcode_svg: {}", e), 0))?
                .encode()
        }
        "upca" => {
            use barcoders::sym::ean13::UPCA;
            UPCA::new(&data_str)
                .map_err(|e| PerlError::runtime(format!("barcode_svg: {}", e), 0))?
                .encode()
        }
        _ => {
            return Err(PerlError::runtime(
                format!(
                    "barcode_svg: unknown type '{}', use code128/code39/ean13/upca",
                    bc_type
                ),
                0,
            ))
        }
    };

    let svg = SVG::new(50)
        .generate(&encoded)
        .map_err(|e| PerlError::runtime(format!("barcode_svg: {}", e), 0))?;
    Ok(PerlValue::string(svg))
}

// ── Poly1305 Standalone MAC ──────────────────────────────────────────────────

/// Poly1305 one-time MAC. key=32 bytes. Returns 128-bit tag as hex (32 chars).
pub(crate) fn poly1305_mac(key: &PerlValue, msg: &PerlValue) -> PerlResult<PerlValue> {
    use poly1305::{universal_hash::UniversalHash, Key, Poly1305};
    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!("poly1305: key must be 32 bytes, got {}", key_bytes.len()),
            0,
        ));
    }
    let key = Key::from_slice(&key_bytes);
    let mut mac = Poly1305::new(key);
    mac.update_padded(&bytes_from_value(msg));
    let tag = mac.finalize();
    Ok(PerlValue::string(hex::encode(tag.as_slice())))
}

// ── RSA ──────────────────────────────────────────────────────────────────────

/// Generate RSA keypair. bits = key size (2048, 3072, 4096). Returns [private_pem, public_pem].
pub(crate) fn rsa_keygen(bits: &PerlValue) -> PerlResult<PerlValue> {
    let key_bits = bits.to_int().max(2048) as usize;
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, key_bits)
        .map_err(|e| PerlError::runtime(format!("rsa_keygen: {}", e), 0))?;
    let pub_key = priv_key.to_public_key();
    let priv_pem = priv_key
        .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
        .map_err(|e| PerlError::runtime(format!("rsa_keygen: {}", e), 0))?;
    let pub_pem = pub_key
        .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
        .map_err(|e| PerlError::runtime(format!("rsa_keygen: {}", e), 0))?;
    Ok(PerlValue::array(vec![
        PerlValue::string(priv_pem.to_string()),
        PerlValue::string(pub_pem),
    ]))
}

/// RSA-OAEP encrypt with SHA-256. public_key_pem, plaintext. Returns base64 ciphertext.
pub(crate) fn rsa_encrypt(pub_pem: &PerlValue, plaintext: &PerlValue) -> PerlResult<PerlValue> {
    let pub_key = RsaPublicKey::from_public_key_pem(&pub_pem.to_string())
        .map_err(|e| PerlError::runtime(format!("rsa_encrypt: invalid public key: {}", e), 0))?;
    let mut rng = rand::thread_rng();
    let padding = Oaep::new::<Sha256>();
    let ciphertext = pub_key
        .encrypt(&mut rng, padding, &bytes_from_value(plaintext))
        .map_err(|e| PerlError::runtime(format!("rsa_encrypt: {}", e), 0))?;
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&ciphertext),
    ))
}

/// RSA-OAEP decrypt with SHA-256. private_key_pem, base64_ciphertext. Returns plaintext.
pub(crate) fn rsa_decrypt(
    priv_pem: &PerlValue,
    ciphertext_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    let priv_key = RsaPrivateKey::from_pkcs8_pem(&priv_pem.to_string())
        .map_err(|e| PerlError::runtime(format!("rsa_decrypt: invalid private key: {}", e), 0))?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("rsa_decrypt: invalid base64: {}", e), 0))?;
    let padding = Oaep::new::<Sha256>();
    let plaintext = priv_key
        .decrypt(padding, &ciphertext)
        .map_err(|e| PerlError::runtime(format!("rsa_decrypt: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&plaintext).into_owned(),
    ))
}

/// RSA-PKCS1v15 encrypt (legacy). public_key_pem, plaintext. Returns base64 ciphertext.
pub(crate) fn rsa_encrypt_pkcs1(
    pub_pem: &PerlValue,
    plaintext: &PerlValue,
) -> PerlResult<PerlValue> {
    let pub_key = RsaPublicKey::from_public_key_pem(&pub_pem.to_string()).map_err(|e| {
        PerlError::runtime(format!("rsa_encrypt_pkcs1: invalid public key: {}", e), 0)
    })?;
    let mut rng = rand::thread_rng();
    let ciphertext = pub_key
        .encrypt(&mut rng, Pkcs1v15Encrypt, &bytes_from_value(plaintext))
        .map_err(|e| PerlError::runtime(format!("rsa_encrypt_pkcs1: {}", e), 0))?;
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&ciphertext),
    ))
}

/// RSA-PKCS1v15 decrypt (legacy). private_key_pem, base64_ciphertext. Returns plaintext.
pub(crate) fn rsa_decrypt_pkcs1(
    priv_pem: &PerlValue,
    ciphertext_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    let priv_key = RsaPrivateKey::from_pkcs8_pem(&priv_pem.to_string()).map_err(|e| {
        PerlError::runtime(format!("rsa_decrypt_pkcs1: invalid private key: {}", e), 0)
    })?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("rsa_decrypt_pkcs1: invalid base64: {}", e), 0))?;
    let plaintext = priv_key
        .decrypt(Pkcs1v15Encrypt, &ciphertext)
        .map_err(|e| PerlError::runtime(format!("rsa_decrypt_pkcs1: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&plaintext).into_owned(),
    ))
}

/// RSA-PKCS1v15-SHA256 sign. private_key_pem, message. Returns base64 signature.
pub(crate) fn rsa_sign(priv_pem: &PerlValue, message: &PerlValue) -> PerlResult<PerlValue> {
    let priv_key = RsaPrivateKey::from_pkcs8_pem(&priv_pem.to_string())
        .map_err(|e| PerlError::runtime(format!("rsa_sign: invalid private key: {}", e), 0))?;
    let signing_key = RsaSigningKey::<Sha256>::new_unprefixed(priv_key);
    let sig = signing_key.sign(&bytes_from_value(message));
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(sig.to_bytes()),
    ))
}

/// RSA-PKCS1v15-SHA256 verify. public_key_pem, message, base64_signature. Returns 1 if valid.
pub(crate) fn rsa_verify(
    pub_pem: &PerlValue,
    message: &PerlValue,
    signature_b64: &PerlValue,
) -> PerlResult<PerlValue> {
    let pub_key = RsaPublicKey::from_public_key_pem(&pub_pem.to_string())
        .map_err(|e| PerlError::runtime(format!("rsa_verify: invalid public key: {}", e), 0))?;
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("rsa_verify: invalid base64: {}", e), 0))?;
    let verifying_key = RsaVerifyingKey::<Sha256>::new_unprefixed(pub_key);
    let sig = rsa::pkcs1v15::Signature::try_from(sig_bytes.as_slice())
        .map_err(|e| PerlError::runtime(format!("rsa_verify: invalid signature: {}", e), 0))?;
    let ok = verifying_key
        .verify(&bytes_from_value(message), &sig)
        .is_ok();
    Ok(PerlValue::integer(i64::from(ok)))
}

// ── ECDSA (P-256, P-384, secp256k1) ──────────────────────────────────────────

/// Generate ECDSA P-256 keypair. Returns [private_hex, public_hex_compressed].
pub(crate) fn ecdsa_p256_keygen() -> PerlResult<PerlValue> {
    use p256::ecdsa::SigningKey;
    let sk = SigningKey::random(&mut rand::thread_rng());
    let vk = sk.verifying_key();
    let priv_hex = hex::encode(sk.to_bytes());
    let pub_hex = hex::encode(vk.to_encoded_point(true).as_bytes());
    Ok(PerlValue::array(vec![
        PerlValue::string(priv_hex),
        PerlValue::string(pub_hex),
    ]))
}

/// ECDSA P-256 sign. private_key_hex, message. Returns signature as hex (DER-encoded).
pub(crate) fn ecdsa_p256_sign(priv_hex: &PerlValue, message: &PerlValue) -> PerlResult<PerlValue> {
    use p256::ecdsa::{signature::Signer, SigningKey};
    let priv_bytes = hex::decode(priv_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ecdsa_p256_sign: invalid hex: {}", e), 0))?;
    let sk = SigningKey::from_bytes(priv_bytes.as_slice().into())
        .map_err(|e| PerlError::runtime(format!("ecdsa_p256_sign: invalid key: {}", e), 0))?;
    let sig: p256::ecdsa::Signature = sk.sign(&bytes_from_value(message));
    Ok(PerlValue::string(hex::encode(sig.to_der().as_bytes())))
}

/// ECDSA P-256 verify. public_key_hex, message, signature_hex. Returns 1 if valid.
pub(crate) fn ecdsa_p256_verify(
    pub_hex: &PerlValue,
    message: &PerlValue,
    sig_hex: &PerlValue,
) -> PerlResult<PerlValue> {
    use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
    use p256::EncodedPoint;
    let pub_bytes = hex::decode(pub_hex.to_string().trim()).map_err(|e| {
        PerlError::runtime(format!("ecdsa_p256_verify: invalid hex pubkey: {}", e), 0)
    })?;
    let point = EncodedPoint::from_bytes(&pub_bytes)
        .map_err(|e| PerlError::runtime(format!("ecdsa_p256_verify: invalid point: {}", e), 0))?;
    let vk = VerifyingKey::from_encoded_point(&point)
        .map_err(|e| PerlError::runtime(format!("ecdsa_p256_verify: invalid pubkey: {}", e), 0))?;
    let sig_bytes = hex::decode(sig_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ecdsa_p256_verify: invalid hex sig: {}", e), 0))?;
    let sig = Signature::from_der(&sig_bytes).map_err(|e| {
        PerlError::runtime(format!("ecdsa_p256_verify: invalid signature: {}", e), 0)
    })?;
    let ok = vk.verify(&bytes_from_value(message), &sig).is_ok();
    Ok(PerlValue::integer(i64::from(ok)))
}

/// Generate ECDSA P-384 keypair. Returns [private_hex, public_hex_compressed].
pub(crate) fn ecdsa_p384_keygen() -> PerlResult<PerlValue> {
    use p384::ecdsa::SigningKey;
    let sk = SigningKey::random(&mut rand::thread_rng());
    let vk = sk.verifying_key();
    let priv_hex = hex::encode(sk.to_bytes());
    let pub_hex = hex::encode(vk.to_encoded_point(true).as_bytes());
    Ok(PerlValue::array(vec![
        PerlValue::string(priv_hex),
        PerlValue::string(pub_hex),
    ]))
}

/// ECDSA P-384 sign. Returns signature as hex (DER-encoded).
pub(crate) fn ecdsa_p384_sign(priv_hex: &PerlValue, message: &PerlValue) -> PerlResult<PerlValue> {
    use p384::ecdsa::{signature::Signer, SigningKey};
    let priv_bytes = hex::decode(priv_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ecdsa_p384_sign: invalid hex: {}", e), 0))?;
    let sk = SigningKey::from_bytes(priv_bytes.as_slice().into())
        .map_err(|e| PerlError::runtime(format!("ecdsa_p384_sign: invalid key: {}", e), 0))?;
    let sig: p384::ecdsa::Signature = sk.sign(&bytes_from_value(message));
    Ok(PerlValue::string(hex::encode(sig.to_der().as_bytes())))
}

/// ECDSA P-384 verify. Returns 1 if valid.
pub(crate) fn ecdsa_p384_verify(
    pub_hex: &PerlValue,
    message: &PerlValue,
    sig_hex: &PerlValue,
) -> PerlResult<PerlValue> {
    use p384::ecdsa::{signature::Verifier, Signature, VerifyingKey};
    use p384::EncodedPoint;
    let pub_bytes = hex::decode(pub_hex.to_string().trim()).map_err(|e| {
        PerlError::runtime(format!("ecdsa_p384_verify: invalid hex pubkey: {}", e), 0)
    })?;
    let point = EncodedPoint::from_bytes(&pub_bytes)
        .map_err(|e| PerlError::runtime(format!("ecdsa_p384_verify: invalid point: {}", e), 0))?;
    let vk = VerifyingKey::from_encoded_point(&point)
        .map_err(|e| PerlError::runtime(format!("ecdsa_p384_verify: invalid pubkey: {}", e), 0))?;
    let sig_bytes = hex::decode(sig_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ecdsa_p384_verify: invalid hex sig: {}", e), 0))?;
    let sig = Signature::from_der(&sig_bytes).map_err(|e| {
        PerlError::runtime(format!("ecdsa_p384_verify: invalid signature: {}", e), 0)
    })?;
    let ok = vk.verify(&bytes_from_value(message), &sig).is_ok();
    Ok(PerlValue::integer(i64::from(ok)))
}

/// Generate ECDSA secp256k1 keypair (Bitcoin/Ethereum). Returns [private_hex, public_hex_compressed].
pub(crate) fn ecdsa_secp256k1_keygen() -> PerlResult<PerlValue> {
    use k256::ecdsa::SigningKey;
    let sk = SigningKey::random(&mut rand::thread_rng());
    let vk = sk.verifying_key();
    let priv_hex = hex::encode(sk.to_bytes());
    let pub_hex = hex::encode(vk.to_encoded_point(true).as_bytes());
    Ok(PerlValue::array(vec![
        PerlValue::string(priv_hex),
        PerlValue::string(pub_hex),
    ]))
}

/// ECDSA secp256k1 sign. Returns signature as hex (DER-encoded).
pub(crate) fn ecdsa_secp256k1_sign(
    priv_hex: &PerlValue,
    message: &PerlValue,
) -> PerlResult<PerlValue> {
    use k256::ecdsa::{signature::Signer, SigningKey};
    let priv_bytes = hex::decode(priv_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ecdsa_secp256k1_sign: invalid hex: {}", e), 0))?;
    let sk = SigningKey::from_bytes(priv_bytes.as_slice().into())
        .map_err(|e| PerlError::runtime(format!("ecdsa_secp256k1_sign: invalid key: {}", e), 0))?;
    let sig: k256::ecdsa::Signature = sk.sign(&bytes_from_value(message));
    Ok(PerlValue::string(hex::encode(sig.to_der().as_bytes())))
}

/// ECDSA secp256k1 verify. Returns 1 if valid.
pub(crate) fn ecdsa_secp256k1_verify(
    pub_hex: &PerlValue,
    message: &PerlValue,
    sig_hex: &PerlValue,
) -> PerlResult<PerlValue> {
    use k256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
    use k256::EncodedPoint;
    let pub_bytes = hex::decode(pub_hex.to_string().trim()).map_err(|e| {
        PerlError::runtime(
            format!("ecdsa_secp256k1_verify: invalid hex pubkey: {}", e),
            0,
        )
    })?;
    let point = EncodedPoint::from_bytes(&pub_bytes).map_err(|e| {
        PerlError::runtime(format!("ecdsa_secp256k1_verify: invalid point: {}", e), 0)
    })?;
    let vk = VerifyingKey::from_encoded_point(&point).map_err(|e| {
        PerlError::runtime(format!("ecdsa_secp256k1_verify: invalid pubkey: {}", e), 0)
    })?;
    let sig_bytes = hex::decode(sig_hex.to_string().trim()).map_err(|e| {
        PerlError::runtime(format!("ecdsa_secp256k1_verify: invalid hex sig: {}", e), 0)
    })?;
    let sig = Signature::from_der(&sig_bytes).map_err(|e| {
        PerlError::runtime(
            format!("ecdsa_secp256k1_verify: invalid signature: {}", e),
            0,
        )
    })?;
    let ok = vk.verify(&bytes_from_value(message), &sig).is_ok();
    Ok(PerlValue::integer(i64::from(ok)))
}

// ── ECDH (P-256, P-384) ──────────────────────────────────────────────────────

/// ECDH P-256 key exchange. my_private_hex, their_public_hex. Returns shared_secret_hex.
pub(crate) fn ecdh_p256(
    my_priv_hex: &PerlValue,
    their_pub_hex: &PerlValue,
) -> PerlResult<PerlValue> {
    use p256::{EncodedPoint, PublicKey};
    let priv_bytes = hex::decode(my_priv_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ecdh_p256: invalid hex private key: {}", e), 0))?;
    let pub_bytes = hex::decode(their_pub_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ecdh_p256: invalid hex public key: {}", e), 0))?;
    let point = EncodedPoint::from_bytes(&pub_bytes)
        .map_err(|e| PerlError::runtime(format!("ecdh_p256: invalid point: {}", e), 0))?;
    let their_pk = PublicKey::from_encoded_point(&point)
        .into_option()
        .ok_or_else(|| PerlError::runtime("ecdh_p256: invalid public key", 0))?;
    let my_sk = p256::SecretKey::from_bytes(priv_bytes.as_slice().into())
        .map_err(|e| PerlError::runtime(format!("ecdh_p256: invalid private key: {}", e), 0))?;
    let shared = p256::ecdh::diffie_hellman(my_sk.to_nonzero_scalar(), their_pk.as_affine());
    Ok(PerlValue::string(hex::encode(shared.raw_secret_bytes())))
}

/// ECDH P-384 key exchange. my_private_hex, their_public_hex. Returns shared_secret_hex.
pub(crate) fn ecdh_p384(
    my_priv_hex: &PerlValue,
    their_pub_hex: &PerlValue,
) -> PerlResult<PerlValue> {
    use p384::{EncodedPoint, PublicKey};
    let priv_bytes = hex::decode(my_priv_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ecdh_p384: invalid hex private key: {}", e), 0))?;
    let pub_bytes = hex::decode(their_pub_hex.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ecdh_p384: invalid hex public key: {}", e), 0))?;
    let point = EncodedPoint::from_bytes(&pub_bytes)
        .map_err(|e| PerlError::runtime(format!("ecdh_p384: invalid point: {}", e), 0))?;
    let their_pk = PublicKey::from_encoded_point(&point)
        .into_option()
        .ok_or_else(|| PerlError::runtime("ecdh_p384: invalid public key", 0))?;
    let my_sk = p384::SecretKey::from_bytes(priv_bytes.as_slice().into())
        .map_err(|e| PerlError::runtime(format!("ecdh_p384: invalid private key: {}", e), 0))?;
    let shared = p384::ecdh::diffie_hellman(my_sk.to_nonzero_scalar(), their_pk.as_affine());
    Ok(PerlValue::string(hex::encode(shared.raw_secret_bytes())))
}

// ── Password Hashing (KDFs) ──────────────────────────────────────────────────

/// Argon2id password hash. Returns PHC string format.
pub(crate) fn argon2_hash(password: &PerlValue) -> PerlResult<PerlValue> {
    use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
    let salt = SaltString::generate(&mut rand::thread_rng());
    let argon = Argon2::default();
    let hash = argon
        .hash_password(password.to_string().as_bytes(), &salt)
        .map_err(|e| PerlError::runtime(format!("argon2_hash: {}", e), 0))?;
    Ok(PerlValue::string(hash.to_string()))
}

/// Verify password against Argon2 PHC hash string.
pub(crate) fn argon2_verify(password: &PerlValue, hash: &PerlValue) -> PerlResult<PerlValue> {
    use argon2::{password_hash::PasswordHash, password_hash::PasswordVerifier, Argon2};
    let hash_str = hash.to_string();
    let parsed = PasswordHash::new(&hash_str)
        .map_err(|e| PerlError::runtime(format!("argon2_verify: invalid hash: {}", e), 0))?;
    let ok = Argon2::default()
        .verify_password(password.to_string().as_bytes(), &parsed)
        .is_ok();
    Ok(PerlValue::integer(i64::from(ok)))
}

/// Bcrypt password hash. Returns standard bcrypt string ($2b$...).
pub(crate) fn bcrypt_hash(password: &PerlValue) -> PerlResult<PerlValue> {
    let hash = bcrypt::hash(password.to_string(), bcrypt::DEFAULT_COST)
        .map_err(|e| PerlError::runtime(format!("bcrypt_hash: {}", e), 0))?;
    Ok(PerlValue::string(hash))
}

/// Verify password against bcrypt hash.
pub(crate) fn bcrypt_verify(password: &PerlValue, hash: &PerlValue) -> PerlResult<PerlValue> {
    let ok = bcrypt::verify(password.to_string(), &hash.to_string())
        .map_err(|e| PerlError::runtime(format!("bcrypt_verify: {}", e), 0))?;
    Ok(PerlValue::integer(i64::from(ok)))
}

/// Scrypt password hash. Returns PHC string format.
pub(crate) fn scrypt_hash(password: &PerlValue) -> PerlResult<PerlValue> {
    use scrypt::{
        password_hash::{PasswordHasher, SaltString},
        Scrypt,
    };
    let salt = SaltString::generate(&mut rand::thread_rng());
    let hash = Scrypt
        .hash_password(password.to_string().as_bytes(), &salt)
        .map_err(|e| PerlError::runtime(format!("scrypt_hash: {}", e), 0))?;
    Ok(PerlValue::string(hash.to_string()))
}

/// Verify password against scrypt PHC hash.
pub(crate) fn scrypt_verify(password: &PerlValue, hash: &PerlValue) -> PerlResult<PerlValue> {
    use scrypt::{
        password_hash::{PasswordHash, PasswordVerifier},
        Scrypt,
    };
    let hash_str = hash.to_string();
    let parsed = PasswordHash::new(&hash_str)
        .map_err(|e| PerlError::runtime(format!("scrypt_verify: invalid hash: {}", e), 0))?;
    let ok = Scrypt
        .verify_password(password.to_string().as_bytes(), &parsed)
        .is_ok();
    Ok(PerlValue::integer(i64::from(ok)))
}

/// PBKDF2-HMAC-SHA256 key derivation. Returns hex of derived key (32 bytes).
/// Args: password, salt, iterations (default 100000).
pub(crate) fn pbkdf2_derive(
    password: &PerlValue,
    salt: &PerlValue,
    iterations: &PerlValue,
) -> PerlResult<PerlValue> {
    let iters = if !iterations.is_undef() && iterations.to_int() > 0 {
        iterations.to_int() as u32
    } else {
        100_000
    };
    let key: [u8; 32] = pbkdf2_hmac_array::<Sha256, 32>(
        password.to_string().as_bytes(),
        &bytes_from_value(salt),
        iters,
    );
    Ok(PerlValue::string(hex::encode(key)))
}

// ── Symmetric Encryption ─────────────────────────────────────────────────────

/// Generate cryptographically secure random bytes; returns as bytes PerlValue.
pub(crate) fn random_bytes(n: &PerlValue) -> PerlResult<PerlValue> {
    let len = n.to_int().max(0) as usize;
    let mut buf = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut buf);
    Ok(PerlValue::bytes(Arc::new(buf)))
}

/// Generate cryptographically secure random bytes as hex string.
pub(crate) fn random_bytes_hex(n: &PerlValue) -> PerlResult<PerlValue> {
    let len = n.to_int().max(0) as usize;
    let mut buf = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut buf);
    Ok(PerlValue::string(hex::encode(buf)))
}

/// AES-256-GCM encrypt. key=32 bytes, nonce=12 bytes (auto-generated if not provided).
/// Returns base64(nonce || ciphertext || tag).
pub(crate) fn aes_encrypt(key: &PerlValue, plaintext: &PerlValue) -> PerlResult<PerlValue> {
    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!("aes_encrypt: key must be 32 bytes, got {}", key_bytes.len()),
            0,
        ));
    }
    let cipher = <Aes256Gcm as AesKeyInit>::new_from_slice(&key_bytes)
        .map_err(|e| PerlError::runtime(format!("aes_encrypt: {}", e), 0))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = AesNonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, bytes_from_value(plaintext).as_ref())
        .map_err(|e| PerlError::runtime(format!("aes_encrypt: {}", e), 0))?;
    let mut out = nonce_bytes.to_vec();
    out.extend(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&out),
    ))
}

/// AES-256-GCM decrypt. key=32 bytes. Input is base64(nonce || ciphertext || tag).
pub(crate) fn aes_decrypt(key: &PerlValue, ciphertext_b64: &PerlValue) -> PerlResult<PerlValue> {
    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!("aes_decrypt: key must be 32 bytes, got {}", key_bytes.len()),
            0,
        ));
    }
    let cipher = <Aes256Gcm as AesKeyInit>::new_from_slice(&key_bytes)
        .map_err(|e| PerlError::runtime(format!("aes_decrypt: {}", e), 0))?;
    let data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("aes_decrypt: invalid base64: {}", e), 0))?;
    if data.len() < 12 {
        return Err(PerlError::runtime("aes_decrypt: ciphertext too short", 0));
    }
    let nonce = AesNonce::from_slice(&data[..12]);
    let plaintext = cipher.decrypt(nonce, &data[12..]).map_err(|_| {
        PerlError::runtime("aes_decrypt: decryption failed (bad key or tampered)", 0)
    })?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&plaintext).into_owned(),
    ))
}

/// ChaCha20-Poly1305 encrypt. key=32 bytes, nonce=12 bytes (auto-generated).
/// Returns base64(nonce || ciphertext || tag).
pub(crate) fn chacha_encrypt(key: &PerlValue, plaintext: &PerlValue) -> PerlResult<PerlValue> {
    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!(
                "chacha_encrypt: key must be 32 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let cipher = <ChaCha20Poly1305 as ChachaKeyInit>::new_from_slice(&key_bytes)
        .map_err(|e| PerlError::runtime(format!("chacha_encrypt: {}", e), 0))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = ChachaNonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, bytes_from_value(plaintext).as_ref())
        .map_err(|e| PerlError::runtime(format!("chacha_encrypt: {}", e), 0))?;
    let mut out = nonce_bytes.to_vec();
    out.extend(ciphertext);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&out),
    ))
}

/// ChaCha20-Poly1305 decrypt. key=32 bytes. Input is base64(nonce || ciphertext || tag).
pub(crate) fn chacha_decrypt(key: &PerlValue, ciphertext_b64: &PerlValue) -> PerlResult<PerlValue> {
    let key_bytes = bytes_from_value(key);
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!(
                "chacha_decrypt: key must be 32 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let cipher = <ChaCha20Poly1305 as ChachaKeyInit>::new_from_slice(&key_bytes)
        .map_err(|e| PerlError::runtime(format!("chacha_decrypt: {}", e), 0))?;
    let data = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("chacha_decrypt: invalid base64: {}", e), 0))?;
    if data.len() < 12 {
        return Err(PerlError::runtime(
            "chacha_decrypt: ciphertext too short",
            0,
        ));
    }
    let nonce = ChachaNonce::from_slice(&data[..12]);
    let plaintext = cipher.decrypt(nonce, &data[12..]).map_err(|_| {
        PerlError::runtime("chacha_decrypt: decryption failed (bad key or tampered)", 0)
    })?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&plaintext).into_owned(),
    ))
}

// ── Asymmetric Crypto (Ed25519, X25519) ──────────────────────────────────────

/// Generate Ed25519 keypair. Returns [private_key_hex, public_key_hex].
pub(crate) fn ed25519_keygen() -> PerlResult<PerlValue> {
    let secret = SigningKey::generate(&mut rand::thread_rng());
    let public = secret.verifying_key();
    Ok(PerlValue::array(vec![
        PerlValue::string(hex::encode(secret.to_bytes())),
        PerlValue::string(hex::encode(public.to_bytes())),
    ]))
}

/// Ed25519 sign. private_key_hex (64 chars / 32 bytes), message.
/// Returns signature as hex (128 chars / 64 bytes).
pub(crate) fn ed25519_sign(private_key: &PerlValue, message: &PerlValue) -> PerlResult<PerlValue> {
    let key_bytes = hex::decode(private_key.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ed25519_sign: invalid hex key: {}", e), 0))?;
    if key_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!(
                "ed25519_sign: key must be 32 bytes, got {}",
                key_bytes.len()
            ),
            0,
        ));
    }
    let secret: [u8; 32] = key_bytes.try_into().unwrap();
    let signing_key = SigningKey::from_bytes(&secret);
    let sig = signing_key.sign(&bytes_from_value(message));
    Ok(PerlValue::string(hex::encode(sig.to_bytes())))
}

/// Ed25519 verify. public_key_hex (64 chars), message, signature_hex (128 chars).
/// Returns 1 if valid, 0 if invalid.
pub(crate) fn ed25519_verify(
    public_key: &PerlValue,
    message: &PerlValue,
    signature: &PerlValue,
) -> PerlResult<PerlValue> {
    let pub_bytes = hex::decode(public_key.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ed25519_verify: invalid hex pubkey: {}", e), 0))?;
    let sig_bytes = hex::decode(signature.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("ed25519_verify: invalid hex sig: {}", e), 0))?;
    if pub_bytes.len() != 32 {
        return Err(PerlError::runtime(
            format!(
                "ed25519_verify: pubkey must be 32 bytes, got {}",
                pub_bytes.len()
            ),
            0,
        ));
    }
    if sig_bytes.len() != 64 {
        return Err(PerlError::runtime(
            format!(
                "ed25519_verify: signature must be 64 bytes, got {}",
                sig_bytes.len()
            ),
            0,
        ));
    }
    let pub_arr: [u8; 32] = pub_bytes.try_into().unwrap();
    let sig_arr: [u8; 64] = sig_bytes.try_into().unwrap();
    let verifying_key = VerifyingKey::from_bytes(&pub_arr)
        .map_err(|e| PerlError::runtime(format!("ed25519_verify: invalid pubkey: {}", e), 0))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
    let ok = verifying_key
        .verify(&bytes_from_value(message), &sig)
        .is_ok();
    Ok(PerlValue::integer(i64::from(ok)))
}

/// Generate X25519 keypair. Returns [private_key_hex, public_key_hex].
pub(crate) fn x25519_keygen() -> PerlResult<PerlValue> {
    let secret = X25519StaticSecret::random_from_rng(rand::thread_rng());
    let public = X25519PublicKey::from(&secret);
    Ok(PerlValue::array(vec![
        PerlValue::string(hex::encode(secret.to_bytes())),
        PerlValue::string(hex::encode(public.to_bytes())),
    ]))
}

/// X25519 Diffie-Hellman. my_private_hex, their_public_hex → shared_secret_hex.
pub(crate) fn x25519_dh(my_private: &PerlValue, their_public: &PerlValue) -> PerlResult<PerlValue> {
    let priv_bytes = hex::decode(my_private.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("x25519_dh: invalid hex private key: {}", e), 0))?;
    let pub_bytes = hex::decode(their_public.to_string().trim())
        .map_err(|e| PerlError::runtime(format!("x25519_dh: invalid hex public key: {}", e), 0))?;
    if priv_bytes.len() != 32 || pub_bytes.len() != 32 {
        return Err(PerlError::runtime("x25519_dh: keys must be 32 bytes", 0));
    }
    let priv_arr: [u8; 32] = priv_bytes.try_into().unwrap();
    let pub_arr: [u8; 32] = pub_bytes.try_into().unwrap();
    let secret = X25519StaticSecret::from(priv_arr);
    let public = X25519PublicKey::from(pub_arr);
    let shared = secret.diffie_hellman(&public);
    Ok(PerlValue::string(hex::encode(shared.as_bytes())))
}

// ── Special Math Functions ───────────────────────────────────────────────────

/// Error function erf(x).
pub(crate) fn math_erf(v: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::erf::erf;
    Ok(PerlValue::float(erf(v.to_number())))
}

/// Complementary error function erfc(x) = 1 - erf(x).
pub(crate) fn math_erfc(v: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::erf::erfc;
    Ok(PerlValue::float(erfc(v.to_number())))
}

/// Gamma function Γ(x).
pub(crate) fn math_gamma(v: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::gamma::gamma;
    Ok(PerlValue::float(gamma(v.to_number())))
}

/// Natural log of gamma function ln(Γ(x)).
pub(crate) fn math_lgamma(v: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::gamma::ln_gamma;
    Ok(PerlValue::float(ln_gamma(v.to_number())))
}

/// Digamma function ψ(x) = d/dx ln(Γ(x)).
pub(crate) fn math_digamma(v: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::gamma::digamma;
    Ok(PerlValue::float(digamma(v.to_number())))
}

/// Beta function B(a, b) = Γ(a)Γ(b)/Γ(a+b).
pub(crate) fn math_beta(a: &PerlValue, b: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::beta::beta;
    Ok(PerlValue::float(beta(a.to_number(), b.to_number())))
}

/// Natural log of beta function ln(B(a, b)).
pub(crate) fn math_lbeta(a: &PerlValue, b: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::beta::ln_beta;
    Ok(PerlValue::float(ln_beta(a.to_number(), b.to_number())))
}

/// Regularized incomplete beta function I_x(a, b).
pub(crate) fn math_betainc(x: &PerlValue, a: &PerlValue, b: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::beta::beta_reg;
    Ok(PerlValue::float(beta_reg(
        a.to_number(),
        b.to_number(),
        x.to_number(),
    )))
}

/// Lower incomplete gamma function γ(a, x).
pub(crate) fn math_gammainc(a: &PerlValue, x: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::gamma::gamma_li;
    Ok(PerlValue::float(gamma_li(a.to_number(), x.to_number())))
}

/// Upper incomplete gamma function Γ(a, x).
pub(crate) fn math_gammaincc(a: &PerlValue, x: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::gamma::gamma_ui;
    Ok(PerlValue::float(gamma_ui(a.to_number(), x.to_number())))
}

/// Regularized lower incomplete gamma P(a, x) = γ(a,x)/Γ(a).
pub(crate) fn math_gammainc_reg(a: &PerlValue, x: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::gamma::gamma_lr;
    Ok(PerlValue::float(gamma_lr(a.to_number(), x.to_number())))
}

/// Regularized upper incomplete gamma Q(a, x) = Γ(a,x)/Γ(a).
pub(crate) fn math_gammaincc_reg(a: &PerlValue, x: &PerlValue) -> PerlResult<PerlValue> {
    use statrs::function::gamma::gamma_ur;
    Ok(PerlValue::float(gamma_ur(a.to_number(), x.to_number())))
}

/// Raw HMAC-SHA256 bytes (for JWT and other binary signatures).
pub(crate) fn hmac_sha256_raw(key: &PerlValue, msg: &PerlValue) -> PerlResult<Vec<u8>> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha256: {}", e), 0))?;
    Mac::update(&mut mac, &bytes_from_value(msg));
    Ok(mac.finalize().into_bytes().to_vec())
}

/// Constant-time HMAC verification (rejects forged or truncated tags).
pub(crate) fn hmac_sha256_verify_raw(
    key: &PerlValue,
    msg: &PerlValue,
    tag: &[u8],
) -> PerlResult<()> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha256: {}", e), 0))?;
    Mac::update(&mut mac, &bytes_from_value(msg));
    mac.verify_slice(tag)
        .map_err(|_| PerlError::runtime("HMAC verification failed", 0))
}

/// JWT / RFC 4648 URL-safe base64 **without** padding.
pub(crate) fn base64url_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// Decode URL-safe base64 (adds `=` padding as needed).
pub(crate) fn base64url_decode(s: &str) -> Result<Vec<u8>, PerlError> {
    let s = s.trim().replace(' ', "");
    let pad = (4 - (s.len() % 4)) % 4;
    let mut padded = s;
    padded.push_str(&"=".repeat(pad));
    base64::engine::general_purpose::URL_SAFE
        .decode(padded.as_bytes())
        .map_err(|e| PerlError::runtime(format!("base64url_decode: {}", e), 0))
}

/// Random UUID (v4) as hyphenated lowercase string.
pub(crate) fn uuid_v4() -> PerlResult<PerlValue> {
    Ok(PerlValue::string(
        uuid::Uuid::new_v4().hyphenated().to_string(),
    ))
}

pub(crate) fn base64_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&b),
    ))
}

pub(crate) fn base64_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| PerlError::runtime(format!("base64_decode: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&raw).into_owned(),
    ))
}

/// Bytes to lowercase hex (two hex digits per byte).
pub(crate) fn hex_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(hex::encode(bytes_from_value(v))))
}

/// Hex string (even length) to a Perl string (may be non-UTF-8; uses lossy UTF-8 like other byte paths).
pub(crate) fn hex_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    let raw =
        hex::decode(s.trim()).map_err(|e| PerlError::runtime(format!("hex_decode: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&raw).into_owned(),
    ))
}

pub(crate) fn gzip(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&b)
        .map_err(|e| PerlError::runtime(format!("gzip: {}", e), 0))?;
    let out = enc
        .finish()
        .map_err(|e| PerlError::runtime(format!("gzip: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(out)))
}

pub(crate) fn gunzip(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    let mut dec = GzDecoder::new(&b[..]);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| PerlError::runtime(format!("gunzip: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(out)))
}

pub(crate) fn zstd_compress(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    let out =
        zstd::encode_all(&b[..], 3).map_err(|e| PerlError::runtime(format!("zstd: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(out)))
}

pub(crate) fn zstd_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    let out = zstd::decode_all(&b[..])
        .map_err(|e| PerlError::runtime(format!("zstd_decode: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(out)))
}

// ── Brotli Compression ───────────────────────────────────────────────────────

/// Brotli compress. Returns compressed bytes.
pub(crate) fn brotli_compress(v: &PerlValue) -> PerlResult<PerlValue> {
    let input = bytes_from_value(v);
    let mut output = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut output, 4096, 6, 22);
        writer
            .write_all(&input)
            .map_err(|e| PerlError::runtime(format!("brotli: {}", e), 0))?;
    }
    Ok(PerlValue::bytes(Arc::new(output)))
}

/// Brotli decompress. Returns decompressed bytes.
pub(crate) fn brotli_decompress(v: &PerlValue) -> PerlResult<PerlValue> {
    let input = bytes_from_value(v);
    let mut output = Vec::new();
    {
        let mut reader = brotli::Decompressor::new(&input[..], 4096);
        reader
            .read_to_end(&mut output)
            .map_err(|e| PerlError::runtime(format!("brotli_decode: {}", e), 0))?;
    }
    Ok(PerlValue::bytes(Arc::new(output)))
}

// ── XZ / LZMA Compression ────────────────────────────────────────────────────

/// XZ/LZMA compress. Returns compressed bytes.
pub(crate) fn xz_compress(v: &PerlValue) -> PerlResult<PerlValue> {
    use std::io::Write;
    let input = bytes_from_value(v);
    let mut output = Vec::new();
    {
        let mut encoder = xz2::write::XzEncoder::new(&mut output, 6);
        encoder
            .write_all(&input)
            .map_err(|e| PerlError::runtime(format!("xz: {}", e), 0))?;
        encoder
            .finish()
            .map_err(|e| PerlError::runtime(format!("xz: {}", e), 0))?;
    }
    Ok(PerlValue::bytes(Arc::new(output)))
}

/// XZ/LZMA decompress. Returns decompressed bytes.
pub(crate) fn xz_decompress(v: &PerlValue) -> PerlResult<PerlValue> {
    let input = bytes_from_value(v);
    let mut output = Vec::new();
    {
        let mut decoder = xz2::read::XzDecoder::new(&input[..]);
        decoder
            .read_to_end(&mut output)
            .map_err(|e| PerlError::runtime(format!("xz_decode: {}", e), 0))?;
    }
    Ok(PerlValue::bytes(Arc::new(output)))
}

// ── Bzip2 Compression ────────────────────────────────────────────────────────

/// Bzip2 compress. Returns compressed bytes.
pub(crate) fn bzip2_compress(v: &PerlValue) -> PerlResult<PerlValue> {
    use bzip2::write::BzEncoder;
    use bzip2::Compression;
    use std::io::Write;
    let input = bytes_from_value(v);
    let mut output = Vec::new();
    {
        let mut encoder = BzEncoder::new(&mut output, Compression::default());
        encoder
            .write_all(&input)
            .map_err(|e| PerlError::runtime(format!("bzip2: {}", e), 0))?;
        encoder
            .finish()
            .map_err(|e| PerlError::runtime(format!("bzip2: {}", e), 0))?;
    }
    Ok(PerlValue::bytes(Arc::new(output)))
}

/// Bzip2 decompress. Returns decompressed bytes.
pub(crate) fn bzip2_decompress(v: &PerlValue) -> PerlResult<PerlValue> {
    use bzip2::read::BzDecoder;
    let input = bytes_from_value(v);
    let mut output = Vec::new();
    {
        let mut decoder = BzDecoder::new(&input[..]);
        decoder
            .read_to_end(&mut output)
            .map_err(|e| PerlError::runtime(format!("bzip2_decode: {}", e), 0))?;
    }
    Ok(PerlValue::bytes(Arc::new(output)))
}

// ── LZ4 Compression ──────────────────────────────────────────────────────────

/// LZ4 compress (fast). Returns compressed bytes.
pub(crate) fn lz4_compress(v: &PerlValue) -> PerlResult<PerlValue> {
    let input = bytes_from_value(v);
    let output = lz4_flex::compress_prepend_size(&input);
    Ok(PerlValue::bytes(Arc::new(output)))
}

/// LZ4 decompress. Returns decompressed bytes.
pub(crate) fn lz4_decompress(v: &PerlValue) -> PerlResult<PerlValue> {
    let input = bytes_from_value(v);
    let output = lz4_flex::decompress_size_prepended(&input)
        .map_err(|e| PerlError::runtime(format!("lz4_decode: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(output)))
}

// ── Snappy Compression ───────────────────────────────────────────────────────

/// Snappy compress (very fast, moderate ratio). Returns compressed bytes.
pub(crate) fn snappy_compress(v: &PerlValue) -> PerlResult<PerlValue> {
    let input = bytes_from_value(v);
    let mut encoder = snap::raw::Encoder::new();
    let output = encoder
        .compress_vec(&input)
        .map_err(|e| PerlError::runtime(format!("snappy: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(output)))
}

/// Snappy decompress. Returns decompressed bytes.
pub(crate) fn snappy_decompress(v: &PerlValue) -> PerlResult<PerlValue> {
    let input = bytes_from_value(v);
    let mut decoder = snap::raw::Decoder::new();
    let output = decoder
        .decompress_vec(&input)
        .map_err(|e| PerlError::runtime(format!("snappy_decode: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(output)))
}

// ── Tar Archive ──────────────────────────────────────────────────────────────

/// Create tar archive from a directory. Returns tar bytes.
pub(crate) fn tar_create(dir: &PerlValue) -> PerlResult<PerlValue> {
    let dir_path = dir.to_string();
    let mut archive = tar::Builder::new(Vec::new());
    archive
        .append_dir_all(".", &dir_path)
        .map_err(|e| PerlError::runtime(format!("tar_create: {}", e), 0))?;
    let output = archive
        .into_inner()
        .map_err(|e| PerlError::runtime(format!("tar_create: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(output)))
}

/// Extract tar archive to a directory.
pub(crate) fn tar_extract(tar_data: &PerlValue, dest_dir: &PerlValue) -> PerlResult<PerlValue> {
    let data = bytes_from_value(tar_data);
    let dest = dest_dir.to_string();
    let mut archive = tar::Archive::new(&data[..]);
    archive
        .unpack(&dest)
        .map_err(|e| PerlError::runtime(format!("tar_extract: {}", e), 0))?;
    Ok(PerlValue::integer(1))
}

/// List files in tar archive. Returns array of paths.
pub(crate) fn tar_list(tar_data: &PerlValue) -> PerlResult<PerlValue> {
    let data = bytes_from_value(tar_data);
    let mut archive = tar::Archive::new(&data[..]);
    let mut files = Vec::new();
    for entry in archive
        .entries()
        .map_err(|e| PerlError::runtime(format!("tar_list: {}", e), 0))?
    {
        let entry = entry.map_err(|e| PerlError::runtime(format!("tar_list: {}", e), 0))?;
        let path = entry
            .path()
            .map_err(|e| PerlError::runtime(format!("tar_list: {}", e), 0))?;
        files.push(PerlValue::string(path.to_string_lossy().into_owned()));
    }
    Ok(PerlValue::array(files))
}

/// Create tar.gz (gzipped tar). Convenience for tar_create + gzip.
pub(crate) fn tar_gz_create(dir: &PerlValue) -> PerlResult<PerlValue> {
    let tar_bytes = tar_create(dir)?;
    gzip(&tar_bytes)
}

/// Extract tar.gz (gzipped tar) to directory.
pub(crate) fn tar_gz_extract(
    tar_gz_data: &PerlValue,
    dest_dir: &PerlValue,
) -> PerlResult<PerlValue> {
    let decompressed = gunzip(tar_gz_data)?;
    tar_extract(&decompressed, dest_dir)
}

// ── LZW Compression ──────────────────────────────────────────────────────────

/// LZW compress (GIF/TIFF style). Returns compressed bytes.
pub(crate) fn lzw_compress(v: &PerlValue) -> PerlResult<PerlValue> {
    use weezl::{encode::Encoder, BitOrder};
    let input = bytes_from_value(v);
    let mut encoder = Encoder::new(BitOrder::Msb, 8);
    let output = encoder
        .encode(&input)
        .map_err(|e| PerlError::runtime(format!("lzw: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(output)))
}

/// LZW decompress. Returns decompressed bytes.
pub(crate) fn lzw_decompress(v: &PerlValue) -> PerlResult<PerlValue> {
    use weezl::{decode::Decoder, BitOrder};
    let input = bytes_from_value(v);
    let mut decoder = Decoder::new(BitOrder::Msb, 8);
    let output = decoder
        .decode(&input)
        .map_err(|e| PerlError::runtime(format!("lzw_decode: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(output)))
}

// ── ZIP Archive ──────────────────────────────────────────────────────────────

/// Create ZIP archive from a directory. Returns zip bytes.
pub(crate) fn zip_create(dir: &PerlValue) -> PerlResult<PerlValue> {
    use std::path::Path;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let dir_path = dir.to_string();
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buffer);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        fn add_dir_recursive(
            zip: &mut ZipWriter<&mut std::io::Cursor<Vec<u8>>>,
            base: &Path,
            current: &Path,
            options: SimpleFileOptions,
        ) -> Result<(), PerlError> {
            for entry in std::fs::read_dir(current)
                .map_err(|e| PerlError::runtime(format!("zip_create: {}", e), 0))?
            {
                let entry =
                    entry.map_err(|e| PerlError::runtime(format!("zip_create: {}", e), 0))?;
                let path = entry.path();
                let name = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();

                if path.is_dir() {
                    zip.add_directory(&name, options)
                        .map_err(|e| PerlError::runtime(format!("zip_create: {}", e), 0))?;
                    add_dir_recursive(zip, base, &path, options)?;
                } else {
                    zip.start_file(&name, options)
                        .map_err(|e| PerlError::runtime(format!("zip_create: {}", e), 0))?;
                    let content = std::fs::read(&path)
                        .map_err(|e| PerlError::runtime(format!("zip_create: {}", e), 0))?;
                    std::io::Write::write_all(zip, &content)
                        .map_err(|e| PerlError::runtime(format!("zip_create: {}", e), 0))?;
                }
            }
            Ok(())
        }

        let base = Path::new(&dir_path);
        add_dir_recursive(&mut zip, base, base, options)?;
        zip.finish()
            .map_err(|e| PerlError::runtime(format!("zip_create: {}", e), 0))?;
    }
    Ok(PerlValue::bytes(Arc::new(buffer.into_inner())))
}

/// Extract ZIP archive to a directory.
pub(crate) fn zip_extract(zip_data: &PerlValue, dest_dir: &PerlValue) -> PerlResult<PerlValue> {
    use std::fs;
    use std::io::Write;
    use std::path::Path;

    let data = bytes_from_value(zip_data);
    let dest = dest_dir.to_string();
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| PerlError::runtime(format!("zip_extract: {}", e), 0))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| PerlError::runtime(format!("zip_extract: {}", e), 0))?;
        let outpath = Path::new(&dest).join(file.name());

        if file.is_dir() {
            fs::create_dir_all(&outpath)
                .map_err(|e| PerlError::runtime(format!("zip_extract: {}", e), 0))?;
        } else {
            if let Some(p) = outpath.parent() {
                fs::create_dir_all(p)
                    .map_err(|e| PerlError::runtime(format!("zip_extract: {}", e), 0))?;
            }
            let mut outfile = fs::File::create(&outpath)
                .map_err(|e| PerlError::runtime(format!("zip_extract: {}", e), 0))?;
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut content)
                .map_err(|e| PerlError::runtime(format!("zip_extract: {}", e), 0))?;
            outfile
                .write_all(&content)
                .map_err(|e| PerlError::runtime(format!("zip_extract: {}", e), 0))?;
        }
    }
    Ok(PerlValue::integer(1))
}

/// List files in ZIP archive. Returns array of paths.
pub(crate) fn zip_list(zip_data: &PerlValue) -> PerlResult<PerlValue> {
    let data = bytes_from_value(zip_data);
    let cursor = std::io::Cursor::new(data);
    let archive = zip::ZipArchive::new(cursor)
        .map_err(|e| PerlError::runtime(format!("zip_list: {}", e), 0))?;

    let mut files = Vec::new();
    for i in 0..archive.len() {
        files.push(PerlValue::string(
            archive.name_for_index(i).unwrap_or("").to_string(),
        ));
    }
    Ok(PerlValue::array(files))
}

// ── DateTime (UTC / epoch; uses chrono, no heap object type) ──

/// Current time as RFC 3339 UTC (e.g. `2024-01-02T15:04:05.123456789Z`).
pub(crate) fn datetime_utc() -> PerlResult<PerlValue> {
    let t = Utc::now();
    Ok(PerlValue::string(
        t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
    ))
}

/// Unix epoch seconds (float) → RFC 3339 UTC string. Fractional seconds preserved when representable.
pub(crate) fn datetime_from_epoch(v: &PerlValue) -> PerlResult<PerlValue> {
    let sec = v.to_number();
    if !sec.is_finite() {
        return Err(PerlError::runtime(
            "datetime_from_epoch: non-finite value",
            0,
        ));
    }
    let t = Utc
        .timestamp_opt(sec.floor() as i64, fraction_nanos(sec))
        .single()
        .ok_or_else(|| PerlError::runtime("datetime_from_epoch: out of range", 0))?;
    Ok(PerlValue::string(
        t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
    ))
}

fn fraction_nanos(sec: f64) -> u32 {
    let frac = sec - sec.floor();
    let n = (frac * 1_000_000_000.0).round() as i64;
    n.clamp(0, 999_999_999) as u32
}

/// Parse RFC 3339 / ISO-8601 datetime → Unix seconds as float (UTC).
pub(crate) fn datetime_parse_rfc3339(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    let t = DateTime::parse_from_rfc3339(s.trim())
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| s.trim().parse::<DateTime<Utc>>())
        .map_err(|e| PerlError::runtime(format!("datetime_parse_rfc3339: {}", e), 0))?;
    let secs = t.timestamp() as f64 + f64::from(t.timestamp_subsec_nanos()) / 1e9;
    Ok(PerlValue::float(secs))
}

/// `strftime` formatting for UTC epoch seconds. `fmt` uses chrono's `strftime` specifiers.
pub(crate) fn datetime_strftime(epoch: &PerlValue, fmt: &PerlValue) -> PerlResult<PerlValue> {
    let sec = epoch.to_number();
    if !sec.is_finite() {
        return Err(PerlError::runtime("datetime_strftime: non-finite epoch", 0));
    }
    let pattern = fmt.to_string();
    let t = Utc
        .timestamp_opt(sec.floor() as i64, fraction_nanos(sec))
        .single()
        .ok_or_else(|| PerlError::runtime("datetime_strftime: out of range", 0))?;
    let out = t.format(&pattern).to_string();
    Ok(PerlValue::string(out))
}

/// Current time in an IANA timezone (e.g. `America/New_York`) as RFC 3339 with offset.
pub(crate) fn datetime_now_tz(tz_name: &PerlValue) -> PerlResult<PerlValue> {
    let tz: Tz = parse_tz(tz_name)?;
    let t = Utc::now().with_timezone(&tz);
    Ok(PerlValue::string(
        t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
    ))
}

fn parse_tz(tz_name: &PerlValue) -> Result<Tz, PerlError> {
    tz_name
        .to_string()
        .trim()
        .parse()
        .map_err(|_| PerlError::runtime(format!("unknown timezone {:?}", tz_name.to_string()), 0))
}

/// Unix epoch seconds (UTC float) formatted with [`chrono::format::strftime`] in an IANA timezone.
pub(crate) fn datetime_format_tz(
    epoch: &PerlValue,
    tz_name: &PerlValue,
    fmt: &PerlValue,
) -> PerlResult<PerlValue> {
    let sec = epoch.to_number();
    if !sec.is_finite() {
        return Err(PerlError::runtime(
            "datetime_format_tz: non-finite epoch",
            0,
        ));
    }
    let tz: Tz = parse_tz(tz_name)?;
    let pattern = fmt.to_string();
    let t = Utc
        .timestamp_opt(sec.floor() as i64, fraction_nanos(sec))
        .single()
        .ok_or_else(|| PerlError::runtime("datetime_format_tz: out of range", 0))?;
    let local = t.with_timezone(&tz);
    Ok(PerlValue::string(local.format(&pattern).to_string()))
}

/// Wall-clock / naive datetime string interpreted in an IANA timezone → UTC epoch seconds (float).
/// Accepts `%Y-%m-%d %H:%M:%S`, `%Y-%m-%dT%H:%M:%S`, or `%Y-%m-%d` (midnight).
pub(crate) fn datetime_parse_local(s: &PerlValue, tz_name: &PerlValue) -> PerlResult<PerlValue> {
    let tz: Tz = parse_tz(tz_name)?;
    let text = s.to_string();
    let naive = parse_naive_datetime(text.trim()).ok_or_else(|| {
        PerlError::runtime(
            "datetime_parse_local: expected YYYY-MM-DD [HH:MM:SS] or YYYY-MM-DDTHH:MM:SS",
            0,
        )
    })?;
    let mapped = tz
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| PerlError::runtime("datetime_parse_local: invalid local time", 0))?;
    let utc = mapped.with_timezone(&Utc);
    let secs = utc.timestamp() as f64 + f64::from(utc.timestamp_subsec_nanos()) / 1e9;
    Ok(PerlValue::float(secs))
}

fn parse_naive_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok())
        .or_else(|| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
        })
}

/// Epoch arithmetic: `datetime_add_seconds($epoch, $delta)` — both floats; result is float UTC seconds.
pub(crate) fn datetime_add_seconds(epoch: &PerlValue, secs: &PerlValue) -> PerlResult<PerlValue> {
    let a = epoch.to_number();
    let b = secs.to_number();
    if !a.is_finite() || !b.is_finite() {
        return Err(PerlError::runtime(
            "datetime_add_seconds: non-finite values",
            0,
        ));
    }
    Ok(PerlValue::float(a + b))
}

// ── XML (subset: elements, attributes, text, repeated child tags) ──

/// Decode XML to a single-root hashref: `{ root_tag => { ... } }`.
/// Attributes become keys `@name` (match XML local names). Text content uses `#text`. Repeated child
/// elements become an array.
///
/// When building hashes in Perl for [`xml_encode`], use **single-quoted** keys for attributes (e.g.
/// `'@id'`) so `@` is not interpolated inside double quotes.
pub(crate) fn xml_decode(s: &str) -> PerlResult<PerlValue> {
    let doc = roxmltree::Document::parse(s.trim())
        .map_err(|e| PerlError::runtime(format!("xml_decode: {}", e), 0))?;
    let root = doc.root_element();
    let name = root.tag_name().name().to_string();
    let inner = xml_element_to_perl(root)?;
    let mut m = IndexMap::new();
    m.insert(name, inner);
    Ok(PerlValue::hash_ref(Arc::new(RwLock::new(m))))
}

fn xml_element_to_perl(node: roxmltree::Node<'_, '_>) -> PerlResult<PerlValue> {
    let mut map = IndexMap::new();
    for a in node.attributes() {
        let an = a.name();
        map.insert(format!("@{an}"), PerlValue::string(a.value().to_string()));
    }
    let mut text_buf = String::new();
    for c in node.children() {
        if c.is_text() {
            text_buf.push_str(c.text().unwrap_or(""));
        }
    }
    let t = text_buf.trim();
    if !t.is_empty() {
        map.insert("#text".into(), PerlValue::string(t.to_string()));
    }
    let mut groups: IndexMap<String, Vec<PerlValue>> = IndexMap::new();
    for c in node.children() {
        if c.is_element() {
            let tag = c.tag_name().name().to_string();
            groups.entry(tag).or_default().push(xml_element_to_perl(c)?);
        }
    }
    for (tag, mut vals) in groups {
        let v = if vals.len() == 1 {
            vals.pop().expect("one")
        } else {
            PerlValue::array(vals)
        };
        map.insert(tag, v);
    }
    Ok(PerlValue::hash_ref(Arc::new(RwLock::new(map))))
}

fn is_valid_xml_element_name(name: &str) -> bool {
    let mut ch = name.chars();
    let Some(first) = ch.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == ':') {
        return false;
    }
    ch.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

fn escape_xml_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn escape_xml_attr_value(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

fn xml_write_element(out: &mut String, tag: &str, v: &PerlValue) -> PerlResult<()> {
    if !is_valid_xml_element_name(tag) {
        return Err(PerlError::runtime(
            format!("xml_encode: invalid element name `{tag}`"),
            0,
        ));
    }
    if let Some(s) = v.as_str() {
        out.push('<');
        out.push_str(tag);
        out.push('>');
        escape_xml_text(&s, out);
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        return Ok(());
    }
    if let Some(n) = v.as_integer() {
        out.push('<');
        out.push_str(tag);
        out.push('>');
        out.push_str(itoa::Buffer::new().format(n));
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        return Ok(());
    }
    if v.as_float().is_some() {
        out.push('<');
        out.push_str(tag);
        out.push('>');
        out.push_str(&v.to_string());
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        return Ok(());
    }
    let map = if let Some(m) = v.as_hash_map() {
        m.clone()
    } else if let Some(r) = v.as_hash_ref() {
        r.read().clone()
    } else {
        return Err(PerlError::runtime(
            "xml_encode: element value must be hash(ref), string, or number",
            0,
        ));
    };
    if map.is_empty() {
        out.push('<');
        out.push_str(tag);
        out.push_str("/>");
        return Ok(());
    }
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut text: Option<String> = None;
    let mut children: Vec<(String, PerlValue)> = Vec::new();
    for (k, val) in map {
        if k.starts_with('@') && k.len() > 1 {
            let an = &k[1..];
            if !is_valid_xml_element_name(an) {
                return Err(PerlError::runtime(
                    format!("xml_encode: invalid attribute name `{an}`"),
                    0,
                ));
            }
            attrs.push((an.to_string(), val.to_string()));
        } else if k == "#text" || k == "_" {
            text = Some(val.to_string());
        } else {
            if !is_valid_xml_element_name(&k) {
                return Err(PerlError::runtime(
                    format!("xml_encode: invalid child element name `{k}`"),
                    0,
                ));
            }
            children.push((k, val));
        }
    }
    out.push('<');
    out.push_str(tag);
    for (a, aval) in &attrs {
        out.push(' ');
        out.push_str(a);
        out.push_str("=\"");
        escape_xml_attr_value(aval, out);
        out.push('"');
    }
    let has_body = text.is_some() || !children.is_empty();
    if !has_body {
        out.push_str("/>");
        return Ok(());
    }
    out.push('>');
    if let Some(ref t) = text {
        escape_xml_text(t, out);
    }
    for (cn, cv) in children {
        if let Some(a) = cv.as_array_vec() {
            for item in a {
                xml_write_element(out, &cn, &item)?;
            }
        } else {
            xml_write_element(out, &cn, &cv)?;
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    Ok(())
}

/// Encode `v` as XML: `v` must be a hash(ref) with **exactly one** top-level key (the root element name).
///
/// Attribute keys use a leading `@` in Perl (`'@href' => '...'`); the `@` is stripped in the XML output.
pub(crate) fn xml_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let map = if let Some(m) = v.as_hash_map() {
        m.clone()
    } else if let Some(r) = v.as_hash_ref() {
        r.read().clone()
    } else {
        return Err(PerlError::runtime(
            "xml_encode: need hash or hashref with one root element key",
            0,
        ));
    };
    if map.len() != 1 {
        return Err(PerlError::runtime(
            "xml_encode: top-level hash must have exactly one key (root element name)",
            0,
        ));
    }
    let (root_name, inner) = map.iter().next().expect("one");
    if !is_valid_xml_element_name(root_name) {
        return Err(PerlError::runtime(
            format!("xml_encode: invalid root element name `{root_name}`"),
            0,
        ));
    }
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    xml_write_element(&mut out, root_name, inner)?;
    Ok(PerlValue::string(out))
}

// ── TOML / YAML → PerlValue (same spirit as JSON) ──

pub(crate) fn toml_decode(s: &str) -> PerlResult<PerlValue> {
    let v: toml::Value = toml::from_str(s.trim())
        .map_err(|e| PerlError::runtime(format!("toml_decode: {}", e), 0))?;
    Ok(toml_to_perl(v))
}

/// Serialize a [`PerlValue`] to TOML text (via JSON as intermediate; `null` / unsupported shapes error).
pub(crate) fn toml_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let j = crate::native_data::perl_to_json_value(v)?;
    let s =
        toml::to_string(&j).map_err(|e| PerlError::runtime(format!("toml_encode: {}", e), 0))?;
    Ok(PerlValue::string(s))
}

fn toml_to_perl(v: toml::Value) -> PerlValue {
    match v {
        toml::Value::String(s) => PerlValue::string(s),
        toml::Value::Integer(n) => PerlValue::integer(n),
        toml::Value::Float(x) => PerlValue::float(x),
        toml::Value::Boolean(b) => PerlValue::integer(i64::from(b)),
        toml::Value::Datetime(d) => PerlValue::string(d.to_string()),
        toml::Value::Array(a) => PerlValue::array(a.into_iter().map(toml_to_perl).collect()),
        toml::Value::Table(t) => {
            let mut map = IndexMap::new();
            for (k, v) in t {
                map.insert(k, toml_to_perl(v));
            }
            PerlValue::hash_ref(Arc::new(RwLock::new(map)))
        }
    }
}

pub(crate) fn yaml_decode(s: &str) -> PerlResult<PerlValue> {
    let v: serde_yaml::Value = serde_yaml::from_str(s.trim())
        .map_err(|e| PerlError::runtime(format!("yaml_decode: {}", e), 0))?;
    Ok(yaml_to_perl(v))
}

/// Serialize a [`PerlValue`] to YAML text (via JSON as intermediate).
pub(crate) fn yaml_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let j = crate::native_data::perl_to_json_value(v)?;
    let s = serde_yaml::to_string(&j)
        .map_err(|e| PerlError::runtime(format!("yaml_encode: {}", e), 0))?;
    Ok(PerlValue::string(s))
}

/// Percent-encode for URI components (RFC 3986 unreserved kept; space → `%20`).
pub(crate) fn url_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    Ok(PerlValue::string(
        utf8_percent_encode(s.as_str(), NON_ALPHANUMERIC).to_string(),
    ))
}

/// Decode `%XX` plus unescaped bytes; UTF-8 is lossy-decoded.
pub(crate) fn url_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    Ok(PerlValue::string(
        percent_decode_str(s.trim())
            .decode_utf8_lossy()
            .into_owned(),
    ))
}

fn yaml_to_perl(v: serde_yaml::Value) -> PerlValue {
    match v {
        serde_yaml::Value::Null => PerlValue::UNDEF,
        serde_yaml::Value::Bool(b) => PerlValue::integer(i64::from(b)),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                PerlValue::integer(i)
            } else if let Some(u) = n.as_u64() {
                PerlValue::integer(u as i64)
            } else {
                PerlValue::float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_yaml::Value::String(s) => PerlValue::string(s),
        serde_yaml::Value::Sequence(seq) => {
            PerlValue::array(seq.into_iter().map(yaml_to_perl).collect())
        }
        serde_yaml::Value::Mapping(m) => {
            let mut map = IndexMap::new();
            for (k, v) in m {
                let key = match k {
                    serde_yaml::Value::String(s) => s,
                    _ => yaml_to_perl(k).to_string(),
                };
                map.insert(key, yaml_to_perl(v));
            }
            PerlValue::hash_ref(Arc::new(RwLock::new(map)))
        }
        serde_yaml::Value::Tagged(t) => yaml_to_perl(t.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vector() {
        let p = sha256(&PerlValue::string("abc".into())).expect("sha256");
        assert_eq!(
            p.to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn md5_and_sha1_vectors() {
        assert_eq!(
            md5_digest(&PerlValue::string("".into()))
                .unwrap()
                .to_string(),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            md5_digest(&PerlValue::string("abc".into()))
                .unwrap()
                .to_string(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            sha1_digest(&PerlValue::string("abc".into()))
                .unwrap()
                .to_string(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn url_encode_decode_roundtrip() {
        let s = "a b+c?foo";
        let e = url_encode(&PerlValue::string(s.into())).unwrap();
        assert_eq!(e.to_string(), "a%20b%2Bc%3Ffoo");
        let d = url_decode(&e).unwrap();
        assert_eq!(d.to_string(), s);
    }

    #[test]
    fn toml_yaml_encode_roundtrip_simple() {
        let h = PerlValue::hash_ref(Arc::new(RwLock::new({
            let mut m = IndexMap::new();
            m.insert("x".into(), PerlValue::integer(7));
            m.insert("k".into(), PerlValue::string("v".into()));
            m
        })));
        let t = toml_encode(&h).unwrap().to_string();
        let back = toml_decode(&t).unwrap();
        assert_eq!(
            back.as_hash_ref()
                .unwrap()
                .read()
                .get("x")
                .unwrap()
                .to_int(),
            7
        );
        let y = yaml_encode(&h).unwrap().to_string();
        let yback = yaml_decode(&y).unwrap();
        assert_eq!(
            yback
                .as_hash_ref()
                .unwrap()
                .read()
                .get("k")
                .unwrap()
                .to_string(),
            "v"
        );
    }

    #[test]
    fn gzip_roundtrip() {
        let s = "hello compression";
        let g = gzip(&PerlValue::string(s.into())).expect("gzip");
        let back = gunzip(&g).expect("gunzip");
        assert_eq!(back.to_string(), s);
    }

    #[test]
    fn zstd_roundtrip() {
        let s = "zstd payload";
        let z = zstd_compress(&PerlValue::string(s.into())).expect("zstd");
        let back = zstd_decode(&z).expect("zstd_decode");
        assert_eq!(back.to_string(), s);
    }

    #[test]
    fn yaml_mapping() {
        let p = yaml_decode("a: 1\nb: [x, y]").expect("yaml");
        let h = p.as_hash_ref().expect("hash");
        let g = h.read();
        assert_eq!(g.get("a").unwrap().to_int(), 1);
    }

    #[test]
    fn xml_decode_attrs_and_repeated_children() {
        let x = r#"<root k="1"><item>a</item><item>b</item></root>"#;
        let p = xml_decode(x).expect("xml_decode");
        let h = p.as_hash_ref().expect("root hash");
        let g = h.read();
        let root = g.get("root").expect("root");
        let root_h = root.as_hash_ref().expect("inner");
        let rh = root_h.read();
        assert_eq!(rh.get("@k").unwrap().to_string(), "1");
        let items = rh.get("item").unwrap().as_array_vec().expect("items");
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0]
                .as_hash_ref()
                .unwrap()
                .read()
                .get("#text")
                .unwrap()
                .to_string(),
            "a"
        );
    }

    #[test]
    fn xml_encode_decode_roundtrip() {
        let mut inner = IndexMap::new();
        inner.insert("@id".into(), PerlValue::string("9".into()));
        inner.insert("#text".into(), PerlValue::string("hi".into()));
        let mut root = IndexMap::new();
        root.insert(
            "msg".into(),
            PerlValue::hash_ref(Arc::new(RwLock::new(inner))),
        );
        let v = PerlValue::hash_ref(Arc::new(RwLock::new(root)));
        let s = xml_encode(&v).unwrap().to_string();
        let back = xml_decode(&s).unwrap();
        let back_h = back.as_hash_ref().unwrap();
        let h = back_h.read();
        let msg_h = h.get("msg").unwrap().as_hash_ref().unwrap();
        let msg = msg_h.read();
        assert_eq!(msg.get("@id").unwrap().to_string(), "9");
        assert_eq!(msg.get("#text").unwrap().to_string(), "hi");
    }

    #[test]
    fn toml_table() {
        let p = toml_decode("key = \"v\"\nn = 3").expect("toml");
        let h = p.as_hash_ref().expect("hash");
        let g = h.read();
        assert_eq!(g.get("key").unwrap().to_string(), "v");
        assert_eq!(g.get("n").unwrap().to_int(), 3);
    }

    #[test]
    fn datetime_parse_format_america_new_york() {
        let epoch = datetime_parse_local(
            &PerlValue::string("2024-06-15 12:00:00".into()),
            &PerlValue::string("America/New_York".into()),
        )
        .expect("parse");
        let wall = datetime_format_tz(
            &epoch,
            &PerlValue::string("America/New_York".into()),
            &PerlValue::string("%Y-%m-%d %H:%M:%S".into()),
        )
        .expect("fmt");
        assert_eq!(wall.to_string(), "2024-06-15 12:00:00");
    }

    #[test]
    fn datetime_add_seconds_delta() {
        let out =
            datetime_add_seconds(&PerlValue::float(1_000.0), &PerlValue::float(2.25)).unwrap();
        assert!((out.to_number() - 1002.25).abs() < 1e-9);
    }
}
