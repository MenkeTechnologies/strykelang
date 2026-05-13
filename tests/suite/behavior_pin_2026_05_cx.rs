//! Behavior-pinning batch CX (2026-05-09): **`windowed` / `chunked`** first operand must spread (tuple / variadic) — bracket
//! **`ARRAY`** alone fails (**BUG-174**); **`trimmed_mean`** leading **`ARRAY`** numifies to length — silent garbage (**BUG-175**);
//! **`base_convert`** two-arg numeric form — digit-string parsed in **`FROM`** radix (**BUG-176**);
//! **lists** (**`zip`**, **`unzip`**, **`interleave`**, **`partition_n`**), **strings** (**`levenshtein`**, **`damerau_levenshtein`**,
//! **`jaro_winkler`**, **`sorensen_dice`**), **stats** (**`harmonic_mean`**, **`rms`**, **`trimmed_mean`**, **`quantiles`**, **`skewness`**,
//! **`kurtosis`**, **`variance`**, **`weighted_mean`**, **`quartiles`**, **`iqr`**, **`ecdf`**, **`percentile_rank`**, **`clamp`**),
//! **sequences** (**`count_inversions`**, **`lis`**, **`longest_increasing`**, **`rotate`**, **`count_digits`**, **`digital_root`**, **`digits_of`**),
//! **bits / gray** (**`popcount`**, **`hamming_weight`**, **`binary_to_gray`**, **`gray_to_binary`**, **`gray_code_sequence`**), **interp**
//! (**`lerp`**, **`inverse_lerp`**), **aggregates** (**`mean`**, **`median`**, **`is_sorted`**, **`cumsum`**, **`cumprod`**, **`signum`**, **`copysign`**,
//! **`hypot`**), **combinatorics / NT** (**`nth_prime`**, **`is_prime`**, **`fib`**, **`factorial`**, **`euler_totient`**, **`sum_divisors`**
//! (proper divisors), **`mobius`**, **`bell_number`**, **`stirling2`**, **`binomial`**, **`merge_sorted`**, **`collatz_length`**, **`is_perfect`**,
//! **`base_convert`**, **`from_digits`**, **`polygon_area`**, **`elem_indices`**).

use crate::common::*;

/// Tuple / variadic first operand; a lone **`[LIST]`** cell is length **1** (**BUG-174**).
#[test]
fn windowed_tuple_two_overlap_three_windows_cx() {
    assert_eq!(
        eval_string(r#"stringify(windowed((1, 2, 3, 4), 2))"#),
        "([1, 2], [2, 3], [3, 4])"
    );
}

#[test]
fn windowed_bracket_array_yields_empty_bug_cx() {
    assert_eq!(eval_string(r#"stringify(windowed([1, 2, 3, 4], 2))"#), "()");
}

#[test]
fn chunked_tuple_pairs_cx() {
    assert_eq!(
        eval_string(r#"stringify(chunked((1, 2, 3, 4), 2))"#),
        "([1, 2], [3, 4])"
    );
}

#[test]
fn chunked_bracket_array_single_outer_chunk_bug_cx() {
    assert_eq!(
        eval_string(r#"stringify(chunked([1, 2, 3, 4], 2))"#),
        "[[1, 2, 3, 4]]"
    );
}

#[test]
fn trimmed_mean_twenty_percent_trim_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", trimmed_mean(20, [1, 2, 3, 4, 100]))"#),
        "3"
    );
}

/// **`to_number(ARRAY)`** = length → wrong **%** and **`collect_numbers`** tail (**BUG-175**).
#[test]
fn trimmed_mean_list_first_yields_mean_of_tail_only_bug_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", trimmed_mean([1, 2, 3, 4, 100], 20))"#),
        "3"
    );
}

#[test]
fn zip_unzip_roundtrip_lists_cx() {
    assert_eq!(
        eval_string(r#"stringify(zip([1, 2], [10, 20]))"#),
        "([1, 10], [2, 20])"
    );
    assert_eq!(
        eval_string(r#"stringify(unzip([(1, 10), (2, 20)]))"#),
        "([1, 2], [10, 20])"
    );
}

#[test]
fn interleave_shortest_exhausts_cx() {
    assert_eq!(
        eval_string(r#"stringify(interleave([1, 2, 3], [10, 20]))"#),
        "(1, 10, 2, 20, 3)"
    );
}

#[test]
fn partition_n_pairs_cx() {
    assert_eq!(
        eval_string(r#"stringify(partition_n(2, [1, 2, 3, 4]))"#),
        "([1, 2], [3, 4])"
    );
}

#[test]
fn string_distances_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", levenshtein("kitten", "sitting"))"#),
        "3"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", damerau_levenshtein("ab", "ba"))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaro_winkler("martha", "marhta"))"#),
        "0.9611111111"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", sorensen_dice("night", "nacht"))"#),
        "0"
    );
}

#[test]
fn harmonic_mean_three_values_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", harmonic_mean([2, 4, 8]))"#),
        "3.428571429"
    );
}

#[test]
fn rms_three_four_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", rms([3, 4]))"#),
        "3.535533906"
    );
}

#[test]
fn quantiles_interior_three_cx() {
    assert_eq!(
        eval_string(r#"stringify(quantiles([1, 2, 3, 4, 100], [0.25, 0.5, 0.75]))"#),
        "(2, 3, 4)"
    );
}

#[test]
fn skewness_and_kurtosis_five_cx() {
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
fn variance_and_weighted_mean_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", variance([1, 2, 3, 4]))"#),
        "1.25"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", weighted_mean([1, 2, 3], [1, 1, 1]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", weighted_mean([1, 2, 3], [1, 2, 3]))"#),
        "2.333333333"
    );
}

