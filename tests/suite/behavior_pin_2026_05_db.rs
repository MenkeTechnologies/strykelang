//! Behavior-pinning batch DB (2026-05-09): **descriptive stats** (`median`, `variance`, `stddev`, `skewness`,
//! `kurtosis`, `percentile` vs **`quantile`** scale), **means** (`harmonic_mean`, `geometric_mean`, **`weighted_mean`**),
//! **vectors** (`dot_product`, `cosine_similarity`, `euclidean_distance`, `manhattan_distance`, `minkowski_distance`,
//! **`cross_product`**), **planar** **`chebyshev_distance(x1,y1,x2,y2)`** (not list args — cross-ref **BUG-162**),
//! **strings / sets** (`lcs_length`, `edit_distance`, **`dice_coefficient`** on scalars — **BUG-184**), **ML-ish**
//! (`confusion_counts`, `one_hot`, `softmax`, `sigmoid`, `relu`, `leaky_relu`, `gelu`), **special functions**
//! (`factorial`, `lgamma`, `tgamma`, `digamma`, `beta_fn`, `betainc`, `bessel_j` / `bessel_i` / `bessel_k`,
//! `riemann_zeta`), **convolution** (`convolve_valid`, `convolve_full` — scalar outputs today), **`zscore(X, sample…)`**
//! (value first), **information** (`entropy`, `entropy_bits`, `kl_divergence`), **runs** (`cumsum`, `diff`,
//! `numerical_diff`), **matrix** (`frobenius_norm`, `matrix_multiply`, `matrix_rank`), **geo / color**
//! (`haversine_distance`, `vincenty_distance`, `rgb_to_hsv`, `hsv_to_rgb`), **similarity** (`jaccard_index`,
//! `overlap_coefficient`), **combinatorics** (`bell_number`, `catalan`, `derangements`, `partition_function`),
//! **order stats** (`argmax`, `argmin`, `percentile_rank`), **`winsorize(PCT, …)`** (**BUG-185**), **RLE**
//! (`run_length_encode`), **autocorrelation**, **search** (`binary_search`, `lower_bound`, `upper_bound`, `equal_range` —
//! **needle / value first** — **BUG-183**), **covariance** / **`outer_product`**, **`r_squared`**, **Poisson / uniforms**
//! (`poisson_pmf`, `ppois`, `dunif`, `punif`), **polygon** (`shoelace_area`).

use crate::common::*;

#[test]
fn median_variance_stddev_small_list_db() {
    assert_eq!(eval_string(r#"sprintf("%.10g", median([3, 1, 2]))"#), "2");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", variance([1, 2, 3]))"#),
        "0.6666666667"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", stddev([1, 2, 3]))"#),
        "0.8164965809"
    );
}

#[test]
fn skewness_zero_kurtosis_platycurtic_five_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", skewness([1, 2, 3, 4, 5]))"#),
        "0"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kurtosis([1, 2, 3, 4, 5]))"#),
        "-1.3"
    );
}

#[test]
fn percentile_vs_quantile_median_conventions_db() {
    assert_eq!(
        eval_string(r#"stringify(percentile([10, 20, 30, 40, 50], 50))"#),
        "50"
    );
    assert_eq!(
        eval_string(r#"stringify(quantile([10, 20, 30, 40, 50], 0.5))"#),
        "30"
    );
}

#[test]
fn harmonic_geometric_weighted_mean_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", harmonic_mean([1, 2, 4]))"#),
        "1.714285714"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", geometric_mean([1, 2, 4]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", weighted_mean([10, 20], [1, 3]))"#),
        "17.5"
    );
}

#[test]
fn dot_cos_eucl_manhattan_cross_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dot_product([1, 2, 3], [4, 5, 6]))"#),
        "32"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cosine_similarity([1, 2, 3], [4, 5, 6]))"#),
        "0.9746318462"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", euclidean_distance([0, 0], [3, 4]))"#),
        "5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", manhattan_distance([1, 2], [4, 6]))"#),
        "7"
    );
    assert_eq!(
        eval_string(r#"stringify(cross_product([1, 0, 0], [0, 1, 0]))"#),
        "(0, 0, 1)"
    );
}

#[test]
fn minkowski_l1_l2_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", minkowski_distance([0, 0], [3, 4], 1))"#),
        "7"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", minkowski_distance([0, 0], [3, 4], 2))"#),
        "5"
    );
}

#[test]
fn chebyshev_scalar_four_args_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chebyshev_distance(0, 0, 3, 4))"#),
        "4"
    );
}

