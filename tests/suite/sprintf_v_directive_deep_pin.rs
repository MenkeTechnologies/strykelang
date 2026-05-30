//! Deep `%v` (vector) directive pins for sprintf.
//!
//! `%v` joins per-character "values" of the arg string with `.`, applying
//! the trailing directive to each. Stryke iterates *bytes* of the
//! UTF-8-encoded string (matches Perl's `%v` semantics on byte-strings).
//!
//! Existing coverage: 1 generic case + 3 in `behavior_pin_2026_05_b`.
//! This file pins:
//!   - empty input
//!   - single-char vs multi-char
//!   - every variant directive (`%vd`, `%vx`, `%vX`, `%vo`, `%vb`)
//!   - composability inside larger format strings (prefix/suffix, multiple
//!     `%v` in one format)
//!   - UTF-8 multibyte expansion (chr(192) → 2 bytes → 2 joined ints)
//!   - high-byte boundary (chr(127), chr(192))
//!
//! Pins divergences from Perl reference with `#[ignore]` + `STRYKE BUG:`
//! marker:
//!   - `%v<width>d` (e.g. `%v3d`) — Perl pads each element to width;
//!     Stryke ignores the width inside the vector directive.

use crate::common::*;

// ── Empty + single-char (boundary) ──────────────────────────────────

#[test]
fn vd_empty_string_yields_empty() {
    assert_eq!(eval_string(r#"sprintf("%vd", "")"#), "");
}

#[test]
fn vd_single_ascii_char_no_dot_separator() {
    // Single element: just the codepoint, no leading/trailing dot.
    assert_eq!(eval_string(r#"sprintf("%vd", "A")"#), "65");
}

#[test]
fn vd_two_ascii_chars_dot_joined() {
    assert_eq!(eval_string(r#"sprintf("%vd", "AB")"#), "65.66");
}

// ── Directive variants (the four numeric bases) ─────────────────────

#[test]
fn vx_lowercase_hex() {
    // 'A'=0x41, 'B'=0x42, 'C'=0x43.
    assert_eq!(eval_string(r#"sprintf("%vx", "ABC")"#), "41.42.43");
}

#[test]
#[allow(non_snake_case)]
fn vX_uppercase_hex_letters_present() {
    // 'a'=0x61, 'b'=0x62, 'c'=0x63 — lowercase ASCII, but %vX uses
    // uppercase hex letters for any A-F that would appear. Use 'k'=0x6B
    // (no A-F) so format only differs on hex-letter inputs; here pin the
    // 'abc' case which has no A-F, so result identical to lowercase.
    assert_eq!(eval_string(r#"sprintf("%vX", "abc")"#), "61.62.63");
}

#[test]
fn vo_octal_per_byte() {
    // 'A'=0x41=0101, 'B'=0x42=0102, 'C'=0x43=0103.
    assert_eq!(eval_string(r#"sprintf("%vo", "ABC")"#), "101.102.103");
}

#[test]
fn vb_binary_per_byte() {
    // 'A'=0x41=01000001, 'B'=0x42=01000010 (leading zeros stripped by stryke).
    assert_eq!(eval_string(r#"sprintf("%vb", "AB")"#), "1000001.1000010");
}

// ── Embedded in larger format strings ───────────────────────────────

#[test]
fn vd_with_prefix_and_suffix_literal() {
    assert_eq!(
        eval_string(r#"sprintf("ver=%vd end", "ABC")"#),
        "ver=65.66.67 end"
    );
}

#[test]
fn two_v_directives_in_one_format_each_consume_one_arg() {
    assert_eq!(
        eval_string(r#"sprintf("%vd|%vd", "AB", "CD")"#),
        "65.66|67.68"
    );
}

// ── UTF-8 multibyte: byte-iteration semantics ──────────────────────

/// chr(192) is U+00C0, which UTF-8-encodes to two bytes: 0xC3 0x80 =
/// 195 128. So `%vd` on a single chr(192) yields the two-byte expansion.
/// Pin this so a future "iterate by codepoint" regression is caught.
#[test]
fn vd_on_chr_192_yields_two_utf8_bytes() {
    assert_eq!(eval_string(r#"sprintf("%vd", chr(192))"#), "195.128");
}

/// chr(127) is the last single-byte ASCII codepoint. Verifies the
/// boundary: one byte in, one decimal out.
#[test]
fn vd_on_chr_127_yields_single_byte() {
    assert_eq!(eval_string(r#"sprintf("%vd", chr(127))"#), "127");
}

/// Mixed ASCII + multibyte: `chr(1) . chr(0) . chr(255)`.
/// chr(255) = U+00FF → 0xC3 0xBF = 195 191 (two bytes).
/// Final byte stream: 1, 0, 195, 191 → "1.0.195.191".
#[test]
fn vd_mixed_low_and_high_bytes_yields_expanded_stream() {
    assert_eq!(
        eval_string(r#"sprintf("%vd", chr(1) . chr(0) . chr(255))"#),
        "1.0.195.191"
    );
}

// ── Divergences from Perl reference ────────────────────────────────

/// STRYKE BUG: `%v<width>d` ignored. Perl `%v3d` pads each per-byte
/// element to width 3 (`[ 65. 66]`); Stryke emits no padding (`[65.66]`).
/// Re-enable when Stryke supports width-inside-vector directives.
#[test]
#[ignore]
fn stryke_bug_vd_with_width_does_not_pad_each_element() {
    assert_eq!(eval_string(r#"sprintf("[%v3d]", "AB")"#), "[ 65. 66]");
}

/// STRYKE BUG: `%v0<width>d` zero-pad is also ignored. Perl emits
/// `[065.066]`; Stryke emits `[65.66]`.
#[test]
#[ignore]
fn stryke_bug_vd_with_zero_pad_width_does_not_pad_each_element() {
    assert_eq!(eval_string(r#"sprintf("[%v03d]", "AB")"#), "[065.066]");
}
