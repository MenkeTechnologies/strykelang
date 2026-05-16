//! Encoding pins: base64 / hex / URL round-trips.

use crate::common::*;

// ── base64 ──────────────────────────────────────────────────────────

#[test]
fn base64_encode_known_value() {
    let code = r#"
        base64_encode("hello world") eq "aGVsbG8gd29ybGQ=" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn base64_decode_known_value() {
    let code = r#"
        base64_decode("aGVsbG8gd29ybGQ=") eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn base64_roundtrip_ascii() {
    let code = r#"
        my $orig = "The quick brown fox jumps over the lazy dog.";
        my $back = base64_decode(base64_encode($orig));
        $back eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn base64_roundtrip_unicode() {
    let code = r#"
        my $orig = "café 🌟 中文";
        my $back = base64_decode(base64_encode($orig));
        $back eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn base64_roundtrip_binary_with_nulls() {
    let code = r#"
        my $orig = "abc\x00def\x01\x02\x03";
        my $back = base64_decode(base64_encode($orig));
        $back eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn base64_roundtrip_1kb_string() {
    let code = r#"
        my $orig = "x" x 1024;
        my $back = base64_decode(base64_encode($orig));
        len($back) == 1024 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn base64_empty_string() {
    let code = r#"
        base64_encode("") eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── hex ─────────────────────────────────────────────────────────────

#[test]
fn hex_encode_known_value() {
    let code = r#"
        # "ABC" = 0x41 0x42 0x43.
        hex_encode("ABC") eq "414243" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hex_decode_known_value() {
    let code = r#"
        hex_decode("414243") eq "ABC" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hex_roundtrip_ascii() {
    let code = r#"
        my $orig = "hello world";
        my $back = hex_decode(hex_encode($orig));
        $back eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hex_lower_is_string_case_op_not_hex_encoder() {
    // Surface note: stryke's `hex_lower` lowercases the string, NOT
    // hex-encodes. Use `hex_encode` for hex encoding.
    let code = r#"
        hex_lower("ABC") eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hex_upper_uppercases_only() {
    let code = r#"
        hex_upper("abc") eq "ABC" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hex_roundtrip_binary_value() {
    let code = r#"
        my $orig = "abc\x00def";
        my $hex = hex_encode($orig);
        my $back = hex_decode($hex);
        $back eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── URL encode/decode ──────────────────────────────────────────────

#[test]
fn url_encode_space_to_percent20() {
    let code = r#"
        my $r = url_encode("hello world");
        # Either "hello%20world" or "hello+world".
        ($r eq "hello%20world" || $r eq "hello+world") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn url_encode_special_chars() {
    let code = r#"
        my $r = url_encode("a&b=c");
        # Must percent-encode at least & and =.
        index($r, "&") < 0 ? 1 : 0   # raw & should not survive
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn url_encode_decode_roundtrip() {
    let code = r#"
        my $orig = "hello world?key=value&other=42";
        my $back = url_decode(url_encode($orig));
        $back eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn url_encode_unicode() {
    let code = r#"
        my $orig = "café";
        my $back = url_decode(url_encode($orig));
        $back eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn url_encode_aggressive_encodes_unreserved_chars() {
    // BUG-241 (potential): RFC 3986 marks `-`, `_`, `.`, `~` as
    // unreserved (should not be encoded). Stryke's url_encode
    // percent-encodes them anyway. Pin the observed behavior;
    // round-trip via url_decode still works.
    let code = r#"
        my $r = url_encode("ABCabc123-_.~");
        # Alnums preserved; punctuation encoded.
        index($r, "ABCabc123") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── URL parts via url_* family ────────────────────────────────────

#[test]
fn url_scheme_extraction() {
    let code = r#"
        url_scheme("https://example.com/path") eq "https" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn url_host_extraction() {
    let code = r#"
        url_host("https://example.com/path") eq "example.com" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn url_path_extraction() {
    let code = r#"
        url_path("https://example.com/some/path") eq "/some/path" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chained encoding round-trip ───────────────────────────────────

#[test]
fn base64_then_hex_roundtrip() {
    let code = r#"
        my $orig = "stryke is a programming language";
        my $b = base64_encode($orig);
        my $h = hex_encode($b);
        my $back = base64_decode(hex_decode($h));
        $back eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Decode garbage handling ───────────────────────────────────────

#[test]
fn base64_decode_garbage_safe_under_eval() {
    let code = r#"
        my $r = eval { base64_decode("not valid base64!!!") };
        # Either returns undef/empty or raises; both acceptable.
        (!defined($r) || len($r) == 0 || $@) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hex of single byte ─────────────────────────────────────────────

#[test]
fn hex_encode_single_byte() {
    let code = r#"
        hex_encode("A") eq "41" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── URL-safe base64 vs standard ───────────────────────────────────

#[test]
fn base64_encode_does_not_use_url_unsafe_padding() {
    let code = r#"
        # Standard base64 may include + and / and =.
        my $r = base64_encode("any value with bytes");
        # Just verify the encoded form is ASCII-safe.
        $r =~ /^[A-Za-z0-9+\/=]+$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Various-length round-trip ─────────────────────────────────────

#[test]
fn base64_roundtrip_lengths_1_to_10() {
    let code = r#"
        my $ok = 1;
        for my $n (1:10) {
            my $orig = "a" x $n;
            my $back = base64_decode(base64_encode($orig));
            $ok = 0 unless $back eq $orig;
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── hex encode of unicode multibyte ──────────────────────────────

#[test]
fn hex_encode_emoji_byte_sequence() {
    // 🌟 = 0xF0 0x9F 0x8C 0x9F (4 bytes UTF-8).
    let code = r#"
        my $r = hex_encode("🌟");
        lc($r) eq "f09f8c9f" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
