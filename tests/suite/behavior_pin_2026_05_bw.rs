//! Behavior-pinning batch BW (2026-05-09): vector-distance variants (`cosine_distance`, `minkowski_*`,
//! `canberra_*`, `bray_curtis*`, `chi_squared_distance`), planar `chebyshev_distance`, supervised error
//! metrics (`mean_absolute_error`, `mean_squared_error`, `rmse`), dispersion (`rms`, `median_absolute_deviation`),
//! and `winsorize` percentile clamp idiom output.

use crate::common::*;

#[test]
fn chebyshev_distance_square_diagonal_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", chebyshev_distance(0, 0, 3, 4))"#),
        "4.000000"
    );
}

#[test]
fn minkowski_equals_euclidean_when_p2_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", minkowski_distance([0, 0], [3, 4], 2))"#),
        "5.000000"
    );
}

#[test]
fn minkowski_equals_manhattan_when_p1_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", minkowski_distance([1, 2], [4, 6], 1))"#),
        "7.000000"
    );
}

#[test]
fn canberra_distance_ratio_sum_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", canberra_distance([1, 2], [4, 8]))"#),
        "1.200000"
    );
}

#[test]
fn bray_curtis_distance_scaled_l1_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", bray_curtis_distance([1, 2], [4, 8]))"#),
        "0.600000"
    );
}

#[test]
fn cosine_distance_parallel_near_zero_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", cosine_distance([1, 2], [2, 4]))"#),
        "0.0000"
    );
}

#[test]
fn cosine_distance_orthogonal_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", cosine_distance([1, 0], [0, 1]))"#),
        "1.0000000000"
    );
}

#[test]
fn chi_squared_distance_two_dims_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", chi_squared_distance([1, 2], [4, 8]))"#),
        "2.700000"
    );
}

#[test]
fn mean_absolute_error_two_points_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", mean_absolute_error([0, 10], [0, 20]))"#),
        "5.000000"
    );
}

#[test]
fn mean_squared_error_two_points_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", mean_squared_error([0, 2], [0, 6]))"#),
        "8.000000"
    );
}

#[test]
fn rmse_aligned_with_mse_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", rmse([0, 2], [0, 10]))"#),
        "5.6568542495"
    );
}

#[test]
fn median_absolute_deviation_sorted_quartet_bw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", median_absolute_deviation(1, 2, 9, 10))"#),
        "7.0000000000"
    );
}

#[test]
fn rms_of_scalar_pair_bw() {
    assert_eq!(eval_string(r#"sprintf("%.6f", rms(3, 4))"#), "3.535534");
}

#[test]
fn winsorize_quarter_clip_extremes_bw() {
    assert_eq!(
        eval_string(r#"stringify(winsorize(25, 1, 2, 3, 4, 100))"#),
        r##"(2, 2, 3, 4, 100)"##
    );
}
