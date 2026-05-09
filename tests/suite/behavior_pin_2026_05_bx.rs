//! Behavior-pinning batch BX (2026-05-09): information measures (`kl_divergence`, `js_divergence`,
//! `jensen_shannon_div`, `mutual_information`, `cross_entropy_arr`), `sum_squares` / `hellinger_kernel`,
//! and regression guards for `cosine_distance` / `median_absolute_deviation` edge cases documented in
//! `docs/BUGS.md`.

use crate::common::*;

#[test]
fn kl_divergence_identical_distributions_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", kl_divergence([0.5, 0.5], [0.5, 0.5]))"#),
        "0.000000000000"
    );
}

#[test]
fn kl_divergence_biased_vs_uniform_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", kl_div([0.75, 0.25], [0.5, 0.5]))"#),
        "0.130812035941"
    );
}

#[test]
fn js_divergence_identical_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", js_divergence([0.5, 0.5], [0.5, 0.5]))"#),
        "0.000000000000"
    );
}

// `jensen_shannon_div` → `kullback_jensen_div` (log2); see BUG-122.
#[test]
fn jensen_shannon_div_triple_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", jensen_shannon_div([0.1, 0.2, 0.7], [0.2, 0.3, 0.5]))"#),
        "0.031596722287"
    );
}

// `js_divergence` uses ln in KL; differs from `jensen_shannon_div` (log2).
#[test]
fn js_divergence_triple_nats_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", js_div([0.1, 0.2, 0.7], [0.2, 0.3, 0.5]))"#),
        "0.021901178968"
    );
}

#[test]
fn mutual_information_independent_uniform_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", mutual_information([[0.25, 0.25], [0.25, 0.25]]))"#),
        "0.0000000000"
    );
}

#[test]
fn mutual_information_perfect_correlation_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", mutual_information([[0.5, 0], [0, 0.5]]))"#),
        "0.6931471806"
    );
}

#[test]
fn cross_entropy_arr_two_bin_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", cross_entropy_arr([0.25, 0.75], [0.75, 0.25]))"#),
        "1.111641288953"
    );
}

#[test]
fn sum_squares_pair_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", sum_squares(3, 4))"#),
        "25.000000"
    );
}

#[test]
fn hellinger_kernel_asymmetric_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", hellinger_kernel([0.25, 0.75], [0.75, 0.25]))"#),
        "0.764946645195"
    );
}

// Zero norm → distance 1 (BUG-120).
#[test]
fn cosine_distance_zero_operand_is_unit_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", cosine_distance([0, 0], [1, 2]))"#),
        "1.0000000000"
    );
}

// Even-length sample uses sorted[n/2] as center, not mean of middles (BUG-121).
#[test]
fn median_absolute_deviation_even_n_spread_bx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", median_absolute_deviation(1, 2, 100, 101))"#),
        "98.0000000000"
    );
}
