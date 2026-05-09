//! Behavior-pinning batch CV (2026-05-09): **ML / stats losses** (**`huber_loss`**, **`hinge_loss`**,
//! **`ml_smooth_l1_loss`**, **`ml_binary_cross_entropy`** — open **(0, 1)** only, **BUG-171**), **options
//! pricing**, **geometry & distances**, **physics** (**`spring_*`**, **`kinetic_energy`**, **`gravity`**),
//! **optics** (**`snell`**, **`brewster_angle`**, **`thin_lens`**), **summaries** (**`five_number_summary`**,
//! **`outliers_iqr`**), **list utils** (**`prefix_sums`**, **`split_at`**, **`partition_two`**, **`intersperse`**),
//! **strings** (**`pad_left`**, **`hamming_distance_str`**, **`rgb_to_hsv`**), **distributions**, **`rmse`**.

use crate::common::*;

#[test]
fn wiener_filter_gain_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", wiener_filter(4, 1))"#),
        "0.8"
    );
}

#[test]
fn black_scholes_call_atm_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", black_scholes_call(100, 100, 0.25, 0.05, 0.2))"#),
        "4.61499713"
    );
}

#[test]
fn black_scholes_put_atm_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", black_scholes_put(100, 100, 0.25, 0.05, 0.2))"#),
        "3.372777179"
    );
}

#[test]
fn huber_loss_single_residual_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", huber_loss([3], [0], 1))"#),
        "2.5"
    );
}

#[test]
fn hinge_loss_soft_margin_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", hinge_loss(0.5, 1))"#),
        "0.5"
    );
}

#[test]
fn ml_smooth_l1_abs_residual_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_smooth_l1_loss(2, 0))"#),
        "2"
    );
}

#[test]
fn argmax_argmin_indices_cv() {
    assert_eq!(eval_string(r#"sprintf("%.0f", argmax([1, 9, 3]))"#), "1");
    assert_eq!(eval_string(r#"sprintf("%.0f", argmin([9, 1, 3]))"#), "1");
}

#[test]
fn ml_binary_cross_entropy_interior_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_binary_cross_entropy(0.5, 0.5))"#),
        "0.6931471806"
    );
}

/// **`p ∈ (0, 1)`** only; **`p = 1`** or **`p = 0`** yields **`inf`** (**BUG-171**).
#[test]
fn ml_binary_cross_entropy_prob_one_is_inf_bug_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_binary_cross_entropy(1, 1))"#),
        "inf"
    );
}

#[test]
fn ml_label_smoothing_binary_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_label_smoothing(1, 0.1, 2))"#),
        "0.95"
    );
}

#[test]
fn bilinear_interp_2d_center_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bilinear_interp_2d(0, 10, 20, 30, 0.5, 0.5))"#),
        "15"
    );
}

#[test]
fn triangle_heron_three_four_five_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", triangle_area_heron(3, 4, 5))"#),
        "6"
    );
}

#[test]
fn polygon_perimeter_right_triangle_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", polygon_perimeter([[0, 0], [3, 0], [3, 4]]))"#),
        "12"
    );
}

#[test]
fn dot_product_plane_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dot_product([1, 2], [3, 4]))"#),
        "11"
    );
}

#[test]
fn det_trace_small_matrix_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", det([[1, 2], [3, 4]]))"#),
        "-2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_trace([[1, 2], [3, 4]]))"#),
        "5"
    );
}

#[test]
fn euclidean_canberra_bray_chi_metrics_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", euclidean_distance([0, 0], [3, 4]))"#),
        "5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", canberra_distance([1, 1], [3, 5]))"#),
        "1.166666667"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bray_curtis_distance([1, 2], [4, 6]))"#),
        "0.5384615385"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chi_squared_distance([1, 0], [1, 1]))"#),
        "0.5"
    );
}

#[test]
fn spring_force_and_energy_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", spring_force(2, 0.5))"#),
        "-1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", spring_energy(2, 0.5))"#),
        "0.25"
    );
}

#[test]
fn kinetic_energy_quarter_mass_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kinetic_energy(10, 2))"#),
        "20"
    );
}

#[test]
fn standard_gravity_constant_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gravity())"#),
        "9.80665"
    );
}

#[test]
fn snell_refraction_air_to_glass_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", snell(1, 1.5, 30))"#),
        "19.47122063"
    );
}

#[test]
fn brewster_angle_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", brewster_angle(1, 1.5))"#),
        "56.30993247"
    );
}

#[test]
fn thin_lens_image_distance_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", thin_lens(20, 10))"#),
        "20"
    );
}

#[test]
fn five_number_summary_short_cv() {
    assert_eq!(
        eval_string(r#"stringify(five_number_summary([1, 2, 3, 4, 100]))"#),
        "(1, 2, 3, 4, 100)"
    );
}

#[test]
fn outliers_iqr_single_high_tail_cv() {
    assert_eq!(
        eval_string(r#"stringify(outliers_iqr([1, 2, 3, 4, 100]))"#),
        "100"
    );
}

#[test]
fn rle_rld_roundtrip_cv() {
    assert_eq!(
        eval_string(r#"stringify(rld(rle(1, 1, 2)))"#),
        "(\"1\", \"1\", \"2\")"
    );
}

#[test]
fn pad_left_zeros_cv() {
    assert_eq!(
        eval_string(r#"pad_left("ab", 5, "0")"#),
        "000ab"
    );
}

#[test]
fn hamming_distance_str_substitution_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", hamming_distance_str("abc", "axc"))"#),
        "1"
    );
}

#[test]
fn rgb_red_to_hsv_cv() {
    assert_eq!(
        eval_string(r#"stringify(rgb_to_hsv(255, 0, 0))"#),
        "(0, 1, 1)"
    );
}

#[test]
fn poisson_pmf_four_lambda_three_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", poisson_pmf(4, 3))"#),
        "0.1680313557"
    );
}

#[test]
fn dnbinom_two_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dnbinom(2, 5, 0.3))"#),
        "0.0178605"
    );
}

#[test]
fn intersperse_zeros_cv() {
    assert_eq!(
        eval_string(r#"stringify(intersperse([1, 2, 3], 0))"#),
        "(1, 0, 2, 0, 3)"
    );
}

#[test]
fn repeat_list_twice_cv() {
    assert_eq!(
        eval_string(r#"stringify(repeat_list(2, [1, 2]))"#),
        "(1, 2, 1, 2)"
    );
}

#[test]
fn split_at_index_three_cv() {
    assert_eq!(
        eval_string(r#"stringify(split_at(3, [1, 2, 3, 4, 5]))"#),
        "([1, 2, 3], [4, 5])"
    );
}

#[test]
fn partition_two_prefix_cv() {
    assert_eq!(
        eval_string(r#"stringify(partition_two([1, 2, 3, 4, 5]))"#),
        "([1, 2], [3, 4, 5])"
    );
}

#[test]
fn prefix_sums_partial_cv() {
    assert_eq!(
        eval_string(r#"stringify(prefix_sums([3, 5, 7]))"#),
        "(3, 8, 15)"
    );
}

#[test]
fn rmse_two_points_cv() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", rmse([1, 2], [1, 4]))"#),
        "1.414213562"
    );
}