#[test]
fn lcs_length_edit_distance_db() {
    assert_eq!(eval_string(r#"stringify(lcs_length("abc", "ac"))"#), "2");
    assert_eq!(eval_string(r#"stringify(edit_distance("abc", "def"))"#), "3");
}

#[test]
fn confusion_counts_binary_db() {
    assert_eq!(
        eval_string(r#"stringify(confusion_counts([1, 0, 1], [1, 1, 0]))"#),
        "(1, 1, 0, 1)"
    );
}

#[test]
fn one_hot_softmax_sigmoid_relu_leaky_gelu_db() {
    assert_eq!(
        eval_string(r#"stringify(one_hot(2, 4))"#),
        "(0, 0, 1, 0)"
    );
    assert_eq!(
        eval_string(r#"stringify(softmax([1, 2, 3]))"#),
        "(0.0900305731703805, 0.244728471054798, 0.665240955774822)"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", sigmoid(0))"#), "0.5");
    assert_eq!(eval_string(r#"sprintf("%.10g", relu(-3))"#), "0");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", leaky_relu(-3, 0.01))"#),
        "-0.03"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", gelu(0))"#), "0");
}

#[test]
fn gamma_beta_bessel_zeta_samples_db() {
    assert_eq!(eval_string(r#"sprintf("%.10g", factorial(5))"#), "120");
    assert_eq!(eval_string(r#"sprintf("%.10g", lgamma(5))"#), "3.17805383");
    assert_eq!(eval_string(r#"sprintf("%.10g", tgamma(5))"#), "24");
    assert_eq!(eval_string(r#"sprintf("%.10g", digamma(2))"#), "0.4227843351");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", beta_fn(2, 3))"#),
        "0.08333333333"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", betainc(0.5, 2, 3))"#),
        "0.6875"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bessel_j(0, 1))"#),
        "0.7651976866"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bessel_i(0, 1))"#),
        "1.266065848"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bessel_k(0, 1))"#),
        "0.4210244211"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", riemann_zeta(2))"#),
        "1.644934067"
    );
}

#[test]
fn convolve_valid_full_scalar_outputs_db() {
    assert_eq!(
        eval_string(r#"stringify(convolve_valid([1, 2, 1], [1, 1]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"stringify(convolve_full([1, 2], [1, 1, 1]))"#),
        "4"
    );
}

#[test]
fn zscore_value_then_sample_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", zscore(5, [1, 2, 3, 4, 5]))"#),
        "1.414213562"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", zscore(3, [1, 2, 3, 4, 5]))"#),
        "0"
    );
}

#[test]
fn entropy_kl_divergence_small_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", entropy([1, 1, 2, 2]))"#),
        "1.329661349"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", entropy_bits([1, 1, 2, 2]))"#),
        "1.918295834"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kl_divergence([0.5, 0.5], [0.6, 0.4]))"#),
        "0.02041099726"
    );
}

#[test]
fn cumsum_diff_numerical_diff_db() {
    assert_eq!(
        eval_string(r#"stringify(cumsum([1, 2, 3, 4]))"#),
        "(1, 3, 6, 10)"
    );
    assert_eq!(
        eval_string(r#"stringify(diff([1, 4, 9, 16]))"#),
        "(3, 5, 7)"
    );
    assert_eq!(
        eval_string(r#"stringify(numerical_diff([1, 4, 9, 16], 1))"#),
        "(3, 4, 6, 7)"
    );
}

#[test]
fn frobenius_norm_and_matrix_multiply_identity_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", frobenius_norm([[1, 2], [3, 4]]))"#),
        "5.477225575"
    );
    assert_eq!(
        eval_string(r#"stringify(matrix_multiply([[1, 2], [3, 4]], [[1, 0], [0, 1]]))"#),
        "([1, 2], [3, 4])"
    );
}

#[test]
fn matrix_rank_singular_vs_invertible_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_rank([[1, 2], [2, 4]]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_rank([[1, 2], [3, 4]]))"#),
        "2"
    );
}

#[test]
fn haversine_vincenty_equator_one_degree_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", haversine_distance(0, 0, 0, 1))"#),
        "111.1949266"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", vincenty_distance(0, 0, 0, 1))"#),
        "111319.4908"
    );
}

#[test]
fn rgb_hsv_red_roundtrip_db() {
    assert_eq!(
        eval_string(r#"stringify(rgb_to_hsv(255, 0, 0))"#),
        "(0, 1, 1)"
    );
    assert_eq!(
        eval_string(r#"stringify(hsv_to_rgb(0, 1, 1))"#),
        "(255, 0, 0)"
    );
}

#[test]
fn jaccard_overlap_numeric_sets_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaccard_index([1, 2, 3], [2, 3, 4]))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", overlap_coefficient([1, 2, 3], [2, 3, 4]))"#),
        "0.6666666667"
    );
}

#[test]
fn autocorrelation_lag_one_simple_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", autocorrelation([1, 2, 3, 2, 1], 1))"#),
        "1"
    );
}

#[test]
fn run_length_encode_numeric_db() {
    assert_eq!(
        eval_string(r#"stringify(run_length_encode([1, 1, 2, 2, 2, 3]))"#),
        r#"(["1", 2], ["2", 3], ["3", 1])"#
    );
}

#[test]
fn combinatorics_bell_catalan_derangements_partition_db() {
    assert_eq!(eval_string(r#"sprintf("%d", bell_number(4))"#), "15");
    assert_eq!(eval_string(r#"sprintf("%.10g", catalan(4))"#), "14");
    assert_eq!(eval_string(r#"sprintf("%d", derangements(4))"#), "36");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", partition_function(5))"#),
        "0.006737946999"
    );
}

#[test]
fn argmax_argmin_percentile_rank_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", argmax([3, 1, 4, 1, 5]))"#),
        "4"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", argmin([3, 1, 4, 1, 5]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", percentile_rank(25, [10, 20, 30, 40, 50]))"#),
        "40"
    );
}

#[test]
fn winsorize_percent_first_bracket_list_db() {
    assert_eq!(
        eval_string(
            r#"join(",", winsorize(10, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]))"#
        ),
        "2,2,3,4,5,6,7,8,9,10,11"
    );
}

/// **`winsorize(PCT, LIST)`** — putting the **array first** treats the bracket list as **`pct`** and leaves a bogus sample (**BUG-185**).
#[test]
fn winsorize_array_first_yields_scalar_noise_db() {
    assert_eq!(
        eval_string(r#"stringify(winsorize([1, 2, 3, 4, 100], 10))"#),
        "10"
    );
}

/// **`binary_search NEEDLE, LIST`**, **`lower_bound VALUE, LIST`**, etc. — not **`(ARRAYREF, scalar)`** (**BUG-183**).
#[test]
fn binary_search_lower_upper_correct_needle_first_db() {
    assert_eq!(
        eval_string(r#"stringify(binary_search(5, [1, 3, 5, 7]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"stringify(lower_bound(5, [1, 3, 5, 7]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"stringify(upper_bound(5, [1, 3, 5, 7]))"#),
        "3"
    );
    assert_eq!(
        eval_string(r#"stringify(equal_range(5, [1, 3, 5, 7]))"#),
        "(2, 3)"
    );
}

#[test]
fn binary_search_swapped_args_not_found_db() {
    assert_eq!(
        eval_string(r#"stringify(binary_search([1, 3, 5, 7], 5))"#),
        "-1"
    );
}

#[test]
fn lower_bound_swapped_args_returns_zero_db() {
    assert_eq!(
        eval_string(r#"stringify(lower_bound([1, 3, 5, 7], 5))"#),
        "0"
    );
}

/// String operands are **single multiset elements**; use **`split //, $s`** (etc.) for character Dice (**BUG-184**).
#[test]
fn dice_coefficient_strings_singleton_tokens_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dice_coefficient("abc", "abd"))"#),
        "0"
    );
}

#[test]
fn dice_coefficient_numeric_lists_expected_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dice_coefficient([1, 2, 3], [1, 2, 4]))"#),
        "0.6666666667"
    );
}

#[test]
fn covariance_outer_product_perfect_slope_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", covariance([1, 2, 3], [2, 4, 6]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"stringify(outer_product([1, 2], [10, 20]))"#),
        "((10, 20), (20, 40))"
    );
}

#[test]
fn r_squared_almost_collinear_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", r_squared([1, 2, 3], [1.1, 2.2, 2.9]))"#),
        "0.97"
    );
}

#[test]
fn poisson_pmf_and_cdf_uniform_density_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", poisson_pmf(3, 2))"#),
        "0.1804470443"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ppois(3, 2))"#),
        "0.8571234605"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dunif(0.25, 0, 1))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", punif(0.25, 0, 1))"#),
        "0.25"
    );
}

#[test]
fn shoelace_area_right_triangle_db() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", shoelace_area([[0, 0], [4, 0], [0, 3]]))"#),
        "6"
    );
}