#[test]
fn count_inversions_lis_longest_increasing_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", count_inversions([3, 1, 2]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lis([3, 1, 2, 1, 4]))"#),
        "3"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", longest_increasing([3, 1, 2, 1, 4]))"#),
        "3"
    );
}

#[test]
fn digits_digital_root_count_digits_cx() {
    assert_eq!(
        eval_string(r#"stringify(digits_of(12345))"#),
        "(1, 2, 3, 4, 5)"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", digital_root(999))"#), "9");
    assert_eq!(eval_string(r#"sprintf("%.10g", count_digits(999))"#), "3");
}

#[test]
fn rotate_left_two_bracket_list_cx() {
    assert_eq!(
        eval_string(r#"stringify(rotate(2, [1, 2, 3, 4, 5]))"#),
        "(3, 4, 5, 1, 2)"
    );
}

#[test]
fn nth_prime_is_prime_fib_factorial_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", nth_prime(10))"#), "29");
    assert_eq!(eval_string(r#"sprintf("%.10g", is_prime(29))"#), "1");
    assert_eq!(eval_string(r#"sprintf("%.10g", fib(10))"#), "55");
    assert_eq!(eval_string(r#"sprintf("%.10g", factorial(6))"#), "720");
}

#[test]
fn popcount_hamming_weight_gray_family_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", popcount(7))"#), "3");
    assert_eq!(eval_string(r#"sprintf("%.10g", hamming_weight(15))"#), "4");
    assert_eq!(eval_string(r#"sprintf("%.10g", binary_to_gray(3))"#), "2");
    assert_eq!(eval_string(r#"sprintf("%.10g", gray_to_binary(2))"#), "3");
    assert_eq!(
        eval_string(r#"stringify(gray_code_sequence(3))"#),
        "(0, 1, 3, 2, 6, 7, 5, 4)"
    );
}

#[test]
fn clamp_min_max_value_order_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", clamp(0, 5, 10))"#), "5");
}

#[test]
fn lerp_and_inverse_lerp_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", lerp(0, 10, 0.25))"#), "2.5");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", inverse_lerp(2, 10, 6))"#),
        "0.5"
    );
}

#[test]
fn ecdf_and_percentile_rank_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ecdf([1, 2, 3, 4, 5], 3))"#),
        "0.6"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", percentile_rank(3, [1, 2, 3, 4, 5]))"#),
        "50"
    );
}

#[test]
fn quartiles_and_iqr_cx() {
    assert_eq!(
        eval_string(r#"stringify(quartiles([1, 2, 3, 4, 100]))"#),
        "(2, 3, 4)"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", iqr([1, 2, 3, 4, 100]))"#),
        "2"
    );
}

#[test]
fn euler_totient_sum_divisors_mobius_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", euler_totient(10))"#), "4");
    assert_eq!(eval_string(r#"sprintf("%.10g", sum_divisors(12))"#), "16");
    assert_eq!(eval_string(r#"sprintf("%.10g", mobius(30))"#), "-1");
}

#[test]
fn elem_indices_value_cx() {
    assert_eq!(
        eval_string(r#"stringify(elem_indices(2, [1, 2, 3, 2]))"#),
        "(1, 3)"
    );
}

#[test]
fn collatz_length_five_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", collatz_length(5))"#), "5");
}

#[test]
fn is_perfect_six_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", is_perfect(6))"#), "1");
}

#[test]
fn bell_number_four_stirling2_five_two_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", bell_number(4))"#), "15");
    assert_eq!(eval_string(r#"sprintf("%.10g", stirling2(5, 2))"#), "15");
}

#[test]
fn binomial_ten_three_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", binomial(10, 3))"#), "120");
}

#[test]
fn merge_sorted_two_lists_cx() {
    assert_eq!(
        eval_string(r#"stringify(merge_sorted([1, 3, 5], [2, 4, 6]))"#),
        "(1, 2, 3, 4, 5, 6)"
    );
}

#[test]
fn base_convert_decimal_string_to_hex_cx() {
    assert_eq!(eval_string(r##"base_convert("255", 10, 16)"##), "ff");
}

/// Missing explicit **`from=10`**, the digit string is parsed as **`from`** (**16**): **`"255"`** → **597** (**BUG-176**).
#[test]
fn base_convert_two_arg_numeric_parses_string_in_from_radix_bug_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", base_convert(255, 16))"#),
        "597"
    );
}

#[test]
fn from_digits_decimal_assembly_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", from_digits(1, 0, 1))"#),
        "101"
    );
}

#[test]
fn polygon_area_triangle_simplices_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", polygon_area([[0, 0], [2, 0], [2, 3]]))"#,),
        "3"
    );
}

#[test]
fn is_sorted_true_false_cx() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", is_sorted([1, 2, 3]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", is_sorted([1, 3, 2]))"#),
        "0"
    );
}

#[test]
fn cumsum_and_cumprod_cx() {
    assert_eq!(
        eval_string(r#"stringify(cumsum([1, 2, 3, 4]))"#),
        "(1, 3, 6, 10)"
    );
    assert_eq!(
        eval_string(r#"stringify(cumprod([1, 2, 3, 4]))"#),
        "(1, 2, 6, 24)"
    );
}

#[test]
fn mean_and_median_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", mean([2, 4, 6]))"#), "4");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", median([1, 2, 3, 4]))"#),
        "2.5"
    );
}

#[test]
fn signum_copysign_hypot_cx() {
    assert_eq!(eval_string(r#"sprintf("%.10g", signum(-3))"#), "-1");
    assert_eq!(eval_string(r#"sprintf("%.10g", copysign(1, -5))"#), "-1");
    assert_eq!(eval_string(r#"sprintf("%.10g", hypot(3, 4))"#), "5");
}
