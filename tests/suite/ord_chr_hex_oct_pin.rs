//! Numeric-conversion builtin pins: `ord`, `chr`, `hex`, `oct`.

use crate::common::*;

// ── ord ────────────────────────────────────────────────────────────

#[test]
fn ord_uppercase_a() {
    assert_eq!(eval_int(r#"ord("A") == 65 ? 1 : 0"#), 1);
}

#[test]
fn ord_lowercase_z() {
    assert_eq!(eval_int(r#"ord("z") == 122 ? 1 : 0"#), 1);
}

#[test]
fn ord_digit_zero() {
    assert_eq!(eval_int(r#"ord("0") == 48 ? 1 : 0"#), 1);
}

#[test]
fn ord_empty_string_is_zero() {
    assert_eq!(eval_int(r#"ord("") == 0 ? 1 : 0"#), 1);
}

#[test]
fn ord_multi_char_returns_first_only() {
    assert_eq!(eval_int(r#"ord("Apple") == 65 ? 1 : 0"#), 1);
}

#[test]
fn ord_newline() {
    assert_eq!(eval_int(r#"ord("\n") == 10 ? 1 : 0"#), 1);
}

#[test]
fn ord_tab() {
    assert_eq!(eval_int(r#"ord("\t") == 9 ? 1 : 0"#), 1);
}

#[test]
fn ord_snowman_bmp_codepoint() {
    // ☃ is U+2603 = 9731.
    assert_eq!(eval_int(r#"ord("\x{2603}") == 9731 ? 1 : 0"#), 1);
}

// ── chr ────────────────────────────────────────────────────────────

#[test]
fn chr_65_is_capital_a() {
    assert_eq!(eval_int(r#"chr(65) eq "A" ? 1 : 0"#), 1);
}

#[test]
fn chr_122_is_lowercase_z() {
    assert_eq!(eval_int(r#"chr(122) eq "z" ? 1 : 0"#), 1);
}

#[test]
fn chr_zero_yields_nul_byte() {
    // Length 1 (single NUL byte).
    assert_eq!(eval_int(r#"length(chr(0)) == 1 ? 1 : 0"#), 1);
}

#[test]
fn chr_127_is_del() {
    assert_eq!(eval_int(r#"ord(chr(127)) == 127 ? 1 : 0"#), 1);
}

#[test]
fn chr_256_yields_two_utf8_bytes() {
    // U+0100 ('Ā') encodes to 2 UTF-8 bytes.
    assert_eq!(eval_int(r#"length(chr(256)) == 2 ? 1 : 0"#), 1);
}

#[test]
fn chr_snowman_round_trip() {
    assert_eq!(eval_int(r#"chr(0x2603) eq "\x{2603}" ? 1 : 0"#), 1);
}

#[test]
fn chr_round_trip_through_ord() {
    assert_eq!(eval_int(r#"ord(chr(42)) == 42 ? 1 : 0"#), 1);
}

#[test]
fn chr_above_unicode_max_returns_empty() {
    // Stryke: chr(0x110000) returns "" (Unicode max is U+10FFFF).
    assert_eq!(eval_int(r#"chr(0x110000) eq "" ? 1 : 0"#), 1);
}

// ── hex ────────────────────────────────────────────────────────────

#[test]
fn hex_bare_ff() {
    assert_eq!(eval_int(r#"hex("ff") == 255 ? 1 : 0"#), 1);
}

#[test]
fn hex_uppercase_ff() {
    assert_eq!(eval_int(r#"hex("FF") == 255 ? 1 : 0"#), 1);
}

#[test]
fn hex_with_lowercase_prefix() {
    assert_eq!(eval_int(r#"hex("0xff") == 255 ? 1 : 0"#), 1);
}

#[test]
fn hex_with_uppercase_prefix() {
    assert_eq!(eval_int(r#"hex("0XFF") == 255 ? 1 : 0"#), 1);
}

#[test]
fn hex_zero() {
    assert_eq!(eval_int(r#"hex("0") == 0 ? 1 : 0"#), 1);
}

#[test]
fn hex_empty_is_zero() {
    assert_eq!(eval_int(r#"hex("") == 0 ? 1 : 0"#), 1);
}

#[test]
fn hex_garbage_is_zero() {
    // Non-hex input → 0 (matches Perl; warns by default).
    assert_eq!(eval_int(r#"hex("zzz") == 0 ? 1 : 0"#), 1);
}

#[test]
fn hex_cafe() {
    assert_eq!(eval_int(r#"hex("CAFE") == 51966 ? 1 : 0"#), 1);
}

#[test]
fn hex_deadbeef() {
    assert_eq!(eval_int(r#"hex("DEADBEEF") == 3735928559 ? 1 : 0"#), 1);
}

#[test]
fn hex_with_leading_space() {
    // Stryke skips leading whitespace.
    assert_eq!(eval_int(r#"hex(" ff") == 255 ? 1 : 0"#), 1);
}

// ── oct ────────────────────────────────────────────────────────────

#[test]
fn oct_bare_octal() {
    // "010" → 8 (octal).
    assert_eq!(eval_int(r#"oct("010") == 8 ? 1 : 0"#), 1);
}

#[test]
fn oct_with_zero_octal_prefix() {
    assert_eq!(eval_int(r#"oct("0777") == 511 ? 1 : 0"#), 1);
}

#[test]
fn oct_with_hex_prefix() {
    // oct() accepts 0x as well; "0x10" → 16.
    assert_eq!(eval_int(r#"oct("0x10") == 16 ? 1 : 0"#), 1);
}

#[test]
fn oct_with_binary_prefix() {
    // "0b101" → 5.
    assert_eq!(eval_int(r#"oct("0b101") == 5 ? 1 : 0"#), 1);
}

#[test]
fn oct_with_explicit_octal_prefix() {
    // Modern Perl supports `0o`; stryke does too.
    assert_eq!(eval_int(r#"oct("0o17") == 15 ? 1 : 0"#), 1);
}

#[test]
fn oct_empty_string_is_zero() {
    assert_eq!(eval_int(r#"oct("") == 0 ? 1 : 0"#), 1);
}

#[test]
fn oct_garbage_is_zero() {
    assert_eq!(eval_int(r#"oct("zzz") == 0 ? 1 : 0"#), 1);
}

#[test]
fn oct_negative_octal() {
    assert_eq!(eval_int(r#"oct("-010") == -8 ? 1 : 0"#), 1);
}

#[test]
fn oct_full_unix_perms() {
    // 0o755 = 493 (rwxr-xr-x).
    assert_eq!(eval_int(r#"oct("755") == 493 ? 1 : 0"#), 1);
}

// ── composition ───────────────────────────────────────────────────

#[test]
fn ord_chr_round_trip_for_ascii() {
    let code = r#"
        my $ok = 1;
        for my $c (0:127) {
            $ok = 0 unless ord(chr($c)) == $c;
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hex_format_round_trip() {
    let code = r#"
        my @nums = (0, 1, 15, 16, 255, 256, 65535);
        my $ok = 1;
        for my $n (@nums) {
            my $h = sprintf("%x", $n);
            $ok = 0 unless hex($h) == $n;
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn oct_format_round_trip() {
    let code = r#"
        my @nums = (0, 1, 7, 8, 63, 64, 511, 4095);
        my $ok = 1;
        for my $n (@nums) {
            my $o = sprintf("%o", $n);
            $ok = 0 unless oct($o) == $n;
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── derived idioms ────────────────────────────────────────────────

#[test]
fn alphabet_via_chr_map() {
    let code = r#"
        my @letters = map { chr($_) } (ord("a"):ord("z"));
        join("", @letters) eq "abcdefghijklmnopqrstuvwxyz" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn caesar_cipher_shift_three() {
    let code = r#"
        fn Demo::OCH::shift_char($c, $n) {
            my $o = ord($c);
            chr(((($o - ord("a")) + $n) % 26) + ord("a"))
        }
        my @out = map { Demo::OCH::shift_char($_, 3) } split //, "abcxyz";
        join("", @out) eq "defabc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hex_color_parse() {
    let code = r#"
        my $color = "FF8800";
        my $r = hex(substr($color, 0, 2));
        my $g = hex(substr($color, 2, 2));
        my $b = hex(substr($color, 4, 2));
        ($r == 255 && $g == 136 && $b == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn perm_string_via_oct() {
    let code = r#"
        # "rwxr-xr-x" decodes to 0755 = 493.
        oct("755") == 493 && oct("644") == 420 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
