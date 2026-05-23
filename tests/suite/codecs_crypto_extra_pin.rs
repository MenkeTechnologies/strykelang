//! Extra codec + crypto pins (2026-05-23) that complement
//! `native_codec.rs` and `csv_codec_pin.rs`:
//!
//! - `sha256("")` — the FIPS 180-4 empty-string vector (`native_codec.rs`
//!   only pinned `"abc"`).
//! - BLAKE3 known vectors (no existing coverage).
//! - HMAC-SHA256 RFC 4231 test case 1 with hex-decoded key
//!   `0x0b * 20` (existing test uses an ASCII key — different vector).
//! - `pack/unpack` combined templates (`A4N`, `C*`).
//! - `from_csv` header-aware shape: returns array-of-hashref keyed by
//!   the first row, NOT array-of-arrayref.
//! - `hmac_sha256` argument-order pin: `(KEY, DATA)`, NOT `(DATA, KEY)`.

use crate::common::*;

// ── SHA-256: FIPS 180-4 empty-string vector ─────────────────────────────

#[test]
fn sha256_empty_string_matches_fips_vector() {
    assert_eq!(
        eval_string(r#"sha256("")"#),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
}

// ── BLAKE3 known vectors ────────────────────────────────────────────────

#[test]
fn blake3_empty_string_matches_vector() {
    assert_eq!(
        eval_string(r#"blake3("")"#),
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
    );
}

#[test]
fn blake3_abc_matches_vector() {
    assert_eq!(
        eval_string(r#"blake3("abc")"#),
        "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
    );
}

// ── HMAC-SHA256 RFC 4231 test case 1 ─────────────────────────────────────
// Key = 0x0b repeated 20 times, data = "Hi There".
// Pins both the canonical vector and the argument order (KEY, DATA).

#[test]
fn hmac_sha256_rfc4231_test_case_1() {
    assert_eq!(
        eval_string(r#"hmac_sha256(hex_decode("0b" x 20), "Hi There")"#),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
    );
}

#[test]
fn hmac_sha256_arg_order_is_key_then_data() {
    // Swapping args produces a totally different digest. This pins that
    // the (key, data) order is the contract — important because Perl's
    // `Digest::SHA::hmac_sha256_hex` is also (data, key). Stryke
    // chose (key, data) to align with OpenSSL / Rust / Go conventions.
    let key_first = eval_string(r#"hmac_sha256("k", "data")"#);
    let data_first = eval_string(r#"hmac_sha256("data", "k")"#);
    assert_ne!(
        key_first, data_first,
        "swapped args must produce different digests",
    );
}

// ── pack / unpack — combined templates ──────────────────────────────────

#[test]
fn pack_unpack_a4n_combined_template() {
    let code = r#"
        my $rec = pack("A4N", "STRK", 42);
        my @d = unpack("A4N", $rec);
        $d[0] eq "STRK" && $d[1] == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pack_a4n_produces_eight_bytes() {
    assert_eq!(eval_int(r#"len(pack("A4N", "STRK", 42))"#), 8);
}

#[test]
fn unpack_c_star_expands_each_byte() {
    let code = r#"
        my @b = unpack("C*", "ABC");
        $b[0] == 65 && $b[1] == 66 && $b[2] == 67 && len(@b) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pack_n_uint32_big_endian_roundtrip() {
    let code = r#"
        my $b = pack("N", 0xDEADBEEF);
        my @x = unpack("N", $b);
        len($b) == 4 && $x[0] == 3735928559 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── from_csv — header-aware shape ───────────────────────────────────────

#[test]
fn from_csv_returns_array_of_hashref_keyed_by_header() {
    // Pins the contract: first line = headers, every subsequent line
    // becomes a hashref where the header names are keys. NOT
    // array-of-array — that distinction matters for downstream code
    // that does `$row->{name}` instead of `$row->[0]`.
    let code = r#"
        my $r = from_csv("name,age\nalice,30\nbob,25\n");
        len(@$r) == 2
            && $r->[0]->{name} eq "alice"
            && $r->[0]->{age}  == 30
            && $r->[1]->{name} eq "bob"
            && $r->[1]->{age}  == 25
            ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn from_csv_skips_blank_lines() {
    let code = r#"
        my $r = from_csv("a,b\n1,2\n\n3,4\n");
        len(@$r) == 2
            && $r->[0]->{a} == 1
            && $r->[1]->{a} == 3
            ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── URL encode / decode — spaces and reserved chars ─────────────────────

#[test]
fn url_encode_spaces_as_percent_20() {
    assert_eq!(eval_string(r#"url_encode("hello world")"#), "hello%20world");
}

#[test]
fn url_decode_round_trips_percent_encoding() {
    assert_eq!(eval_string(r#"url_decode("hello%20world")"#), "hello world");
}

// ── Stats: stddev sample formula ────────────────────────────────────────
// stddev of (2,4,4,4,5,5,7,9) is exactly 2 under sample-stddev (n-1).

#[test]
fn stddev_sample_of_known_vector_is_two() {
    assert_eq!(eval_int(r#"int(stddev(2,4,4,4,5,5,7,9))"#), 2);
}

#[test]
fn mean_of_one_through_ten_is_5_5() {
    // Pin both spellings — `mean` and the pipeline form.
    assert_eq!(eval_string(r#"mean(1,2,3,4,5,6,7,8,9,10)"#), "5.5");
    assert_eq!(eval_string(r#"(1:10) |> mean"#), "5.5");
}
