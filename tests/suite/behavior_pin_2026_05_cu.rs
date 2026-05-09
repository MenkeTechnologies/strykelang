//! Behavior-pinning batch CU (2026-05-09): **moments** (**`variance`**, **`covariance`**, **`covar_samp`**, **`pearsonr`**),
//! **series ops** (**`diff`**, **`moving_average`**, **`exponential_moving_average`**, **`cumprod`**, **`rotate`**),
//! **batching** (**`chunk_n`**, **`sliding_pairs`**) — reversed-arg footgun (**BUG-170**), **regression** (**`linreg`**, **`detrend_linear`**),
//! **tests & primes**, **distances** (**`total_variation_distance`**, **`theil_index`**), **utils** (**`transpose`**, **`outer`**,
//! **`vector_normalize`**, **`mod_inv`**, **`base64` round-trip**).

use crate::common::*;

#[test]
fn variance_even_spread_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", variance([2, 4, 6, 8]))"#),
        "5"
    );
}

#[test]
fn covariance_perfect_linear_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", covariance([1, 2, 3], [2, 4, 6]))"#),
        "2"
    );
}

#[test]
fn covar_samp_matches_covariance_short_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", covar_samp([1, 2, 3], [2, 4, 6]))"#),
        "2"
    );
}

#[test]
fn pearsonr_unit_correlation_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pearsonr([1, 2, 3], [2, 4, 6]))"#),
        "1"
    );
}

#[test]
fn diff_consecutive_cu() {
    assert_eq!(
        eval_string(r#"stringify(diff([10, 7, 3, 1]))"#),
        "(-3, -4, -2)"
    );
}

#[test]
fn moving_average_window_first_three_cu() {
    assert_eq!(
        eval_string(r#"stringify(moving_average(3, 1, 2, 3, 4, 5))"#),
        "(2, 3, 4)"
    );
}

/// **`moving_average`** expects **`WINDOW` first**. A leading **`ARRAYREF`** has **`to_int() == 0`** → window clamps to **1**,
/// so **`([1,2,3], 5)`** averages only **`5`** (**BUG-170**).
#[test]
fn moving_average_arrayref_first_tail_only_bug_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", moving_average([1, 2, 3], 5))"#),
        "5"
    );
}

#[test]
fn exponential_moving_average_half_alpha_cu() {
    assert_eq!(
        eval_string(r#"stringify(exponential_moving_average(0.5, [1, 2, 3, 4]))"#),
        "(1, 1.5, 2.25, 3.125)"
    );
}

#[test]
fn detrend_linear_ramp_slope_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", detrend_linear([1, 2, 3, 4, 5]))"#),
        "1"
    );
}

#[test]
fn linreg_perfect_line_cu() {
    assert_eq!(
        eval_string(r#"stringify(linreg([1, 2, 3], [2, 5, 8]))"#),
        "(3, -1, 1)"
    );
}

#[test]
fn chi_square_stat_three_cells_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chi_square_stat([10, 20, 30], [12, 18, 30]))"#),
        "0.5555555556"
    );
}

#[test]
fn is_perfect_six_cu() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_perfect(6))"#), "1");
}

#[test]
fn nth_prime_tenth_cu() {
    assert_eq!(eval_string(r#"sprintf("%.0f", nth_prime(10))"#), "29");
}

#[test]
fn prime_factors_sixty_cu() {
    assert_eq!(
        eval_string(r#"stringify(prime_factors(60))"#),
        "(2, 2, 3, 5)"
    );
}

#[test]
fn acosh_two_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", acosh(2))"#),
        "1.316957897"
    );
}

#[test]
fn asinh_one_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", asinh(1))"#),
        "0.881373587"
    );
}

