//! Behavior-pinning batch CO (2026-05-09): descriptive stats — **`percentile(P, LIST)`** on a **0–100**
//! scale vs **`quantile(LIST, p)`** with **`p`** last on **0–1** (**BUG-161**), multiset **`frequencies`**, **`group_by_fn`**, block **`partition`**, moments / means,
//! binomial family, `erf` / `gamma`, set-ish list ops (`array_*`, `union_list`), zips, **`take_n`** /
//! **`drop_n`** vs **`take(LIST, COUNT)`** (incl. **`ARRAYREF`** bucket via **BUG-143**), **`product`**
//! bracket trap (**BUG-140**), ranking, Cartesian helpers, `clamp` / basic math, hash merge / invert,
//! and iterator-ish list transforms.

use crate::common::*;

#[test]
fn percentile_fifty_median_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", percentile(50, [1, 2, 3, 4, 5]))"#),
        "3"
    );
}

#[test]
fn percentile_fraction_is_percent_units_not_quantile_bug_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", percentile(0.5, [1, 2, 3, 4, 5]))"#),
        "1"
    );
}

#[test]
fn quantile_half_matches_intuition_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", quantile([1, 2, 3, 4, 5], 0.5))"#),
        "3"
    );
}

#[test]
fn quantile_probability_first_arg_is_not_list_plus_p_bug_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", quantile(0.5, [1, 2, 3, 4, 5]))"#),
        "0.5"
    );
}

#[test]
fn percentile_zero_and_hundred_extrema_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", percentile(0, [1, 2, 3, 4, 5]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.6g", percentile(100, [1, 2, 3, 4, 5]))"#),
        "5"
    );
}

#[test]
fn frequencies_multiset_counts_co() {
    assert_eq!(
        eval_string(r#"stringify(frequencies([1, 2, 2, 3]))"#),
        r#"+{1 => 1, 2 => 2, 3 => 1}"#
    );
}

#[test]
fn group_by_fn_first_grapheme_co() {
    assert_eq!(
        eval_string(
            r#"stringify(group_by_fn(sub { substr($_, 0, 1) }, qw(apple apricot banana)))"#,
        ),
        r#"+{a => ["apple", "apricot"], b => ["banana"]}"#,
    );
}

#[test]
fn partition_block_first_letter_co() {
    assert_eq!(
        eval_string(
            r#"stringify(partition { substr($_, 0, 1) eq "a" } qw(apple banana apricot))"#,
        ),
        r#"(["apple", "apricot"], ["banana"])"#,
    );
}

#[test]
fn skewness_three_values_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.5g", skewness([1, 2, 4]))"#),
        "1.7181"
    );
}

#[test]
fn kurtosis_four_values_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", kurtosis([1, 2, 3, 4]))"#),
        "-1.36"
    );
}

#[test]
fn rms_three_four_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", rms([3, 4]))"#),
        "3.53553"
    );
}

#[test]
fn mean_arith_three_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", mean([2, 4, 6]))"#),
        "4"
    );
}

#[test]
fn harmonic_mean_three_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", harmonic_mean([1, 2, 4]))"#),
        "1.71429"
    );
}

#[test]
fn weighted_mean_pair_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", weighted_mean([1, 2], [1, 3]))"#),
        "1.75"
    );
}

#[test]
fn multinomial_coeff_5_2_3_co() {
    assert_eq!(eval_string(r#"sprintf("%.0f", multinomial(5, 2, 3))"#), "10");
}

#[test]
fn binomial_10_choose_3_co() {
    assert_eq!(eval_string(r#"sprintf("%.0f", binomial(10, 3))"#), "120");
}

#[test]
fn lgamma_five_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", lgamma(5.0))"#),
        "3.17805"
    );
}

#[test]
fn tgamma_five_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", tgamma(5.0))"#),
        "24"
    );
}

#[test]
fn erf_one_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", erf(1.0))"#),
        "0.842701"
    );
}

#[test]
fn erfc_one_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", erfc(1.0))"#),
        "0.157299"
    );
}

