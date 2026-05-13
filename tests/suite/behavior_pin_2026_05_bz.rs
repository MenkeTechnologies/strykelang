//! Behavior-pinning batch BZ (2026-05-09): inequality / entropy / tree-split helpers (`gini_coefficient`,
//! `theil_index`, `atkinson_index`, `information_gain`), plus regressions documenting **BUG-126** —
//! builtins whose bodies read only **`args.first()` / `args[0]`** so comma-separated tails are silently
//! dropped (`joint_entropy_step`, `herfindahl_hirschman` / `hhi`, `gini_impurity`, `entropy_bits`,
//! `log_sum_exp` / `lse`).

use crate::common::*;

#[test]
fn doubling_time_decimal_growth_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", doubling_time(0.1))"#),
        "6.931471805599"
    );
}

#[test]
fn joint_entropy_four_uniform_coin_bits_array_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", joint_entropy_step([0.25, 0.25, 0.25, 0.25]))"#),
        "2.000000000000"
    );
}

// BUG-126: only first flattened argument — variadic probs are truncated.
#[test]
fn joint_entropy_variadic_trailing_probs_ignored_tail_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", joint_entropy_step(0.25, 0.25, 0.25, 0.25))"#),
        "0.500000000000"
    );
}

#[test]
fn herfindahl_hirschman_normalized_quarter_shares_array_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", herfindahl_hirschman([0.25, 0.25, 0.25, 0.25]))"#),
        "0.250000000000"
    );
}

#[test]
fn hhi_variadic_trailing_shares_use_first_squared_only_tail_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", hhi(0.25, 0.25, 0.25, 0.25))"#),
        "0.250000000000"
    );
}

#[test]
fn gini_impurity_three_class_normalized_array_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", gini_impurity([0.2, 0.3, 0.5]))"#),
        "0.620000000000"
    );
}

#[test]
fn gini_impurity_variadic_first_probability_only_tail_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", gini_impurity(0.2, 0.3, 0.5))"#),
        "0.000000000000"
    );
}

#[test]
fn entropy_bits_four_coin_array_equals_two_tail_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", entropy_bits([0.25, 0.25, 0.25, 0.25]))"#),
        "2.000000000000"
    );
}

#[test]
fn entropy_bits_variadic_degenerate_after_truncation_tail_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", 0 + entropy_bits(0.25, 0.25, 0.25, 0.25))"#),
        "0.000000000000"
    );
}

#[test]
fn log_sum_exp_array_maximum_dominated_stable_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", log_sum_exp([1, 2, 100]))"#),
        "100.000000000000"
    );
}

#[test]
fn log_sum_exp_variadic_first_scalar_only_tail_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lse(1, 2, 100))"#),
        "1.000000000000"
    );
}

#[test]
fn theil_index_incomes_array_four_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", theil_index([1, 2, 9, 10]))"#),
        "0.303759475356"
    );
}

#[test]
fn atkinson_index_normalized_incomes_eps_half_tail_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", atkinson_index([10, 20, 90], 0.5))"#),
        "0.185730319473"
    );
}

#[test]
fn gini_coeff_pair_spread_four_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", gini_coefficient([1, 4]))"#),
        "0.300000000000"
    );
}

#[test]
fn information_gain_parent_child_splits_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", information_gain([10, 10, 10], [5, 15], [15, 5]))"#),
        "0.503258334776"
    );
}

#[test]
fn gain_ratio_positive_split_information_bz() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", gain_ratio([10, 10, 10], [20], [10]))"#),
        "1.725982457879"
    );
}