#[test]
fn log10_hundred_cu() {
    assert_eq!(eval_string(r#"sprintf("%.10g", log10(100))"#), "2");
}

#[test]
fn sign_negative_three_cu() {
    assert_eq!(eval_string(r#"sprintf("%.0f", sign(-3))"#), "-1");
}

#[test]
fn atan2_both_negative_quadrant_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", atan2(-1, -1))"#),
        "-2.35619449"
    );
}

#[test]
fn transpose_two_variadic_rows_cu() {
    assert_eq!(
        eval_string(r#"stringify(transpose([1, 2], [3, 4]))"#),
        "([1, 3], [2, 4])"
    );
}

#[test]
fn max2_min2_cu() {
    assert_eq!(eval_string(r#"sprintf("%.10g", max2(1, 9))"#), "9");
    assert_eq!(eval_string(r#"sprintf("%.10g", min2(1, 9))"#), "1");
}

#[test]
fn hamming_weight_fifteen_cu() {
    assert_eq!(eval_string(r#"sprintf("%.0f", hamming_weight(15))"#), "4");
}

#[test]
fn is_sorted_nondecreasing_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", is_sorted([1, 2, 2, 9]))"#),
        "1"
    );
}

#[test]
fn all_unique_distinct_and_dup_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", all_unique([1, 2, 3]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.0f", all_unique([1, 2, 2]))"#),
        "0"
    );
}

#[test]
fn chunk_n_size_first_cu() {
    assert_eq!(
        eval_string(r#"stringify(chunk_n(2, [1, 2, 3, 4]))"#),
        "([1, 2], [3, 4])"
    );
}

/// Reversed **`(LIST, N)`** uses **`len(LIST)`** as chunk size and **`N`** as the only datum (**BUG-170**).
#[test]
fn chunk_n_list_first_yields_single_tail_chunk_bug_cu() {
    assert_eq!(
        eval_string(r#"stringify(chunk_n([1, 2, 3, 4], 2))"#),
        "[2]"
    );
}

#[test]
fn sliding_pairs_three_cu() {
    assert_eq!(
        eval_string(r#"stringify(sliding_pairs([1, 2, 3]))"#),
        "([1, 2], [2, 3])"
    );
}

#[test]
fn harmonic_number_ten_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", harmonic_number(10))"#),
        "2.928968254"
    );
}

#[test]
fn rank_order_three_cu() {
    assert_eq!(
        eval_string(r#"stringify(rank([3, 1, 2]))"#),
        "(3, 1, 2)"
    );
}

#[test]
fn outer_product_two_by_two_cu() {
    assert_eq!(
        eval_string(r#"stringify(outer([1, 2], [10, 20]))"#),
        "((10, 20), (20, 40))"
    );
}

#[test]
fn vector_normalize_three_four_cu() {
    assert_eq!(
        eval_string(r#"stringify(vector_normalize([3, 4]))"#),
        "(0.6, 0.8)"
    );
}

#[test]
fn total_variation_two_point_one_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", total_variation_distance([0.5, 0.5], [0.4, 0.6]))"#),
        "0.1"
    );
}

#[test]
fn theil_index_three_incomes_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", theil_index([10, 20, 30]))"#),
        "0.08720802396"
    );
}

#[test]
fn cumprod_three_integers_cu() {
    assert_eq!(
        eval_string(r#"stringify(cumprod([2, 3, 4]))"#),
        "(2, 6, 24)"
    );
}

#[test]
fn rotate_left_by_one_cu() {
    assert_eq!(
        eval_string(r#"stringify(rotate(1, [1, 2, 3, 4]))"#),
        "(2, 3, 4, 1)"
    );
}

#[test]
fn mod_inv_three_mod_seven_cu() {
    assert_eq!(eval_string(r#"sprintf("%.0f", mod_inv(3, 7))"#), "5");
}

#[test]
fn word_count_three_tokens_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", word_count("a bb ccc"))"#),
        "3"
    );
}

#[test]
fn base64_decode_encode_roundtrip_ascii_cu() {
    assert_eq!(
        eval_string(r#"base64_decode(base64_encode("hello"))"#),
        "hello"
    );
}

#[test]
fn renyi_order_two_uniform_four_cu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", renyi_entropy(2, [0.25, 0.25, 0.25, 0.25]))"#),
        "0"
    );
}
