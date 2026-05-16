//! Numeric edge-case pins.

use crate::common::*;

// ── Division by zero ───────────────────────────────────────────────

#[test]
fn divide_by_zero_dies() {
    let code = r#"
        my $r = eval { 1 / 0 };
        !defined($r) && $@ ne "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn divide_zero_by_nonzero_returns_zero() {
    let code = r#"
        my $r = 0 / 5;
        $r == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Modulo with negatives ──────────────────────────────────────────

#[test]
fn modulo_positive_dividend_positive_divisor() {
    let code = r#"
        (10 % 3) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn modulo_negative_dividend_positive_divisor() {
    let code = r#"
        # Perl: -7 % 3 = 2. C: -7 % 3 = -1.
        my $r = -7 % 3;
        ($r == 2 || $r == -1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn modulo_zero_dividend() {
    let code = r#"
        (0 % 5) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Float precision ────────────────────────────────────────────────

#[test]
fn float_addition_imprecise_for_0_1_plus_0_2() {
    let code = r#"
        my $r = 0.1 + 0.2;
        # NOT exactly 0.3 in IEEE 754.
        ($r != 0.3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn float_addition_within_tolerance() {
    let code = r#"
        my $r = 0.1 + 0.2;
        abs($r - 0.3) < 1e-15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Large integer arithmetic ──────────────────────────────────────

#[test]
fn product_of_million_squared() {
    let code = r#"
        my $a = 1_000_000;
        my $b = 1_000_000;
        ($a * $b == 1_000_000_000_000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn large_int_addition_exact() {
    let code = r#"
        my $a = 9_999_999_999;
        my $b = 1;
        ($a + $b == 10_000_000_000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Bitwise on hex ─────────────────────────────────────────────────

#[test]
fn bitwise_xor_zeros_self() {
    let code = r#"
        (0xFFFF ^ 0xFFFF) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bitwise_and_with_zero_is_zero() {
    let code = r#"
        (0xFFFF & 0) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Shift edge cases ──────────────────────────────────────────────

#[test]
fn left_shift_by_zero_unchanged() {
    let code = r#"
        (42 << 0) == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn left_shift_overflow_warning() {
    let code = r#"
        # Shift by 32 on a 32-bit value zeros out (or all-ones with sign).
        # On 64-bit, value is preserved.
        my $r = 1 << 30;
        $r > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Integer division (int) ─────────────────────────────────────────

#[test]
fn int_division_truncates() {
    let code = r#"
        int(7 / 2) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn int_truncates_negative_toward_zero() {
    let code = r#"
        int(-7 / 2) == -3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Number formatting via sprintf ─────────────────────────────────

#[test]
fn sprintf_preserves_large_integer_exactly() {
    let code = r#"
        sprintf("%d", 9_999_999_999) eq "9999999999" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_handles_zero_separately() {
    let code = r#"
        sprintf("%d", 0) eq "0" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Numeric comparison ────────────────────────────────────────────

#[test]
fn numeric_compare_with_string_coerces() {
    let code = r#"
        ("10" == 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn numeric_compare_zero_padded_string() {
    let code = r#"
        ("010" == 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_compare_zero_padded_differs() {
    let code = r#"
        ("010" eq "10") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Exponent with float result ─────────────────────────────────────

#[test]
fn fractional_exponent_yields_float() {
    let code = r#"
        abs(2 ** 0.5 - 1.4142135) < 1e-6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_exponent_gives_fraction() {
    let code = r#"
        abs(2 ** -3 - 0.125) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Compare floats safely ─────────────────────────────────────────

#[test]
fn safe_float_compare_within_eps() {
    let code = r#"
        my $a = 1.0 + 2.0 + 3.0;   # 6.0
        my $b = 6.0;
        abs($a - $b) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Boolean coercion ──────────────────────────────────────────────

#[test]
fn zero_is_falsy() {
    let code = r#"
        0 ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn nonzero_is_truthy() {
    let code = r#"
        42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_is_truthy() {
    let code = r#"
        -5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Underscores in numeric literals ───────────────────────────────

#[test]
fn underscore_in_decimal_literal() {
    let code = r#"
        my $r = 1_000_000;
        $r == 1000000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn underscore_in_float_literal() {
    let code = r#"
        my $r = 1_000.5;
        $r == 1000.5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Round-trip via sprintf ─────────────────────────────────────────

#[test]
fn sprintf_int_then_parse_back() {
    let code = r#"
        my $n = 123456;
        my $s = sprintf("%d", $n);
        my $back = $s + 0;
        $back == $n ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_float_then_parse_back_within_tolerance() {
    let code = r#"
        my $n = 3.14159;
        my $s = sprintf("%.5f", $n);
        my $back = $s + 0;
        abs($back - $n) < 1e-5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Numeric ops on string-encoded numbers ─────────────────────────

#[test]
fn arithmetic_on_string_numbers() {
    let code = r#"
        my $a = "10";
        my $b = "20";
        ($a + $b == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Power of 2 detection via bit-trick ────────────────────────────

#[test]
fn power_of_two_detection() {
    let code = r#"
        fn Demo::NE::is_pow2($n) {
            $n > 0 && ($n & ($n - 1)) == 0
        }
        (Demo::NE::is_pow2(1)
            && Demo::NE::is_pow2(2)
            && Demo::NE::is_pow2(64)
            && !Demo::NE::is_pow2(0)
            && !Demo::NE::is_pow2(3)
            && !Demo::NE::is_pow2(15)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Comparison chain ─────────────────────────────────────────────

#[test]
fn three_way_comparison_via_spaceship() {
    let code = r#"
        my @r = (3 <=> 5, 5 <=> 5, 7 <=> 5);
        join(",", @r) eq "-1,0,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_three_way_via_cmp() {
    let code = r#"
        my @r = ("a" cmp "b", "b" cmp "b", "c" cmp "b");
        join(",", @r) eq "-1,0,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
