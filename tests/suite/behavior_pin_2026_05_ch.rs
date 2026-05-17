//! Behavior-pinning batch CH (2026-05-09): list transforms (`flatten*` / `chain_from` / `interleave`),
//! frequency maps (`frequencies`, `pfrequencies` parallel path), **`normalize` / `normalize_list`**
//! (extra “output range” args pinned as **BUG-139**), **`clamp` vs `clamp_list` order** (**BUG-138**),
//! stats (`stddev`, `variance`, `avg`, `mean`, `median`, `softmax`), `first_or`, `compact`,
//! `squared`/`cubed`, `sum`/`sum0`/`product` + `sum_list` (**BUG-140**), `uniq` (**BUG-126**),
//! `chain_from` variadic vs nested-arg **`ARRAYREF`** pitfall (**BUG-142**),
//! gcd/lcm/min2/max2/sign/negate, `zip`, `expt`, `sq`/`cb` aliases.

use crate::common::*;

#[test]
fn flatten_nested_array_one_level_only_ch() {
    assert_eq!(
        eval_string(r#"stringify(flatten([1, [2, [3]]]))"#),
        "(1, [2, [3]])"
    );
}

#[test]
fn flatten_once_explicit_outer_level_ch() {
    assert_eq!(
        eval_string(r#"stringify(flatten_once([1, [2, 3]]))"#),
        "(1, [2, 3])"
    );
}

#[test]
fn flatten_deep_fully_expands_nested_ch() {
    assert_eq!(
        eval_string(r#"stringify(flatten_deep([1, [2, [3]]]))"#),
        "(1, 2, 3)"
    );
}

#[test]
fn flatten_scalar_context_returns_element_count_ch() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", 0 + flatten([1, [2, [3]]]))"#),
        "2"
    );
}

#[test]
fn interleave_round_robin_two_lists_ch() {
    assert_eq!(
        eval_string(r#"stringify(interleave([10, 30], [20, 40]))"#),
        "(10, 20, 30, 40)"
    );
}

#[test]
fn interleave_uneven_truncates_tail_per_column_ch() {
    assert_eq!(
        eval_string(r#"stringify(interleave(["x", "y", "z"], [10, 11]))"#),
        r#"("x", 10, "y", 11, "z")"#
    );
}

#[test]
fn interleave_scalar_context_element_count_ch() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", 0 + interleave([10, 11], [99]))"#),
        "3"
    );
}

#[test]
fn frequencies_whole_string_counts_as_one_key_ch() {
    assert_eq!(
        eval_string(r#"stringify(frequencies("aab"))"#),
        "+{aab => 1}"
    );
}

#[test]
fn frequencies_chars_mississippi_counts_ch() {
    assert_eq!(
        eval_string(
            r#"my $h = frequencies(chars("mississippi")); ($h->{m} == 1 && $h->{i} == 4 && $h->{s} == 4 && $h->{p} == 2) ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn frequencies_chars_aab_two_keys_ch() {
    assert_eq!(
        eval_string(r#"stringify(frequencies(chars("aab")))"#),
        "+{a => 2, b => 1}"
    );
}

#[test]
fn pfrequencies_matches_frequencies_large_multiset_parallel_path_ch() {
    assert_eq!(
        eval_string(
            r#"my @m = map { int($_ % 7) } iota_range(65536); my $sf = stringify(frequencies(\@m)); my $sp = stringify(pfrequencies(\@m)); ($sf eq $sp) ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn normalize_two_numeric_values_unit_range_ch() {
    assert_eq!(eval_string(r#"stringify(normalize([3, 9]))"#), "(0, 1)");
}

#[test]
fn normalize_list_variadic_matches_array_bucket_ch() {
    assert_eq!(
        eval_string(
            r#"(stringify(normalize_list(3, 9)) eq stringify(normalize([3, 9]))) ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn normalize_extra_leading_scalars_folded_into_source_strip_ch() {
    assert_eq!(
        eval_string(r#"stringify(normalize(10, 110, 5, 15, 25))"#),
        "(0.0476190476190476, 1, 0, 0.0952380952380952, 0.19047619047619)"
    );
}

#[test]
fn clamp_min_max_then_values_tuple_ch() {
    assert_eq!(
        eval_string(r#"stringify(clamp(0, 3, -1, 5, 2))"#),
        "(0, 3, 2)"
    );
}

#[test]
fn clamp_list_explicit_vector_form_ch() {
    assert_eq!(
        eval_string(r#"stringify(clamp_list(0, 3, -1, 5, 2))"#),
        "(0, 3, 2)"
    );
}

#[test]
fn clamp_wrong_shape_list_first_reads_min_from_first_element_ch() {
    assert_eq!(eval_string(r#"stringify(clamp([-1, 5, 2], 0, 3))"#), "0");
}

#[test]
fn stddev_population_five_consecutive_ints_ch() {
    assert_eq!(
        eval_string(r#"sprintf("%.12g", stddev([1, 2, 3, 4, 5]))"#),
        "1.41421356237"
    );
}

#[test]
fn variance_population_pair_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", variance([1, 3]))"#), "1");
}

#[test]
fn avg_three_uniform_step_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", avg([10, 20, 30]))"#), "20");
}

#[test]
fn mean_nested_array_averages_elements_ch() {
    assert_eq!(
        eval_string(r#"sprintf("%.12g", mean([2, 4, 8]))"#),
        "4.66666666667"
    );
}

#[test]
fn median_even_length_linear_interpolation_ch() {
    assert_eq!(
        eval_string(r#"sprintf("%.13g", median([1, 2, 3, 4]))"#),
        "2.5"
    );
}

#[test]
fn first_or_skips_undef_empty_string_zero_ch() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", first_or([undef, "", 0], 99))"#),
        "99"
    );
}

#[test]
fn softmax_uniform_triple_is_thirds_ch() {
    assert_eq!(
        eval_string(r#"stringify(softmax([0, 0, 0]))"#),
        "[0.333333333333333, 0.333333333333333, 0.333333333333333]"
    );
}

#[test]
fn squared_three_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", squared(3))"#), "9");
}

#[test]
fn squared_variadic_second_operand_ignored_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", squared(3, 4))"#), "9");
}

#[test]
fn sq_alias_matches_squared_ch() {
    assert_eq!(
        eval_string(r#"(sprintf("%.13g", squared(5)) eq sprintf("%.13g", sq(5))) ? "1" : "0""#),
        "1"
    );
}

#[test]
fn cubed_two_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", cubed(2))"#), "8");
}

#[test]
fn cubed_variadic_second_operand_ignored_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", cubed(2, 9))"#), "8");
}

#[test]
fn cb_alias_matches_cubed_ch() {
    assert_eq!(
        eval_string(r#"(sprintf("%.13g", cubed(4)) eq sprintf("%.13g", cb(4))) ? "1" : "0""#),
        "1"
    );
}

#[test]
fn expt_cubes_three_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", expt(3, 3))"#), "27");
}

#[test]
fn join_compact_filters_undef_and_blank_ch() {
    assert_eq!(eval_string(r#"join(",", compact(undef, "", 0, 7))"#), "0,7");
}

#[test]
fn chain_from_variadic_top_level_lists_concat_ch() {
    assert_eq!(
        eval_string(r#"stringify(chain_from([1, 2], [3], [], [4]))"#),
        "(1, 2, 3, 4)"
    );
}

#[test]
fn chain_from_single_outer_arrayref_leaves_inner_lists_unmerged_bug_ch() {
    assert_eq!(
        eval_string(r#"stringify(chain_from([[1, 2], [3], [], [4]]))"#),
        "([1, 2], [3], [], [4])"
    );
}

#[test]
fn zip_pairs_two_lists_ch() {
    assert_eq!(
        eval_string(r#"stringify(zip([1, 2], [10, 11]))"#),
        "([1, 10], [2, 11])"
    );
}

#[test]
fn gcd_example_ch() {
    assert_eq!(eval_string(r#"sprintf("%.0f", gcd(84, 132))"#), "12");
}

#[test]
fn lcm_example_ch() {
    assert_eq!(eval_string(r#"sprintf("%.0f", lcm(21, 14))"#), "42");
}

#[test]
fn min2_and_max2_numeric_ch() {
    assert_eq!(
        eval_string(
            r#"(sprintf("%.0f", min2(9, 4)) eq "4" && sprintf("%.0f", max2(-2, 8)) eq "8") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn sign_negative_nonzero_ch() {
    assert_eq!(eval_string(r#"sprintf("%.0f", sign(-7.3))"#), "-1");
}

#[test]
fn negate_float_ch() {
    assert_eq!(eval_string(r#"sprintf("%.0f", negate(5))"#), "-5");
}

#[test]
fn sum_variadic_two_addends_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", sum(10, 11))"#), "21");
}

#[test]
fn sum_single_inline_array_works_ch() {
    // BUG-109/140 FIXED: sum auto-derefs arrayrefs
    assert_eq!(eval_string(r#"sprintf("%.13g", sum([10, 11]))"#), "21");
}

#[test]
fn sum_list_reads_array_contents_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", sum_list([10, 11]))"#), "21");
}

#[test]
fn sum0_empty_is_zero_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", sum0())"#), "0");
}

#[test]
fn product_variadic_two_factors_ch() {
    assert_eq!(eval_string(r#"sprintf("%.13g", product(6, 7))"#), "42");
}

#[test]
fn product_single_inline_array_works_ch() {
    // BUG-109/140 FIXED: product auto-derefs arrayrefs
    assert_eq!(eval_string(r#"sprintf("%.13g", product([6, 7]))"#), "42");
}

#[test]
fn uniq_variadic_deduplicates_neighbors_ch() {
    assert_eq!(eval_string(r#"stringify(uniq 1, 2, 2, 3)"#), "(1, 2, 3)");
}

#[test]
fn uniq_single_array_bucket_dereferenced_ch() {
    // BUG-126/140 fix (2026-05-15): `uniq([1, 2, 2, 3])` now derefs the
    // single arrayref argument and yields a flat uniq list. Previously
    // the arrayref was treated as one atom and returned unchanged
    // (`[1, 2, 2, 3]`). Fix in `strykelang/list_builtins.rs::uniq_list`
    // — added an `as_array_ref` branch alongside `as_array_vec`. Full
    // regression coverage in
    // `tests/suite/library_fixes_2026_05.rs::uniq_*`.
    assert_eq!(eval_string(r#"stringify(uniq([1, 2, 2, 3]))"#), "(1, 2, 3)");
}
