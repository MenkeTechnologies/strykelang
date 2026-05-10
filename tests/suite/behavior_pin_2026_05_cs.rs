//! Behavior-pinning batch CS (2026-05-09): correlations / rank stats, **edit-distance** and **soundex**,
//! **ML activations** (`gelu`, **`softplus`**, **`elu`**), **order-stat** helpers, **JSON** round-trip,
//! **geo** (`haversine_distance`), **DSP windows** vs **string Hamming** (**BUG-168**), **R-style**
//! **`pchisq` / `pt`**, **`softmax`**, **`clamp`** / **`saturate`** / **`lerp`**, **`matrix_transpose`**
//! (contrast variadic **`transpose`** — **BUG-159**), **quaternion** multiply, **`zip`**, **`rle`**.

use crate::common::*;

#[test]
fn correlation_perfect_linear_cs() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", correlation([1, 2, 3, 4, 5], [2, 4, 6, 8, 10]))"#),
        "1"
    );
}

#[test]
fn kendall_tau_one_swap_cs() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kendall_tau([1, 2, 3], [1, 3, 2]))"#),
        "0.3333333333"
    );
}

#[test]
fn damerau_levenshtein_adjacent_swap_cs() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", damerau_levenshtein("foo", "ofo"))"#),
        "1"
    );
}

#[test]
fn jaro_winkler_prefix_discount_cs() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaro_winkler("food", "foo"))"#),
        "0.9416666667"
    );
}

#[test]
fn soundex_smith_cs() {
    assert_eq!(eval_string(r#"soundex("Smith")"#), "S530");
}

#[test]
fn gelu_one_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", gelu(1))"#), "0.9135418956");
}

#[test]
fn softplus_zero_is_ln2_cs() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", softplus(0))"#),
        "0.6931471806"
    );
}

#[test]
fn elu_negative_one_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", elu(-1))"#), "-0.6321205588");
}

#[test]
fn is_semver_valid_and_invalid_cs() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_semver("1.2.3"))"#), "1");
    assert_eq!(eval_string(r#"sprintf("%.0f", is_semver("v1.2"))"#), "0");
}

#[test]
fn pchisq_one_three_df_cs() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pchisq(1, 3))"#),
        "0.1987480431"
    );
}

#[test]
fn pt_zero_five_df_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", pt(0, 5))"#), "0.4999971122");
}

#[test]
fn softmax_three_logits_stringify_cs() {
    assert_eq!(
        eval_string(r#"stringify(softmax([1, 2, 3]))"#),
        "(0.0900305731703805, 0.244728471054798, 0.665240955774822)"
    );
}

/// **`hamming(N)`** is the DSP window; use **`hamming_distance`** for string edit distance (**BUG-168**).
#[test]
fn dsp_hamming_window_four_stringify_cs() {
    assert_eq!(
        eval_string(r#"stringify(hamming(4))"#),
        "(0.08, 0.77, 0.77, 0.08)"
    );
}

#[test]
fn string_hamming_distance_bitstrings_cs() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", hamming_distance("1101", "1001"))"#),
        "1"
    );
}

#[test]
fn hann_window_length_eight_first_sample_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", first(hann(8)))"#), "0");
}

#[test]
fn lp_norm_two_matches_hypotenuse_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", lp_norm([3, 4], 2))"#), "5");
}

#[test]
fn levenshtein_single_substitution_cs() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", levenshtein("abcd", "abxd"))"#),
        "1"
    );
}

#[test]
fn smoothstep_midpoint_cs() {
    assert_eq!(eval_string(r#"smoothstep(0, 1, 0.5)"#), "0.5");
}

#[test]
fn lerp_quarter_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", lerp(0, 100, 0.25))"#), "25");
}

#[test]
fn saturate_clips_above_one_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", saturate(1.5))"#), "1");
}

#[test]
fn clamp_scalar_between_bounds_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", clamp(0, 10, 12))"#), "10");
}

#[test]
fn clamp_list_variadic_cs() {
    assert_eq!(
        eval_string(r#"stringify(clamp(0, 10, 12, 15, -3))"#),
        "(10, 10, 0)"
    );
}

#[test]
fn cumsum_nonunit_steps_cs() {
    assert_eq!(eval_string(r#"stringify(cumsum([2, 3, 5]))"#), "(2, 5, 10)");
}

#[test]
fn median_odd_triple_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", median([1, 2, 9]))"#), "2");
}

#[test]
fn mad_simple_list_cs() {
    assert_eq!(eval_string(r#"sprintf("%.10g", mad([1, 2, 3, 10]))"#), "2");
}

#[test]
fn json_encode_decode_roundtrip_cs() {
    assert_eq!(
        eval_string(r#"stringify(json_decode(json_encode({ x => 42 })))"#),
        "+{x => 42}"
    );
}

#[test]
fn haversine_one_degree_equator_cs() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", haversine_distance(0, 0, 0, 1))"#),
        "111.195"
    );
}

#[test]
fn matrix_transpose_nested_two_by_two_cs() {
    assert_eq!(
        eval_string(r#"stringify(matrix_transpose([[1, 2], [3, 4]]))"#),
        "[[1, 3], [2, 4]]"
    );
}

#[test]
fn quat_mul_basis_i_j_is_k_cs() {
    assert_eq!(
        eval_string(r#"stringify(quat_mul([0, 1, 0, 0], [0, 0, 1, 0]))"#),
        "(0, 0, 0, 1)"
    );
}

#[test]
fn zip_parallel_lists_cs() {
    assert_eq!(
        eval_string(r#"stringify(zip([1, 2], [10, 20]))"#),
        "([1, 10], [2, 20])"
    );
}

#[test]
fn run_length_encode_runs_cs() {
    assert_eq!(
        eval_string(r#"stringify(rle(1, 1, 1, 2, 2))"#),
        r#"(["1", 3], ["2", 2])"#
    );
}
