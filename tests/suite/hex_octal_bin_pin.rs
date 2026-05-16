//! Hex/octal/binary literal + coercion pins.

use crate::common::*;

// ── Source-literal prefixes ────────────────────────────────────────

#[test]
fn hex_literal_0x() {
    assert_eq!(eval_int("0xFF"), 255);
}

#[test]
fn hex_literal_uppercase() {
    assert_eq!(eval_int("0xABCD"), 0xABCD);
}

#[test]
fn hex_literal_mixed_case() {
    assert_eq!(eval_int("0xAbCd"), 0xABCD);
}

#[test]
fn binary_literal_0b() {
    assert_eq!(eval_int("0b1010"), 10);
}

#[test]
fn binary_literal_8_bit() {
    assert_eq!(eval_int("0b11111111"), 255);
}

#[test]
fn octal_literal_0o() {
    let code = r#"
        my $r = 0o17;
        $r == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn underscore_separator_in_hex() {
    let code = r#"
        my $r = 0xFF_FF;
        $r == 0xFFFF ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn underscore_separator_in_binary() {
    let code = r#"
        my $r = 0b1111_0000;
        $r == 240 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sprintf hex output ────────────────────────────────────────────

#[test]
fn sprintf_x_emits_lowercase() {
    let code = r#"
        sprintf("%x", 255) eq "ff" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_x_uppercase() {
    let code = r#"
        sprintf("%X", 255) eq "FF" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_x_zero_pad_width() {
    let code = r#"
        sprintf("%08x", 255) eq "000000ff" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_x_with_alt_prefix() {
    let code = r#"
        # "%#x" should produce "0xff" prefix.
        sprintf("%#x", 255) eq "0xff" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sprintf octal output ──────────────────────────────────────────

#[test]
fn sprintf_o_octal() {
    let code = r#"
        sprintf("%o", 8) eq "10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_o_unix_mode() {
    let code = r#"
        sprintf("%o", 0755) eq "755" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sprintf binary output ─────────────────────────────────────────

#[test]
fn sprintf_b_binary() {
    let code = r#"
        sprintf("%b", 10) eq "1010" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_b_zero_pad() {
    let code = r#"
        sprintf("%08b", 10) eq "00001010" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── hex() function ────────────────────────────────────────────────

#[test]
fn hex_function_decodes_string() {
    let code = r#"
        hex("FF") == 255 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hex_with_0x_prefix() {
    let code = r#"
        hex("0xABCD") == 0xABCD ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── oct() function (handles 0x, 0b, 0o, and bare octal) ───────────

#[test]
fn oct_decodes_bare_octal() {
    let code = r#"
        oct("17") == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn oct_decodes_0o_prefix() {
    let code = r#"
        oct("0o17") == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn oct_decodes_0x_prefix() {
    let code = r#"
        oct("0xFF") == 255 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn oct_decodes_0b_prefix() {
    let code = r#"
        oct("0b1010") == 10 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Bitwise operations on hex ─────────────────────────────────────

#[test]
fn bitwise_and_on_hex() {
    let code = r#"
        (0xF0F0 & 0x0FF0) == 0x00F0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bitwise_or_on_hex() {
    let code = r#"
        (0xF000 | 0x000F) == 0xF00F ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bitwise_xor_on_hex() {
    let code = r#"
        (0xFFFF ^ 0x0FF0) == 0xF00F ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Shift operations ───────────────────────────────────────────────

#[test]
fn left_shift_by_8() {
    let code = r#"
        (1 << 8) == 256 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn right_shift_by_4() {
    let code = r#"
        (0xFF >> 4) == 0x0F ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn shift_then_or_with_overlapping_bits() {
    // 0xFF00 >> 8 = 0xFF. 0xFF | 0xFF = 0xFF (same bits).
    let code = r#"
        my $high = (0xFF00 >> 8);
        my $low = 0x00FF;
        ($high | $low) == 0xFF ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn shift_then_or_with_disjoint_bits() {
    // 0xFF00 already has high byte set; 0x000F has low nibble.
    let code = r#"
        (0xFF00 | 0x000F) == 0xFF0F ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Large hex literal ──────────────────────────────────────────────

#[test]
fn large_hex_literal() {
    let code = r#"
        0xFFFFFFFF == 4294967295 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Bit-flag patterns ─────────────────────────────────────────────

#[test]
fn bit_flag_check_pattern() {
    let code = r#"
        my $flags = 0b1010;
        my $check = (1 << 1);   # bit 1 set
        ($flags & $check) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bit_flag_clear_pattern() {
    let code = r#"
        my $flags = 0xFF;
        my $cleared = $flags & ~(1 << 3);   # clear bit 3
        $cleared == 0xF7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Round-trip via sprintf + hex ───────────────────────────────────

#[test]
fn sprintf_hex_then_hex_roundtrip() {
    let code = r#"
        my $orig = 0xDEADBEEF;
        my $s = sprintf("%x", $orig);
        my $back = hex($s);
        $back == $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
