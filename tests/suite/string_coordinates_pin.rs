//! String coordinate-system pins.
//!
//! Stryke runs string code in **two** parallel coordinate systems:
//!
//!   Perl 5 layer (byte-indexed)  — `length`, `substr`, `index`,
//!                                  `rindex`. Kept for `.pm`-source
//!                                  compat + binary-protocol use.
//!   Stryke layer (codepoint)     — `len`, `$s[i]`, `$s[a:b]`,
//!                                  `cindex`, `crindex`. User-facing
//!                                  text handling.
//!
//! The two are **never** auto-converted. Mixing across systems
//! silently mis-aligns on any non-ASCII content — exactly the fakery
//! the 2026-05-15 audit found in `pauli_x flat length` and other
//! tests that confused the two.

use crate::common::*;

// ── ASCII baseline — both layers agree ────────────────────────────────

#[test]
fn ascii_length_and_len_agree() {
    let code = r#"
        my $s = "hello world";
        (length($s) == 11 && len($s) == 11) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ascii_index_and_cindex_agree() {
    let code = r#"
        my $s = "abcdef";
        (index($s, "c") == 2 && cindex($s, "c") == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── em dash (3 bytes, 1 codepoint) ────────────────────────────────────

#[test]
fn em_dash_length_is_byte_count() {
    // "hello — world" = 11 ASCII bytes + 3-byte em dash + 0-byte
    // separation = 14? Actually: "hello" (5) + space (1) + em dash (3)
    // + space (1) + "world" (5) = 15 bytes total.
    let code = r#"
        my $s = "hello — world";
        length($s) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn em_dash_len_is_codepoint_count() {
    // Same string: 5 + 1 + 1 + 1 + 5 = 13 codepoints.
    let code = r#"
        my $s = "hello — world";
        len($s) == 13 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn em_dash_index_finds_byte_offset() {
    // "au" doesn't exist; use a simpler probe — "world" starts at
    // byte 8 (5 ASCII + space + 3-byte em dash + ... wait: "hello "
    // is 6 bytes, then "—" is 3 bytes (positions 6-8), then " " at 9,
    // then "world" starts at byte 10. cindex sees it at codepoint 8.
    let code = r#"
        my $s = "hello — world";
        index($s, "world") == 10 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn em_dash_cindex_finds_codepoint_offset() {
    let code = r#"
        my $s = "hello — world";
        cindex($s, "world") == 8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── café (2-byte é) ───────────────────────────────────────────────────

#[test]
fn cafe_length_vs_len() {
    let code = r#"
        my $s = "café";
        # 'c' 'a' 'f' (3 bytes) + 'é' (2 bytes) = 5 bytes
        # 4 codepoints
        (length($s) == 5 && len($s) == 4) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cafe_index_au_finds_byte_position() {
    let code = r#"
        my $s = "café au lait";
        # byte position of "au": "café " = 6 bytes, then "au" at 6
        index($s, "au") == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cafe_cindex_au_finds_codepoint_position() {
    let code = r#"
        my $s = "café au lait";
        # codepoint position: 4 cp ("café") + 1 cp (" ") = 5
        cindex($s, "au") == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Emoji (4-byte BMP escapes) ────────────────────────────────────────

#[test]
fn emoji_length_is_four_bytes_per_emoji() {
    let code = r#"
        my $s = "🔑";
        length($s) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn emoji_len_is_one_codepoint_per_emoji() {
    let code = r#"
        my $s = "🔑";
        len($s) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn emoji_string_length_and_len_diverge() {
    let code = r#"
        my $s = "🔑 keys 🔐";
        # 4 bytes + " keys " (6 bytes) + 4 bytes = 14 bytes
        # 1 cp + 6 cp + 1 cp = 8 codepoints
        (length($s) == 14 && len($s) == 8) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slicing — substr (bytes) vs $s[a:b] (codepoints) ──────────────────

#[test]
fn substr_byte_indexed() {
    let code = r#"
        my $s = "rëd hot";
        # "rëd hot" = 'r' (1) 'ë' (2 bytes) 'd' (1) ' ' (1) 'h' 'o' 't' (3) = 8 bytes
        # substr 0,3 should grab "rë" (3 bytes: r + 2-byte ë)
        substr($s, 0, 3) eq "rë" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bracket_slice_codepoint_indexed() {
    let code = r#"
        my $s = "rëd hot";
        # codepoint [0:2] = first 3 codepoints (r, ë, d)
        $s[0:2] eq "rëd" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iteration over codepoints ────────────────────────────────────────

#[test]
fn iterating_via_bracket_index_yields_codepoints() {
    let code = r#"
        my $s = "🌟café";
        my @chars;
        for my $i (0:len($s)-1) {
            push @chars, $s[$i];
        }
        join(",", @chars) eq "🌟,c,a,f,é" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty + edge ─────────────────────────────────────────────────────

#[test]
fn empty_string_length_zero_both_layers() {
    let code = r#"
        my $s = "";
        (length($s) == 0 && len($s) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_missing_needle_returns_minus_one_both_layers() {
    let code = r#"
        my $s = "hello";
        (index($s, "z") == -1 && cindex($s, "z") == -1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Cyrillic + CJK round-trip via len + slice ────────────────────────

#[test]
fn cyrillic_codepoint_count_matches_visible_chars() {
    let code = r#"
        my $s = "Привет";
        # 6 Cyrillic letters; each is 2 bytes in UTF-8.
        (length($s) == 12 && len($s) == 6) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cjk_codepoint_count_matches_visible_chars() {
    let code = r#"
        my $s = "你好";
        # 2 CJK ideographs; each is 3 bytes in UTF-8.
        (length($s) == 6 && len($s) == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sigil-quoted strings work the same way (no mode flip) ─────────────

#[test]
fn single_quoted_string_obeys_same_rules() {
    let code = r#"
        my $s = 'café';
        (length($s) == 5 && len($s) == 4) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
