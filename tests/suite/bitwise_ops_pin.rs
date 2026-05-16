//! Bitwise-operator pins: `&`, `|`, `^`, `~`, `<<`, `>>`.

use crate::common::*;

// ── AND ────────────────────────────────────────────────────────────

#[test]
fn band_basic() {
    assert_eq!(eval_int(r#"(5 & 3) == 1 ? 1 : 0"#), 1);
}

#[test]
fn band_with_zero_is_zero() {
    assert_eq!(eval_int(r#"(0xff & 0) == 0 ? 1 : 0"#), 1);
}

#[test]
fn band_mask_low_nibble() {
    assert_eq!(eval_int(r#"(0xff & 0x0f) == 15 ? 1 : 0"#), 1);
}

#[test]
fn band_identical_value() {
    assert_eq!(eval_int(r#"(0xa5 & 0xa5) == 0xa5 ? 1 : 0"#), 1);
}

// ── OR ─────────────────────────────────────────────────────────────

#[test]
fn bor_basic() {
    assert_eq!(eval_int(r#"(5 | 3) == 7 ? 1 : 0"#), 1);
}

#[test]
fn bor_with_zero_is_self() {
    assert_eq!(eval_int(r#"(0xa5 | 0) == 0xa5 ? 1 : 0"#), 1);
}

#[test]
fn bor_sets_flag_bit() {
    assert_eq!(eval_int(r#"(0x10 | 0x02) == 0x12 ? 1 : 0"#), 1);
}

// ── XOR ────────────────────────────────────────────────────────────

#[test]
fn bxor_basic() {
    assert_eq!(eval_int(r#"(5 ^ 3) == 6 ? 1 : 0"#), 1);
}

#[test]
fn bxor_with_self_is_zero() {
    assert_eq!(eval_int(r#"(0x55 ^ 0x55) == 0 ? 1 : 0"#), 1);
}

#[test]
fn bxor_with_zero_is_self() {
    assert_eq!(eval_int(r#"(0xa5 ^ 0) == 0xa5 ? 1 : 0"#), 1);
}

#[test]
fn bxor_toggle_bit() {
    let code = r#"
        my $x = 0b1010;
        $x = $x ^ 0b0001;   # set low bit
        $x == 0b1011 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bxor_swap_idiom() {
    let code = r#"
        my $a_val = 5;
        my $b_val = 9;
        $a_val ^= $b_val;
        $b_val ^= $a_val;
        $a_val ^= $b_val;
        ($a_val == 9 && $b_val == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── NOT (~) ────────────────────────────────────────────────────────

#[test]
fn bnot_zero_is_minus_one() {
    // Signed i64 ~0 == -1.
    assert_eq!(eval_int(r#"(~0) == -1 ? 1 : 0"#), 1);
}

#[test]
fn bnot_five_is_minus_six() {
    assert_eq!(eval_int(r#"(~5) == -6 ? 1 : 0"#), 1);
}

#[test]
fn bnot_double_is_identity() {
    assert_eq!(eval_int(r#"~~42 == 42 ? 1 : 0"#), 1);
}

// ── shift left ────────────────────────────────────────────────────

#[test]
fn shl_small() {
    assert_eq!(eval_int(r#"(1 << 4) == 16 ? 1 : 0"#), 1);
}

#[test]
fn shl_to_32_bit_boundary() {
    assert_eq!(eval_int(r#"(1 << 32) == 4294967296 ? 1 : 0"#), 1);
}

#[test]
fn shl_to_sign_bit() {
    // 1 << 63 = i64::MIN. The literal -9_223_372_036_854_775_808
    // overflows the i64 lexer; compute via shifts instead.
    let code = r#"
        my $m = 1 << 63;
        my $expected = -(1 << 62) - (1 << 62);
        $m == $expected ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn shl_zero_is_self() {
    assert_eq!(eval_int(r#"(42 << 0) == 42 ? 1 : 0"#), 1);
}

#[test]
fn shl_by_one_is_double() {
    assert_eq!(eval_int(r#"(13 << 1) == 26 ? 1 : 0"#), 1);
}

// ── shift right ───────────────────────────────────────────────────

#[test]
fn shr_small() {
    assert_eq!(eval_int(r#"(256 >> 2) == 64 ? 1 : 0"#), 1);
}

#[test]
fn shr_to_zero() {
    assert_eq!(eval_int(r#"(1 >> 1) == 0 ? 1 : 0"#), 1);
}

#[test]
fn shr_arithmetic_on_negative() {
    // -16 >> 2 should yield -4 (sign-extending shift).
    assert_eq!(eval_int(r#"(-16 >> 2) == -4 ? 1 : 0"#), 1);
}

#[test]
fn shr_neg_one_stays_neg_one() {
    // -1 >> any amount in arithmetic shift stays -1.
    assert_eq!(eval_int(r#"(-1 >> 4) == -1 ? 1 : 0"#), 1);
}

// ── compound assignment ──────────────────────────────────────────

#[test]
fn band_assign_compound() {
    let code = r#"
        my $x = 0xff;
        $x &= 0x0f;
        $x == 0x0f ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bor_assign_compound() {
    let code = r#"
        my $x = 0;
        $x |= 0x10;
        $x |= 0x02;
        $x == 0x12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bxor_assign_compound() {
    let code = r#"
        my $x = 0b1010;
        $x ^= 0b0101;
        $x == 0b1111 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn shl_assign_compound() {
    let code = r#"
        my $x = 1;
        $x <<= 8;
        $x == 256 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn shr_assign_compound() {
    let code = r#"
        my $x = 1024;
        $x >>= 5;
        $x == 32 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── operator precedence ──────────────────────────────────────────

#[test]
fn precedence_or_lower_than_and() {
    let code = r#"
        # & binds tighter than |.
        # 0b1100 | (0b1010 & 0b0011) = 0b1100 | 0b0010 = 0b1110
        (0b1100 | 0b1010 & 0b0011) == 0b1110 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn precedence_xor_lower_than_and() {
    let code = r#"
        # & binds tighter than ^.
        # 0b1100 ^ (0b1010 & 0b0011) = 0b1100 ^ 0b0010 = 0b1110
        (0b1100 ^ 0b1010 & 0b0011) == 0b1110 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn precedence_shift_below_arithmetic() {
    let code = r#"
        # << has lower prec than +.
        # (1 + 2) << 3 = 3 << 3 = 24
        # 1 + (2 << 3) = 1 + 16 = 17
        (1 + 2 << 3) == 24 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── derived idioms ────────────────────────────────────────────────

#[test]
fn is_power_of_two_via_band() {
    let code = r#"
        fn Demo::BW::is_pow2($n) { $n > 0 && (($n & ($n - 1)) == 0) ? 1 : 0 }
        my @check = map { Demo::BW::is_pow2($_) } (1, 2, 3, 4, 5, 6, 7, 8, 16, 17);
        join(",", @check) eq "1,1,0,1,0,0,0,1,1,0" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn count_set_bits_via_kernighan() {
    let code = r#"
        fn Demo::BW::popcount($n) {
            my $count = 0;
            while ($n != 0) {
                $n = $n & ($n - 1);
                $count++;
            }
            $count
        }
        my @r = map { Demo::BW::popcount($_) } (0, 1, 3, 7, 15, 0xff, 0xa5);
        join(",", @r) eq "0,1,2,3,4,8,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rgb_pack_via_shift_or() {
    let code = r#"
        my $r = 0xff;
        my $g = 0x88;
        my $b = 0x00;
        my $rgb = ($r << 16) | ($g << 8) | $b;
        $rgb == 0xff8800 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rgb_unpack_via_shift_and() {
    let code = r#"
        my $rgb = 0xff8800;
        my $r = ($rgb >> 16) & 0xff;
        my $g = ($rgb >> 8)  & 0xff;
        my $b = $rgb         & 0xff;
        ($r == 0xff && $g == 0x88 && $b == 0x00) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn even_odd_check_via_low_bit() {
    let code = r#"
        my @r = map { ($_ & 1) == 0 ? "even" : "odd" } (1, 2, 3, 4, 5);
        join(",", @r) eq "odd,even,odd,even,odd" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn set_bit_at_position() {
    let code = r#"
        my $flags = 0;
        $flags |= (1 << 3);
        $flags |= (1 << 5);
        ($flags == 0b101000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn clear_bit_at_position() {
    let code = r#"
        my $flags = 0xff;
        $flags &= ~(1 << 4);
        $flags == 0b11101111 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn test_bit_at_position() {
    // 0b10110 = 22 = bits 1, 2, 4 (LSB=bit 0).
    let code = r#"
        my $flags = 0b10110;
        my $has_bit_2 = ($flags & (1 << 2)) ? 1 : 0;   # bit 2 set
        my $has_bit_3 = ($flags & (1 << 3)) ? 1 : 0;   # bit 3 NOT set
        my $has_bit_4 = ($flags & (1 << 4)) ? 1 : 0;   # bit 4 set
        my $has_bit_0 = ($flags & (1 << 0)) ? 1 : 0;   # bit 0 NOT set
        ($has_bit_2 == 1 && $has_bit_3 == 0 && $has_bit_4 == 1 && $has_bit_0 == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
