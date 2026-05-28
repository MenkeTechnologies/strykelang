//! Polymorphic steganography: `hide(CARRIER, SECRET [, KEY])` / `reveal(STEGO [, KEY])`.
//!
//! Carrier kind is detected from the input bytes:
//!
//!   * `\x89PNG\r\n\x1a\n` magic → LSB embed in the R/G/B channels (alpha skipped so
//!     transparent regions cannot leak hidden data). Capacity = `width * height * 3 / 8`.
//!   * otherwise → text carrier, secret bits encoded as one zero-width char
//!     (U+200B for 0, U+200C for 1) inserted **after** each visible code point.
//!     Capacity = visible-char count / 8.
//!
//! Wire format embedded in either carrier so `reveal` knows where to stop:
//!
//!   `[4-byte BE length][secret bytes][4-byte BE CRC32-IEEE of (length || secret)]`
//!
//! Optional `key`: SHA-256(key || counter_be32)-derived XOR stream is applied to the
//! secret before embedding (and after extraction). Defeats casual extract; not
//! cryptography — for real privacy, encrypt before passing in.
//!
//! This is the only file-system-independent stego primitive in this codebase; the
//! public surface lives in `builtins::try_builtin` under `"hide"` / `"reveal"` /
//! `"hide_capacity"`.
use crc32fast::Hasher as Crc32;
use image::ImageEncoder;
use sha2::{Digest, Sha256};

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const ZW0: char = '\u{200B}';
const ZW1: char = '\u{200C}';

/// Wrap `secret` in the envelope `[len-be][secret][crc-be]`, optionally XOR-masking
/// the secret with a SHA-256(key||counter)-derived stream first.
pub fn wrap_payload(secret: &[u8], key: Option<&[u8]>) -> Vec<u8> {
    let mut body = secret.to_vec();
    if let Some(k) = key {
        xor_stream(&mut body, k);
    }
    let len = body.len() as u32;
    let mut crc = Crc32::new();
    crc.update(&len.to_be_bytes());
    crc.update(&body);
    let cksum = crc.finalize();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&cksum.to_be_bytes());
    out
}

/// Validate `envelope` and recover the original secret. Errors surface as user-facing
/// "reveal: ..." strings (the caller prepends nothing).
pub fn unwrap_payload(envelope: &[u8], key: Option<&[u8]>) -> Result<Vec<u8>, String> {
    if envelope.len() < 8 {
        return Err("reveal: corrupt or absent payload (envelope shorter than 8 bytes)".into());
    }
    let len = u32::from_be_bytes([envelope[0], envelope[1], envelope[2], envelope[3]]) as usize;
    if envelope.len() < 4 + len + 4 {
        return Err(
            "reveal: corrupt or absent payload (declared length exceeds embedded bits)".into(),
        );
    }
    let body = &envelope[4..4 + len];
    let stored_crc = u32::from_be_bytes([
        envelope[4 + len],
        envelope[5 + len],
        envelope[6 + len],
        envelope[7 + len],
    ]);
    let mut crc = Crc32::new();
    crc.update(&envelope[0..4]);
    crc.update(body);
    if crc.finalize() != stored_crc {
        return Err("reveal: corrupt or absent payload (CRC32 mismatch)".into());
    }
    let mut out = body.to_vec();
    if let Some(k) = key {
        xor_stream(&mut out, k);
    }
    Ok(out)
}

/// Deterministic XOR stream derived from `key`: each 32-byte block is `SHA-256(key || counter_be32)`.
fn xor_stream(buf: &mut [u8], key: &[u8]) {
    let mut counter: u32 = 0;
    let mut pos = 0;
    while pos < buf.len() {
        let mut h = Sha256::new();
        h.update(key);
        h.update(counter.to_be_bytes());
        let block = h.finalize();
        let take = (buf.len() - pos).min(32);
        for i in 0..take {
            buf[pos + i] ^= block[i];
        }
        pos += take;
        counter = counter.wrapping_add(1);
    }
}

