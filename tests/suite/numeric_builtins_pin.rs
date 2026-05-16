//! Numeric/statistical builtin pins.

use crate::common::*;

// ── median ─────────────────────────────────────────────────────────

#[test]
fn median_odd_count() {
    let code = r#"
        median(1, 2, 3, 4, 5) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn median_even_count() {
    let code = r#"
        # Median of 1..6 = (3 + 4) / 2 = 3.5.
        my $m = median(1, 2, 3, 4, 5, 6);
        abs($m - 3.5) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn median_unsorted_input() {
    let code = r#"
        median(5, 2, 8, 1, 9) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn median_single_element() {
    let code = r#"
        median(42) == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── variance ──────────────────────────────────────────────────────

#[test]
fn variance_of_simple_list() {
    let code = r#"
        # variance(1..5) = 2 (population variance).
        abs(variance(1, 2, 3, 4, 5) - 2) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn variance_of_constant_is_zero() {
    let code = r#"
        variance(5, 5, 5, 5, 5) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── stddev ────────────────────────────────────────────────────────

#[test]
fn stddev_of_simple_list() {
    let code = r#"
        # sqrt(variance(1..5)) = sqrt(2) ≈ 1.414.
        abs(stddev(1, 2, 3, 4, 5) - sqrt(2)) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn stddev_of_constant_is_zero() {
    let code = r#"
        stddev(10, 10, 10) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sum/product ────────────────────────────────────────────────────

#[test]
fn sum_of_ten_integers() {
    let code = r#"
        sum(1, 2, 3, 4, 5, 6, 7, 8, 9, 10) == 55 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn product_of_factorial_inputs() {
    let code = r#"
        product(1, 2, 3, 4, 5, 6) == 720 ? 1 : 0   # 6!
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── avg ───────────────────────────────────────────────────────────

#[test]
fn avg_of_integers() {
    let code = r#"
        avg(2, 4, 6, 8, 10) == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn avg_of_negatives_balanced_to_zero() {
    let code = r#"
        avg(-3, -1, 1, 3) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── min/max with floats ────────────────────────────────────────────

#[test]
fn min_with_floats() {
    let code = r#"
        abs(min(1.5, 2.5, 0.5, 3.5) - 0.5) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn max_with_floats() {
    let code = r#"
        abs(max(1.5, 2.5, 0.5, 3.5) - 3.5) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── min/max with single value ──────────────────────────────────────

#[test]
fn min_of_one_element() {
    let code = r#"
        min(99) == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn max_of_one_element() {
    let code = r#"
        max(-7) == -7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ceil / floor combined ──────────────────────────────────────────

#[test]
fn floor_truncates_down() {
    let code = r#"
        floor(3.9) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ceil_rounds_up() {
    let code = r#"
        ceil(3.1) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn floor_negative_rounds_away_from_zero() {
    let code = r#"
        floor(-3.1) == -4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ceil_negative_rounds_toward_zero() {
    let code = r#"
        ceil(-3.9) == -3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sqrt edge cases ────────────────────────────────────────────────

#[test]
fn sqrt_one_is_one() {
    let code = r#"
        sqrt(1) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sqrt_zero_is_zero() {
    let code = r#"
        sqrt(0) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sqrt_large_perfect_square() {
    let code = r#"
        sqrt(1000000) == 1000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── log / exp inverse ──────────────────────────────────────────────

#[test]
fn log_then_exp_roundtrip() {
    let code = r#"
        my $x = 7.5;
        abs(exp(log($x)) - $x) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn log_of_one_is_zero_exact() {
    let code = r#"
        log(1) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── abs handles edges ──────────────────────────────────────────────

#[test]
fn abs_of_min_float() {
    let code = r#"
        abs(-3.14) == 3.14 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── median of single + odd/even count distinction ────────────────

#[test]
fn median_of_two_elements_averages() {
    let code = r#"
        abs(median(10, 20) - 15) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Combination: sum then divide for mean ─────────────────────────

#[test]
fn manual_mean_equals_avg_builtin() {
    let code = r#"
        my @input = (3, 7, 8, 12, 25);
        my $manual = sum(@input) / len(@input);
        my $built  = avg(@input);
        abs($manual - $built) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── stddev of distribution with known stddev ──────────────────────

#[test]
fn stddev_of_symmetric_pair() {
    let code = r#"
        # variance(-1, 1) = ((−1)^2 + 1^2) / 2 = 1. stddev = 1.
        abs(stddev(-1, 1) - 1) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sum + product on negatives ────────────────────────────────────

#[test]
fn sum_of_negatives() {
    let code = r#"
        sum(-1, -2, -3, -4, -5) == -15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn product_with_negative() {
    let code = r#"
        product(2, -3, 4) == -24 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range-derived sums ────────────────────────────────────────────

#[test]
fn sum_one_to_100() {
    let code = r#"
        sum(1:100) == 5050 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn product_one_to_six_is_factorial() {
    let code = r#"
        product(1:6) == 720 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── median for outliers ──────────────────────────────────────────

#[test]
fn median_robust_to_outliers() {
    let code = r#"
        # avg is dragged by 1000; median is just the middle.
        my @input = (1, 2, 3, 4, 1000);
        my $a = avg(@input);
        my $m = median(@input);
        ($m == 3 && $a > 200) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
