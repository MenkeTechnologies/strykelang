//! Pins for `hide` / `reveal` / `hide_capacity` — polymorphic steganography.
//!
//! Carrier dispatch (text vs PNG) is detected from the input bytes; the wire format
//! (4-byte BE length + secret + 4-byte BE CRC32) is shared, so the same `reveal`
//! handles either carrier. Pinned here:
//!
//!   * text round-trip (ASCII + Unicode)
//!   * PNG round-trip (binary secret survives LSB embed)
//!   * key-XOR mode: same key recovers, wrong key fails CRC or returns garbage
//!   * CRC corruption (bit-flip in stego) → reveal errors
//!   * capacity guard: too-small carrier → hide errors with clear message
//!   * `hide_capacity` returns useful numbers and tracks carrier size
//!
//! PNG fixtures are generated with the `image` crate so the test stays
//! deterministic across machines and doesn't depend on a checked-in binary.

use crate::common::*;

fn tmp_path(suffix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("/tmp/stryke_stego_pin_{}_{}.tmp", nanos, suffix)
}

/// Generate a 64x64 PNG of a colour gradient — bigger than any envelope we test.
/// Uses `PngEncoder` directly so we don't depend on the file extension hint that
/// `image::save` reads to pick a format.
fn write_gradient_png(path: &str) {
    use image::{ExtendedColorType, ImageEncoder};
    let mut img = image::RgbaImage::new(64, 64);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = image::Rgba([(x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8, 255]);
    }
    let file = std::fs::File::create(path).expect("create PNG file");
    image::codecs::png::PngEncoder::new(file)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgba8,
        )
        .expect("encode PNG");
}

#[test]
fn text_carrier_roundtrip_ascii() {
    // 11-byte secret + 8-byte envelope = 152 bits → carrier must have ≥152 visible chars.
    let code = r#"
        my $carrier = "The quick brown fox jumps over the lazy dog. " x 4;
        my $stego = hide($carrier, "hello world");
        reveal($stego) eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn text_carrier_roundtrip_unicode_secret() {
    // Unicode secret bytes are preserved exactly (UTF-8 byte sequence round-trips).
    let code = r#"
        my $carrier = ("Lorem ipsum dolor sit amet, consectetur adipiscing elit. " x 6);
        my $stego = hide($carrier, "café 🌟");
        reveal($stego) eq "café 🌟" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn text_carrier_capacity_error_when_carrier_too_small() {
    // 1000-byte secret needs (1000+8)*8 = 8064 bits / visible chars — way more than "short".
    let kind = eval_err_kind(r#"hide("short carrier", "x" x 1000)"#);
    assert!(
        matches!(kind, stryke::error::ErrorKind::Runtime { .. }),
        "expected runtime error for over-capacity, got {kind:?}"
    );
}

#[test]
fn text_carrier_corruption_caught_by_crc32() {
    // Prepend an extra zero-width char to the stego, shifting the entire embedded
    // bit stream by one position. The length prefix is now corrupt → reveal
    // either fails the bounds check or fails CRC32.
    let code = "
        my $stego = hide(\"The quick brown fox jumps over the lazy dog. \" x 6, \"ABC\");
        $stego = \"\\x{200B}\" . $stego;
        my $err;
        try { reveal($stego); } catch ($e) { $err = \"$e\"; }
        defined($err) && $err =~ /corrupt|exceeds/ ? 1 : 0
    ";
    assert_eq!(
        eval_int(code),
        1,
        "tampered stego must surface as a reveal error"
    );
}

#[test]
fn key_xor_roundtrip_recovers_with_same_key() {
    let code = r#"
        my $carrier = "The quick brown fox jumps over the lazy dog. " x 6;
        my $stego   = hide($carrier, "secret!", "my-passphrase");
        reveal($stego, "my-passphrase") eq "secret!" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn key_xor_with_wrong_key_does_not_recover_plaintext() {
    // The CRC is computed AFTER XOR so it still validates — but the recovered bytes
    // are an XOR mask of the wrong key against the right ciphertext, i.e. random
    // garbage that should NOT equal the original secret.
    let code = r#"
        my $carrier = "The quick brown fox jumps over the lazy dog. " x 6;
        my $stego   = hide($carrier, "secret!", "right-key");
        my $back    = reveal($stego, "wrong-key");
        $back eq "secret!" ? 0 : 1
    "#;
    assert_eq!(
        eval_int(code),
        1,
        "wrong key must not yield the original plaintext"
    );
}

#[test]
fn hide_capacity_text_carrier_tracks_visible_chars() {
    // 100 visible chars → 100 bits envelope budget → (100/8) - 8 = 4 secret bytes.
    let code = r#"
        my $carrier = "x" x 100;
        hide_capacity($carrier)
    "#;
    assert_eq!(eval_int(code), 4);
}

#[test]
fn png_carrier_roundtrip_binary_secret() {
    let png_path = tmp_path("png_carrier.png");
    write_gradient_png(&png_path);
    let code = format!(
        r#"
            my $png    = slurp("{path}");
            my $secret = pack("C*", 0x00, 0xff, 0xde, 0xad, 0xbe, 0xef);
            my $stego  = hide($png, $secret);
            my $back   = reveal($stego);
            $back eq $secret ? 1 : 0
        "#,
        path = png_path,
    );
    let ok = eval_int(&code);
    let _ = std::fs::remove_file(&png_path);
    assert_eq!(ok, 1, "PNG carrier must round-trip raw bytes byte-for-byte");
}

#[test]
fn png_carrier_with_key_roundtrips() {
    let png_path = tmp_path("png_keyed.png");
    write_gradient_png(&png_path);
    let code = format!(
        r#"
            my $png   = slurp("{path}");
            my $stego = hide($png, "needle", "haystack");
            reveal($stego, "haystack") eq "needle" ? 1 : 0
        "#,
        path = png_path,
    );
    let ok = eval_int(&code);
    let _ = std::fs::remove_file(&png_path);
    assert_eq!(ok, 1);
}

#[test]
fn png_carrier_capacity_reports_pixel_budget() {
    // 64x64 PNG = 64*64*3 = 12288 bits / 8 = 1536 bytes total - 8 envelope = 1528 usable.
    let png_path = tmp_path("png_cap.png");
    write_gradient_png(&png_path);
    let code = format!(r#"hide_capacity(slurp("{path}"))"#, path = png_path);
    let n = eval_int(&code);
    let _ = std::fs::remove_file(&png_path);
    assert_eq!(
        n, 1528,
        "64x64 PNG R+G+B LSBs hold (64*64*3/8) − 8-byte envelope = 1528 secret bytes"
    );
}