// ── PNG carrier (LSB on R/G/B, alpha skipped) ────────────────────────────

pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(PNG_MAGIC)
}

/// Total embeddable bits in a PNG: width * height * 3 (R+G+B LSBs).
pub fn png_capacity_bits(png_bytes: &[u8]) -> Result<usize, String> {
    let img = image::load_from_memory(png_bytes)
        .map_err(|e| format!("hide_capacity: PNG decode failed: {}", e))?;
    Ok((img.width() as usize) * (img.height() as usize) * 3)
}

pub fn png_hide(png_bytes: &[u8], envelope: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(png_bytes)
        .map_err(|e| format!("hide: PNG decode failed: {}", e))?;
    let mut rgba = img.to_rgba8();
    let bits_needed = envelope.len() * 8;
    let cap = (rgba.width() as usize) * (rgba.height() as usize) * 3;
    if bits_needed > cap {
        return Err(format!(
            "hide: secret needs {bits_needed} bits but PNG carrier holds {cap} (R+G+B LSBs)"
        ));
    }
    let (w, h) = (rgba.width(), rgba.height());
    let raw: &mut [u8] = rgba.as_mut();
    let mut bit_idx = 0usize;
    'outer: for px in raw.chunks_exact_mut(4) {
        for c in &mut px[0..3] {
            if bit_idx >= bits_needed {
                break 'outer;
            }
            let bit = (envelope[bit_idx / 8] >> (7 - (bit_idx % 8))) & 1;
            *c = (*c & 0xFE) | bit;
            bit_idx += 1;
        }
    }
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("hide: PNG encode failed: {}", e))?;
    Ok(out)
}

pub fn png_reveal(png_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(png_bytes)
        .map_err(|e| format!("reveal: PNG decode failed: {}", e))?;
    let rgba = img.to_rgba8();
    let raw = rgba.as_raw();
    let total_bits = (rgba.width() as usize) * (rgba.height() as usize) * 3;
    if total_bits < 32 {
        return Err("reveal: PNG too small to hold a length prefix".into());
    }
    let mut len_bytes = [0u8; 4];
    for i in 0..32 {
        len_bytes[i / 8] |= lsb_bit_at(raw, i) << (7 - (i % 8));
    }
    let len = u32::from_be_bytes(len_bytes) as usize;
    let envelope_bits = (4 + len + 4) * 8;
    if envelope_bits > total_bits {
        return Err("reveal: declared length exceeds PNG capacity (no payload?)".into());
    }
    let mut envelope = vec![0u8; 4 + len + 4];
    for i in 0..envelope_bits {
        envelope[i / 8] |= lsb_bit_at(raw, i) << (7 - (i % 8));
    }
    Ok(envelope)
}

#[inline]
fn lsb_bit_at(raw: &[u8], bit_index: usize) -> u8 {
    // R+G+B per pixel (skip alpha at offset 3).
    let p = bit_index / 3;
    let c = bit_index % 3;
    raw[p * 4 + c] & 1
}

// ── Text carrier (zero-width chars) ──────────────────────────────────────

#[inline]
fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')
}

/// One bit per visible code point.
pub fn text_capacity_bits(text: &str) -> usize {
    text.chars().filter(|c| !is_zero_width(*c)).count()
}

pub fn text_hide(text: &str, envelope: &[u8]) -> Result<String, String> {
    let bits_needed = envelope.len() * 8;
    let cap = text_capacity_bits(text);
    if bits_needed > cap {
        return Err(format!(
            "hide: secret needs {bits_needed} bits but text carrier holds {cap} (one bit per visible char)"
        ));
    }
    let mut out = String::with_capacity(text.len() + bits_needed * 3);
    let mut bit_idx = 0usize;
    for ch in text.chars() {
        out.push(ch);
        if is_zero_width(ch) || bit_idx >= bits_needed {
            continue;
        }
        let bit = (envelope[bit_idx / 8] >> (7 - (bit_idx % 8))) & 1;
        out.push(if bit == 0 { ZW0 } else { ZW1 });
        bit_idx += 1;
    }
    Ok(out)
}