#[test]
fn is_subset_and_superset_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", is_subset([1, 2], [1, 2, 3]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.0f", is_superset([1, 2, 3], [1, 2]))"#),
        "1"
    );
}

#[test]
fn array_union_intersection_difference_co() {
    assert_eq!(
        eval_string(r#"stringify(array_union([1, 2], [2, 3]))"#),
        r#"("1", "2", "3")"#
    );
    assert_eq!(
        eval_string(r#"stringify(array_intersection([1, 2], [2, 3]))"#),
        r#""2""#
    );
    assert_eq!(
        eval_string(r#"stringify(array_difference([1, 2, 3], [2]))"#),
        r#"("1", "3")"#
    );
}

#[test]
fn union_list_numeric_concat_unique_co() {
    assert_eq!(
        eval_string(r#"stringify(union_list([1, 2], [2, 3]))"#),
        "(1, 2, 3)"
    );
}

#[test]
fn zip_three_parallel_lists_co() {
    assert_eq!(
        eval_string(r#"stringify(zip([1, 2], [10, 20], [7, 8]))"#),
        "([1, 10, 7], [2, 20, 8])"
    );
}

#[test]
fn zip_fill_padding_co() {
    assert_eq!(
        eval_string(r#"stringify(zip_fill(undef, [1, 2], [10]))"#),
        "([1, 10], [2, undef])"
    );
}

#[test]
fn take_n_and_drop_n_co() {
    assert_eq!(
        eval_string(r#"stringify(take_n(2, [1, 2, 3, 4]))"#),
        "(1, 2)"
    );
    assert_eq!(
        eval_string(r#"stringify(drop_n(2, [1, 2, 3, 4]))"#),
        "(3, 4)"
    );
}

#[test]
fn take_variadic_list_count_last_co() {
    assert_eq!(
        eval_string(r#"stringify(take(1, 2, 3, 4, 2))"#),
        "(1, 2)"
    );
}

#[test]
fn take_single_bracket_bucket_keeps_arrayref_atom_bug_co() {
    assert_eq!(
        eval_string(r#"stringify(take([1, 2, 3, 4], 2))"#),
        "[1, 2, 3, 4]"
    );
}

#[test]
fn product_variadic_three_co() {
    assert_eq!(eval_string(r#"sprintf("%.0f", product(2, 3, 4))"#), "24");
}

#[test]
fn product_lone_bracket_bucket_zero_bug_co() {
    assert_eq!(eval_string(r#"sprintf("%.0f", product([2, 3, 4]))"#), "0");
}

#[test]
fn product_flatten_bracket_array_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", product(flatten([2, 3, 4])))"#),
        "24"
    );
}

#[test]
fn intersperse_val_inserts_sep_co() {
    assert_eq!(
        eval_string(r#"stringify(intersperse_val(0, [1, 2, 3]))"#),
        "(1, 0, 2, 0, 3)"
    );
}

#[test]
fn minmax_three_co() {
    assert_eq!(
        eval_string(r#"stringify(minmax([3, 1, 4]))"#),
        "(1, 4)"
    );
}

#[test]
fn zscore_above_mean_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", zscore(10, [2, 4, 6, 8]))"#),
        "2.23607"
    );
}

#[test]
fn rank_dense_rank_orders_co() {
    assert_eq!(
        eval_string(r#"stringify(rank([30, 10, 20]))"#),
        "(3, 1, 2)"
    );
    assert_eq!(
        eval_string(r#"stringify(dense_rank([30, 10, 20, 10]))"#),
        "(3, 1, 2, 1)"
    );
}

#[test]
fn partition_all_size_two_co() {
    assert_eq!(
        eval_string(r#"stringify(partition_all(2, [1, 2, 3, 4, 5]))"#),
        "([1, 2], [3, 4], [5])"
    );
}

#[test]
fn cartesian_product_two_lists_co() {
    assert_eq!(
        eval_string(r#"stringify(cartesian_product([1, 2], [10, 20]))"#),
        "([1, 10], [1, 20], [2, 10], [2, 20])"
    );
}

#[test]
fn cartesian_power_two_co() {
    assert_eq!(
        eval_string(r#"stringify(cartesian_power([1, 2], 2))"#),
        "([1, 1], [1, 2], [2, 1], [2, 2])"
    );
}

#[test]
fn outer_product_two_vectors_co() {
    assert_eq!(
        eval_string(r#"stringify(outer_product([1, 2], [10, 20]))"#),
        "((10, 20), (20, 40))"
    );
}

#[test]
fn clamp_value_to_bounds_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", clamp(0, 5, 10))"#),
        "5"
    );
}

#[test]
fn hypot_three_four_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", hypot(3, 4))"#),
        "5"
    );
}

#[test]
fn log2_and_log10_co() {
    assert_eq!(eval_string(r#"sprintf("%.6g", log2(8))"#), "3");
    assert_eq!(eval_string(r#"sprintf("%.6g", log10(100))"#), "2");
}

#[test]
fn sign_and_copysign_co() {
    assert_eq!(eval_string(r#"sprintf("%.0f", sign(-7))"#), "-1");
    assert_eq!(eval_string(r#"sprintf("%.0f", copysign(3, -1))"#), "-3");
}

#[test]
fn base64_roundtrip_ascii_co() {
    assert_eq!(
        eval_string(r#"base64_decode(base64_encode("hi"))"#),
        "hi"
    );
}

#[test]
fn indent_prefixes_each_line_co() {
    assert_eq!(
        eval_string(r#"stringify(indent("a\nb", "  "))"#),
        r#""  a\n  b""#
    );
}

#[test]
fn reverse_list_three_co() {
    assert_eq!(
        eval_string(r#"stringify(reverse_list([1, 2, 3]))"#),
        "(3, 2, 1)"
    );
}

#[test]
fn uniqstr_dedup_stable_co() {
    assert_eq!(
        eval_string(r#"stringify(uniqstr(qw(a a b)))"#),
        r#"("a", "b")"#
    );
}

#[test]
fn merge_hash_merges_keys_co() {
    assert_eq!(
        eval_string(r#"stringify(merge_hash({ a => 1 }, { b => 2 }))"#),
        r#"+{a => 1, b => 2}"#
    );
}

#[test]
fn invert_hash_bijection_co() {
    assert_eq!(
        eval_string(r#"stringify(invert({ a => 1, b => 2 }))"#),
        r#"+{1 => "a", 2 => "b"}"#
    );
}

#[test]
fn keys_sorted_lexicographic_co() {
    assert_eq!(
        eval_string(r#"stringify(keys_sorted({ b => 2, a => 1 }))"#),
        r#"("a", "b")"#
    );
}

#[test]
fn enumerate_qw_triple_co() {
    assert_eq!(
        eval_string(r#"stringify(enumerate(qw(a b c)))"#),
        r#"([0, "a"], [1, "b"], [2, "c"])"#
    );
}

#[test]
fn pairwise_sliding_pairs_co() {
    assert_eq!(
        eval_string(r#"stringify(pairwise([1, 2, 3, 4]))"#),
        "([1, 2], [2, 3], [3, 4])"
    );
}

#[test]
fn cumprod_three_co() {
    assert_eq!(
        eval_string(r#"stringify(cumprod([2, 3, 4]))"#),
        "(2, 6, 24)"
    );
}

#[test]
fn entropy_uniform_four_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", entropy([0.25, 0.25, 0.25, 0.25]))"#),
        "1.38629"
    );
}

#[test]
fn dot_product_two_by_two_co() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", dot_product([1, 2], [3, 4]))"#),
        "11"
    );
}

#[test]
fn combinations_two_of_three_co() {
    assert_eq!(
        eval_string(r#"stringify(combinations(2, [1, 2, 3]))"#),
        "([1, 2], [1, 3], [2, 3])"
    );
}

#[test]
fn permutations_two_of_three_co() {
    assert_eq!(
        eval_string(r#"stringify(permutations(2, [1, 2, 3]))"#),
        "([1, 2], [1, 3], [2, 1], [2, 3], [3, 1], [3, 2])"
    );
}
