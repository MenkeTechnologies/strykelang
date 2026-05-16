//! Math builtin pins: abs, sqrt, exp, log, trig, floor/ceil/round, int.

use crate::common::*;

// ── abs ──────────────────────────────────────────────────────────────

#[test]
fn abs_positive_unchanged() {
    assert_eq!(eval_int("abs(42)"), 42);
}

#[test]
fn abs_negative_flipped() {
    assert_eq!(eval_int("abs(-42)"), 42);
}

#[test]
fn abs_zero() {
    assert_eq!(eval_int("abs(0)"), 0);
}

#[test]
fn abs_float() {
    let code = r#"
        abs(abs(-3.14) - 3.14) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sqrt ─────────────────────────────────────────────────────────────

#[test]
fn sqrt_perfect_square() {
    let code = r#"
        sqrt(144) == 12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sqrt_two_approximation() {
    let code = r#"
        abs(sqrt(2) - 1.4142135) < 1e-6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sqrt_zero() {
    let code = r#"
        sqrt(0) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── exp + log ────────────────────────────────────────────────────────

#[test]
fn exp_of_zero_is_one() {
    let code = r#"
        exp(0) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn exp_of_one_is_e() {
    let code = r#"
        abs(exp(1) - 2.71828) < 1e-4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn log_of_e_is_one() {
    let code = r#"
        abs(log(exp(1)) - 1) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn log_of_one_is_zero() {
    let code = r#"
        log(1) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn exp_log_roundtrip() {
    let code = r#"
        my $x = 5;
        abs(exp(log($x)) - $x) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Trig ─────────────────────────────────────────────────────────────

#[test]
fn sin_of_zero_is_zero() {
    let code = r#"
        abs(sin(0)) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cos_of_zero_is_one() {
    let code = r#"
        abs(cos(0) - 1) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sin_of_pi_approx_zero() {
    let code = r#"
        my $pi = 3.14159265358979;
        abs(sin($pi)) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cos_of_pi_is_minus_one() {
    let code = r#"
        my $pi = 3.14159265358979;
        abs(cos($pi) - (-1)) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tan_of_zero_is_zero() {
    let code = r#"
        abs(tan(0)) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pythagorean_identity_sin_sq_plus_cos_sq_eq_one() {
    let code = r#"
        my $x = 1.5;
        abs(sin($x) * sin($x) + cos($x) * cos($x) - 1) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── atan2 ────────────────────────────────────────────────────────────

#[test]
fn atan2_of_zero_zero_handles() {
    let code = r#"
        my $r = atan2(0, 1);
        abs($r) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn atan2_returns_pi_over_2_for_positive_y() {
    let code = r#"
        my $pi = 3.14159265358979;
        abs(atan2(1, 0) - $pi / 2) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── floor / ceil / round ─────────────────────────────────────────────

#[test]
fn floor_rounds_down() {
    let code = r#"
        floor(3.7) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn floor_of_negative_rounds_more_negative() {
    let code = r#"
        floor(-3.2) == -4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ceil_rounds_up() {
    let code = r#"
        ceil(3.2) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ceil_of_integer_unchanged() {
    let code = r#"
        ceil(5) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn round_to_nearest() {
    let code = r#"
        my $r = round(3.5);
        # Either 3 (banker's) or 4 (away-from-zero).
        ($r == 3 || $r == 4) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn round_three_point_seven() {
    let code = r#"
        round(3.7) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── int truncation ───────────────────────────────────────────────────

#[test]
fn int_truncates_toward_zero() {
    let code = r#"
        int(3.7) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn int_truncates_negative_toward_zero() {
    let code = r#"
        int(-3.7) == -3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ** exponent ──────────────────────────────────────────────────────

#[test]
fn exponent_squares_correctly() {
    let code = r#"
        (5 ** 2) == 25 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn exponent_cubes_correctly() {
    let code = r#"
        (2 ** 10) == 1024 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_exponent_yields_fraction() {
    let code = r#"
        abs(2 ** -2 - 0.25) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── max / min ────────────────────────────────────────────────────────

#[test]
fn max_of_floats() {
    let code = r#"
        max(1.5, 2.5, 0.5, 3.5) == 3.5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn min_of_floats() {
    let code = r#"
        min(1.5, 2.5, 0.5, 3.5) == 0.5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Combination: math composition ────────────────────────────────────

#[test]
fn distance_formula_via_sqrt_pow() {
    let code = r#"
        my $dx = 3.0;
        my $dy = 4.0;
        my $dist = sqrt($dx * $dx + $dy * $dy);
        abs($dist - 5.0) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn log_of_product_equals_sum_of_logs() {
    let code = r#"
        my $a = 5;
        my $b = 7;
        abs(log($a * $b) - (log($a) + log($b))) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