pub fn text_reveal(stego: &str) -> Result<Vec<u8>, String> {
    let mut bits = Vec::new();
    for ch in stego.chars() {
        match ch {
            ZW0 => bits.push(0u8),
            ZW1 => bits.push(1u8),
            _ => {}
        }
    }
    if bits.len() < 32 {
        return Err("reveal: not enough zero-width chars for length prefix".into());
    }
    let total_bytes = bits.len() / 8;
    let mut env = vec![0u8; total_bytes];
    for (i, b) in bits.iter().take(total_bytes * 8).enumerate() {
        env[i / 8] |= b << (7 - (i % 8));
    }
    let len = u32::from_be_bytes([env[0], env[1], env[2], env[3]]) as usize;
    if 4 + len + 4 > env.len() {
        return Err("reveal: declared length exceeds embedded bits (no payload?)".into());
    }
    env.truncate(4 + len + 4);
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrips_with_and_without_key() {
        let secret = b"hello world";
        let env = wrap_payload(secret, None);
        assert_eq!(unwrap_payload(&env, None).unwrap(), secret);

        let key = b"shared-passphrase";
        let env_k = wrap_payload(secret, Some(key));
        assert_eq!(unwrap_payload(&env_k, Some(key)).unwrap(), secret);
        // Wrong key → CRC still passes (XOR is post-CRC during wrap), but body differs.
        let unmasked = unwrap_payload(&env_k, Some(b"wrong-key")).unwrap();
        assert_ne!(unmasked, secret);
    }

    #[test]
    fn envelope_detects_corruption() {
        let mut env = wrap_payload(b"abc", None);
        let last = env.len() - 1;
        env[last] ^= 0x01; // flip one CRC bit
        assert!(unwrap_payload(&env, None).is_err());
    }

    #[test]
    fn text_carrier_roundtrip() {
        // 2-byte secret + 8-byte envelope = 80 bits → need ≥80 visible chars in carrier.
        let carrier = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
        let env = wrap_payload(b"hi", None);
        let stego = text_hide(carrier, &env).expect("hide");
        let env2 = text_reveal(&stego).expect("reveal");
        assert_eq!(unwrap_payload(&env2, None).unwrap(), b"hi");
    }

    #[test]
    fn text_carrier_rejects_too_small() {
        let env = wrap_payload(&[0u8; 100], None);
        let result = text_hide("short", &env);
        assert!(result.is_err());
    }

    // ── envelope structure ───────────────────────────────────────────

    #[test]
    fn wrap_payload_layout_len_body_crc() {
        let env = wrap_payload(b"abcd", None);
        // 4-byte BE length + 4-byte body + 4-byte BE CRC = 12 bytes.
        assert_eq!(env.len(), 12);
        assert_eq!(&env[0..4], &[0, 0, 0, 4]);
        assert_eq!(&env[4..8], b"abcd");
    }

    #[test]
    fn wrap_payload_empty_secret_still_carries_len_and_crc() {
        let env = wrap_payload(&[], None);
        assert_eq!(env.len(), 8);
        assert_eq!(&env[0..4], &[0, 0, 0, 0]);
        // CRC over the four length bytes alone.
        let mut h = Crc32::new();
        h.update(&[0, 0, 0, 0]);
        let crc = h.finalize().to_be_bytes();
        assert_eq!(&env[4..8], &crc);
        // And it roundtrips back to empty.
        assert_eq!(unwrap_payload(&env, None).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn wrap_payload_with_key_differs_from_unkeyed() {
        let env_a = wrap_payload(b"hello", None);
        let env_b = wrap_payload(b"hello", Some(b"k"));
        // Length prefix matches, body differs (XOR mask applied), CRC differs too.
        assert_eq!(&env_a[0..4], &env_b[0..4]);
        assert_ne!(&env_a[4..9], &env_b[4..9]);
    }

    #[test]
    fn unwrap_payload_rejects_short_envelope() {
        for n in 0..8 {
            let buf = vec![0u8; n];
            assert!(
                unwrap_payload(&buf, None).is_err(),
                "len {n} should fail short-envelope check"
            );
        }
    }

    #[test]
    fn unwrap_payload_rejects_declared_len_exceeding_buffer() {
        // Declared length = 1000 but the body bytes don't exist.
        let mut env = vec![0u8; 12];
        env[0..4].copy_from_slice(&1000u32.to_be_bytes());
        assert!(unwrap_payload(&env, None).is_err());
    }

    #[test]
    fn unwrap_payload_rejects_flipped_body_byte() {
        let mut env = wrap_payload(b"abcd", None);
        env[5] ^= 0xFF; // flip a body bit → CRC mismatch
        assert!(unwrap_payload(&env, None).is_err());
    }

    // ── XOR stream determinism ───────────────────────────────────────

    #[test]
    fn xor_stream_is_self_inverse() {
        let mut buf = b"the quick brown fox jumps over the lazy dog".to_vec();
        let original = buf.clone();
        xor_stream(&mut buf, b"secret");
        assert_ne!(buf, original, "first XOR should mutate the buffer");
        xor_stream(&mut buf, b"secret");
        assert_eq!(buf, original, "second XOR with same key must restore");
    }

    #[test]
    fn xor_stream_first_block_is_sha256_key_counter() {
        let mut buf = [0u8; 32];
        xor_stream(&mut buf, b"k");
        let mut h = Sha256::new();
        h.update(b"k");
        h.update(0u32.to_be_bytes());
        let block = h.finalize();
        // XOR'ing zeros yields the keystream block directly.
        assert_eq!(&buf[..], &block[..]);
    }

    #[test]
    fn xor_stream_handles_partial_trailing_block() {
        // 33 bytes → block 0 covers 32, block 1 contributes 1 byte.
        let mut buf = vec![0u8; 33];
        xor_stream(&mut buf, b"key");
        let mut h0 = Sha256::new();
        h0.update(b"key");
        h0.update(0u32.to_be_bytes());
        let b0 = h0.finalize();
        let mut h1 = Sha256::new();
        h1.update(b"key");
        h1.update(1u32.to_be_bytes());
        let b1 = h1.finalize();
        assert_eq!(&buf[0..32], &b0[..]);
        assert_eq!(buf[32], b1[0]);
    }

    // ── PNG / text carrier classification ────────────────────────────

    #[test]
    fn is_png_detects_magic_bytes() {
        assert!(is_png(b"\x89PNG\r\n\x1a\nrest..."));
        assert!(!is_png(b"GIF89a..."));
        assert!(!is_png(b""));
        assert!(!is_png(b"\x89PNG\r\n")); // truncated magic
    }

    #[test]
    fn is_zero_width_covers_all_four_codepoints() {
        for c in ['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'] {
            assert!(
                is_zero_width(c),
                "expected {:04X} to count as zero-width",
                c as u32
            );
        }
        for c in ['a', ' ', '\n', '\t', '\u{200A}', '\u{200E}', '\u{FFFD}'] {
            assert!(!is_zero_width(c), "expected {:04X} to NOT count", c as u32);
        }
    }

    #[test]
    fn text_capacity_bits_counts_visible_chars_only() {
        assert_eq!(text_capacity_bits(""), 0);
        assert_eq!(text_capacity_bits("abc"), 3);
        // Existing zero-widths in the carrier do not contribute capacity.
        assert_eq!(text_capacity_bits("a\u{200B}b\u{200C}c\u{FEFF}"), 3);
        // Multi-byte visible chars count once each (no UTF-8 byte inflation).
        assert_eq!(text_capacity_bits("aé你"), 3);
    }
}
