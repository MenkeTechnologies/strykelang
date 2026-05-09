//! Behavior-pinning batch BY (2026-05-09): distribution distances (`total_variation_distance`,
//! `bhattacharyya_coefficient`, `hellinger_distance_step`, `wasserstein_dist_emp`, `chisquare_metric`),
//! Sinkhorn iteration, Rényi / Csiszá-style helpers, plus explicit numeric links between Chi-family
//! builtins and KL scaling (`relative_entropy_kl` vs `kl_div`) documented in `docs/BUGS.md`.

use crate::common::*;

#[test]
fn total_variation_distance_three_way_by() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.12f", total_variation_distance([0.2, 0.3, 0.5], [0.25, 0.25, 0.5]))"#
        ),
        "0.050000000000"
    );
}

#[test]
fn bhattacharyya_coefficient_balanced_coin_by() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", bhattacharyya_coefficient([0.25, 0.25], [0.25, 0.25]))"#),
        "0.500000000000"
    );
}

#[test]
fn wasserstein_dist_emp_equal_cardinality_by() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", wasserstein_dist_emp([3, 1, 2], [0, 4, 2]))"#),
        "0.666666666667"
    );
}

#[test]
fn hellinger_distance_mixed_coin_by() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", hellinger_distance_step([0.25, 0.75], [0.75, 0.25]))"#),
        "0.366025403784"
    );
}

#[test]
fn chisquare_metric_axis_pair_by() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", chisquare_metric([1, 2], [4, 8]))"#),
        "5.400000000000"
    );
}

// Linear relation χ²_metric = 2 * chi_squared_distance on same operands (BUG-123).
#[test]
fn chisquare_metric_equals_twice_chi_squared_distance_by() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10f", chisquare_metric([1, 2], [4, 8]) / chi_squared_distance([1, 2], [4, 8]))"#
        ),
        "2.0000000000"
    );
}

#[test]
fn sinkhorn_iteration_step_division_by() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", sinkhorn_iteration_step(4, 2))"#),
        "2.000000000000"
    );
}

// Implementation yields sum qᵢ ln(pᵢ/qᵢ) = −KL(Q‖P); see BUG-124.
#[test]
fn csiszar_phi_div_coin_pair_by() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", csiszar_phi_div([0.5, 0.5], [0.25, 0.75]))"#),
        "-0.130812035941"
    );
}

#[test]
fn renyi_divergence_half_alpha_by() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", renyi_divergence_step(0.5, [0.5, 0.5], [0.25, 0.75]))"#),
        "0.069336464195"
    );
}

#[test]
fn relative_entropy_kl_uses_bits_by() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", relative_entropy_kl([0.75, 0.25], [0.5, 0.5]))"#),
        "0.188721875541"
    );
}

// relative_entropy_kl is log2 KL; multiply by ln 2 ≡ kl_div nats (BUG-125).
#[test]
fn relative_entropy_kl_times_ln2_matches_kl_div_by() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", relative_entropy_kl([0.55, 0.45], [0.4, 0.6]) * log(2))"#),
        eval_string(r#"sprintf("%.12f", kl_div([0.55, 0.45], [0.4, 0.6]))"#)
    );
}
