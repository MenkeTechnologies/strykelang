//! Behavior-pinning batch CI (2026-05-10): pairwise / sliding windows (`window_n`, `adjacent_pairs`,
//! `sliding_pairs`), Cartesian / zips (`zip_shortest`, `zip_all`, `cartesian_product`, `transpose`),
//! RLE helpers (`run_length_encode*` / `rld`), `take_n`/`rotate`/`swap_pairs`, `prepend`/`append_elem`/
//! `contains_elem`/`index_of_elem`, `mean_list`/`min_list`/`max_list`/`span`/`product_list`, `pairs`,
//! `inits`/`tails`, `list_count`; iterator pins (`enumerate`, `chunk`, `dedup`) vs `range`; documents
//! **`StrykeValue::to_list` / `flatten_args` / iterator helpers** footguns (**BUG-143**),
//! **`transpose` nested single-arg** (**BUG-144**), **`unzip_pairs(zip(...))`** (**BUG-145**).

use crate::common::*;

#[test]
fn pairwise_sliding_four_integers_ci() {
    assert_eq!(
        eval_string(r#"stringify(pairwise([1, 2, 3, 4]))"#),
        "([1, 2], [2, 3], [3, 4])"
    );
}

#[test]
fn window_n_three_width_ci() {
    assert_eq!(
        eval_string(r#"stringify(window_n(3, [1, 2, 3, 4]))"#),
        "([1, 2, 3], [2, 3, 4])"
    );
}

#[test]
fn adjacent_pairs_matches_sliding_pairs_ci() {
    assert_eq!(
        eval_string(
            r#"(stringify(adjacent_pairs([5, 6, 7])) eq stringify(sliding_pairs([5, 6, 7]))) ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn transpose_two_row_arguments_column_major_ci() {
    assert_eq!(
        eval_string(r#"stringify(transpose([1, 2], [3, 4]))"#),
        "([1, 3], [2, 4])"
    );
}

#[test]
fn transpose_single_nested_outer_array_clusters_rows_bug_ci() {
    assert_eq!(
        eval_string(r#"stringify(transpose([[1, 2], [3, 4]]))"#),
        "([[1, 2]], [[3, 4]])"
    );
}

#[test]
fn zip_shortest_truncates_ci() {
    assert_eq!(
        eval_string(r#"stringify(zip_shortest([1, 2, 3], [10, 20]))"#),
        "([1, 10], [2, 20])"
    );
}

#[test]
fn zip_all_multi_list_shortest_ci() {
    assert_eq!(
        eval_string(r#"stringify(zip_all([1, 2], [10, 20, 99], ["a"]))"#),
        "[1, 10, \"a\"]"
    );
}

#[test]
fn cartesian_product_two_small_lists_ci() {
    assert_eq!(
        eval_string(r#"stringify(cartesian_product([0, 1], [9, 8]))"#),
        "([0, 9], [0, 8], [1, 9], [1, 8])"
    );
}

#[test]
fn product_list_of_three_factors_ci() {
    assert_eq!(
        eval_string(r#"sprintf("%.13g", product_list([2, 3, 4]))"#),
        "24"
    );
}

#[test]
fn unzip_interleaved_quad_ci() {
    assert_eq!(
        eval_string(r#"stringify(unzip(1, 10, 2, 11))"#),
        "([1, 2], [10, 11])"
    );
}

#[test]
fn unzip_pairs_explicit_pair_rows_ci() {
    assert_eq!(
        eval_string(r#"stringify(unzip_pairs([[1, 9], [2, 8]]))"#),
        "([1, 2], [9, 8])"
    );
}

#[test]
fn unzip_pairs_after_zip_over_flattens_to_scalars_bug_ci() {
    assert_eq!(
        eval_string(r#"stringify(unzip_pairs(zip([1, 2], [9, 8])))"#),
        "([1, 9, 2, 8], [undef, undef, undef, undef])"
    );
}

#[test]
fn run_length_encode_three_runs_ci() {
    assert_eq!(
        eval_string(r#"stringify(run_length_encode([qw(a a b a)]))"#),
        r#"(["a", 2], ["b", 1], ["a", 1])"#
    );
}

#[test]
fn run_length_decode_inverts_builtin_encode_ci() {
    assert_eq!(
        eval_string(r#"stringify(rld(run_length_encode([qw(x x x y)])))"#),
        r#"("x", "x", "x", "y")"#
    );
}

#[test]
fn run_length_encode_str_counts_ci() {
    assert_eq!(
        eval_string(r#"sprintf("%s", run_length_encode_str("aabbc"))"#),
        "2a2b1c"
    );
}

#[test]
fn prepend_and_append_elem_ci() {
    assert_eq!(
        eval_string(
            r#"(stringify(prepend(0, [1, 2])) eq "(0, 1, 2)" && stringify(append_elem(99, [1, 2])) eq "(1, 2, 99)") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn contains_elem_and_index_with_string_lhs_ci() {
    assert_eq!(
        eval_string(
            r#"(contains_elem("x", [qw(a x c)]) eq "1" && sprintf("%.0f", index_of_elem("x", [qw(a x c)])) eq "1") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn mean_list_numeric_ci() {
    assert_eq!(
        eval_string(r#"sprintf("%.12g", mean_list([2, 4, 8]))"#),
        "4.66666666667"
    );
}

#[test]
fn min_list_max_list_span_three_ci() {
    assert_eq!(
        eval_string(
            r#"(sprintf("%.13g", min_list([5, 2, 9])) eq "2" && sprintf("%.13g", max_list([3, 10, -1])) eq "10" && sprintf("%.13g", span([1, 9, 4])) eq "8") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn pairs_variadic_three_args_yields_one_pair_ci() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", scalar(pairs(1, 2, 3)))"#),
        "1"
    );
}

#[test]
fn pairs_variadic_four_args_yields_two_pairs_ci() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", scalar(pairs(1, 2, 3, 4)))"#),
        "2"
    );
}

#[test]
fn inits_prefixes_ci() {
    assert_eq!(
        eval_string(r#"stringify(inits([10, 11, 12]))"#),
        "([], [10], [10, 11], [10, 11, 12])"
    );
}

#[test]
fn tails_suffixes_ci() {
    assert_eq!(
        eval_string(r#"stringify(tails([10, 11, 12]))"#),
        "([10, 11, 12], [11, 12], [12], [])"
    );
}

#[test]
fn list_count_one_level_flatten_ci() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", list_count([1, [2, 3]]))"#),
        "2"
    );
}

#[test]
fn take_n_first_two_ci() {
    assert_eq!(eval_string(r#"stringify(take_n(2, [1, 2, 3]))"#), "(1, 2)");
}

#[test]
fn rotate_left_one_ci() {
    assert_eq!(
        eval_string(r#"stringify(rotate(1, [1, 2, 3]))"#),
        "(2, 3, 1)"
    );
}

#[test]
fn swap_pairs_quad_ci() {
    assert_eq!(
        eval_string(r#"stringify(swap_pairs(1, 2, 3, 4))"#),
        "(2, 1, 4, 3)"
    );
}

#[test]
fn head_trailing_count_variadic_operands_ci() {
    assert_eq!(eval_string(r#"stringify(head(1, 2, 3, 2))"#), "(1, 2)");
}

#[test]
fn tail_trailing_count_variadic_operands_ci() {
    assert_eq!(eval_string(r#"stringify(tail(1, 2, 3, 2))"#), "(2, 3)");
}

#[test]
fn drop_trailing_count_variadic_skips_leading_ci() {
    assert_eq!(eval_string(r#"stringify(drop(1, 2, 3, 1))"#), "(2, 3)");
}

#[test]
fn head_single_arrayref_with_trailing_count_sees_one_slot_ci() {
    assert_eq!(eval_string(r#"stringify(head([1, 2, 3], 2))"#), "[1, 2, 3]");
}

#[test]
fn enumerate_iterator_range_ci() {
    assert_eq!(
        eval_string(r#"stringify(flatten(enumerate(range(1, 3))))"#),
        "([0, 1], [1, 2], [2, 3])"
    );
}

#[test]
fn enumerate_single_array_bucket_yields_one_indexed_row_ci() {
    assert_eq!(
        eval_string(r#"stringify(flatten(enumerate([qw(a b)])))"#),
        r#"[0, ["a", "b"]]"#
    );
}

#[test]
fn chunk_with_range_iterator_emits_size_n_windows_ci() {
    assert_eq!(
        eval_string(r#"stringify(flatten(chunk(2, range(1, 5))))"#),
        "([1, 2], [3, 4], [5])"
    );
}

#[test]
fn chunk_single_array_bucket_yields_one_oversized_cell_ci() {
    assert_eq!(
        eval_string(r#"stringify(flatten(chunk(2, [1, 2, 3, 4, 5])))"#),
        "[[1, 2, 3, 4, 5]]"
    );
}

#[test]
fn dedup_variadic_consecutive_merge_ci() {
    assert_eq!(
        eval_string(r#"stringify(flatten(dedup(1, 1, 2, 3, 3)))"#),
        "(1, 2, 3)"
    );
}

#[test]
fn dedup_single_array_bucket_does_not_inspect_innards_ci() {
    assert_eq!(
        eval_string(r#"stringify(flatten(dedup([1, 1, 2, 3, 3])))"#),
        "[1, 1, 2, 3, 3]"
    );
}

#[test]
fn mesh_shortest_two_lists_ci() {
    assert_eq!(
        eval_string(r#"stringify(mesh_shortest([1, 2, 3], [9, 8]))"#),
        "(1, 9, 2, 8)"
    );
}

#[test]
fn zip_longest_fills_undef_ci() {
    assert_eq!(
        eval_string(r#"stringify(zip_longest([1], [9, 8]))"#),
        "([1, 9], [undef, 8])"
    );
}

#[test]
fn cartesian_power_square_ci() {
    assert_eq!(
        eval_string(r#"stringify(cartesian_power([0, 1], 2))"#),
        "([0, 0], [0, 1], [1, 0], [1, 1])"
    );
}

#[test]
fn pairwise_iter_over_three_flatten_ci() {
    assert_eq!(
        eval_string(r#"stringify(flatten(pairwise_iter([1, 2, 3])))"#),
        "(1, 2, 2, 3)"
    );
}

#[test]
fn repeat_stringifies_run_scalar_ci() {
    assert_eq!(eval_string(r#"sprintf("%s", repeat(7, 3))"#), "777");
}

#[test]
fn repeat_elem_numeric_triple_ci() {
    assert_eq!(eval_string(r#"stringify(repeat_elem(7, 3))"#), "(7, 7, 7)");
}

#[test]
fn take_n_cycle_iterator_yields_empty_today_bug_ci() {
    assert_eq!(
        eval_string(r#"stringify(take_n(6, cycle([1, 2, 3])))"#),
        "()"
    );
}

#[test]
fn before_n_two_ci() {
    assert_eq!(
        eval_string(r#"stringify(before_n(2, [9, 8, 7, 6]))"#),
        "(9, 8)"
    );
}

#[test]
fn after_n_skips_two_ci() {
    assert_eq!(
        eval_string(r#"stringify(after_n(2, [9, 8, 7, 6]))"#),
        "(7, 6)"
    );
}

#[test]
fn pairkeys_pairvalues_variadic_ci() {
    assert_eq!(
        eval_string(
            r#"(stringify(pairkeys(1, 10, 2, 20)) eq "(1, 2)" && stringify(pairvalues(1, 10, 2, 20)) eq "(10, 20)") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn rev_three_integers_ci() {
    assert_eq!(eval_string(r#"stringify(rev([3, 1, 2]))"#), "[2, 1, 3]");
}

#[test]
fn count_string_codepoints_ci() {
    assert_eq!(eval_string(r#"sprintf("%.0f", count("αβγ"))"#), "3");
}

#[test]
fn count_nested_array_one_level_ci() {
    assert_eq!(eval_string(r#"sprintf("%.0f", count([1, [2, 3]]))"#), "2");
}

#[test]
fn minmax_numeric_pair_ci() {
    assert_eq!(eval_string(r#"stringify(minmax([3, 9, 1]))"#), "(1, 9)");
}

#[test]
fn partition_two_even_length_ci() {
    assert_eq!(
        eval_string(r#"stringify(partition_two([1, -2, 3, -4]))"#),
        "([1, -2], [3, -4])"
    );
}

#[test]
fn windowed_circular_pair_windows_ci() {
    assert_eq!(
        eval_string(r#"stringify(windowed_circular(2, [1, 2, 3]))"#),
        "([1, 2], [2, 3], [3, 1])"
    );
}

#[test]
fn array_intersection_pair_order_ci() {
    assert_eq!(
        eval_string(
            r#"(len(array_intersection([1, 2, 3], [2, 9, 1])) == 2 && stringify(array_intersection([1, 2, 3], [2, 9, 1])) eq "(\"1\", \"2\")") ? "1" : "0""#
        ),
        "1"
    );
}
