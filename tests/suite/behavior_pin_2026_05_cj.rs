//! Behavior-pinning batch CJ (2026-05-09): list glue (`concat`/`chain`, `split_at`, `interleave`,
//! `intersperse`/`riffle`, `interpose`, `mesh`/`mesh_longest`, `partition_n`, `partition_all`,
//! `batch`, `repeat_list`), combinatorics (`combinations`, `permutations` call shapes), numeric
//! reducers on lone **`ARRAYREF`** (**BUG-140**), **`concat` iterator buckets** (**BUG-148**),
//! window edge cases, and **`permutations([...])`** footgun (**BUG-147**).

use crate::common::*;

#[test]
fn concat_iterator_one_bucket_per_arrayref_arg_cj() {
    assert_eq!(
        eval_string(r#"stringify(concat([1, 2], [3], [4, 5]))"#),
        "([1, 2], [3], [4, 5])"
    );
}

#[test]
fn chain_from_three_lists_eager_flat_cj() {
    assert_eq!(
        eval_string(r#"stringify(chain_from([1, 2], [3], [4, 5]))"#),
        "(1, 2, 3, 4, 5)"
    );
}

#[test]
fn split_at_index_two_strings_cj() {
    assert_eq!(
        eval_string(r#"stringify(split_at(2, [qw(a b c d)]))"#),
        r#"(["a", "b"], ["c", "d"])"#
    );
}

#[test]
fn interleave_three_lists_round_robin_trailing_tail_cj() {
    assert_eq!(
        eval_string(r#"stringify(interleave([1, 2], [9, 8, 7], [0, 0]))"#),
        "(1, 9, 0, 2, 8, 0, 7)"
    );
}

#[test]
fn riffle_list_last_sep_cj() {
    assert_eq!(
        eval_string(r#"stringify(riffle([1, 2, 3], 0))"#),
        "(1, 0, 2, 0, 3)"
    );
}

#[test]
fn intersperse_list_comma_sep_matches_riffle_cj() {
    assert_eq!(
        eval_string(
            r#"(stringify(intersperse([1, 2, 3], 0)) eq stringify(riffle([1, 2, 3], 0))) ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn interpose_val_between_elems_cj() {
    assert_eq!(
        eval_string(r#"stringify(interpose(0, [1, 2, 3]))"#),
        "(1, 0, 2, 0, 3)"
    );
}

#[test]
fn mesh_shortest_truncates_longest_tail_cj() {
    assert_eq!(
        eval_string(r#"stringify(mesh_shortest([1, 2, 3], [9, 8]))"#),
        "(1, 9, 2, 8)"
    );
}

#[test]
fn mesh_two_lists_pads_shorter_with_undef_then_tail_cj() {
    assert_eq!(
        eval_string(r#"stringify(mesh([1, 2], [9, 8, 7]))"#),
        "(1, 9, 2, 8, undef, 7)"
    );
}

#[test]
fn mesh_longest_pads_shorter_with_undef_cj() {
    assert_eq!(
        eval_string(r#"stringify(mesh_longest([1, 2, 3], [9, 8]))"#),
        "(1, 9, 2, 8, 3, undef)"
    );
}

#[test]
fn transpose_three_row_operands_cj() {
    assert_eq!(
        eval_string(r#"stringify(transpose([1, 2], [3, 4], [5, 6]))"#),
        "([1, 3, 5], [2, 4, 6])"
    );
}

#[test]
fn partition_n_chunk_width_two_cj() {
    assert_eq!(
        eval_string(r#"stringify(partition_n(2, [1, 2, 3, 4, 5]))"#),
        "([1, 2], [3, 4], [5])"
    );
}

#[test]
fn partition_n_width_exceeds_length_one_group_cj() {
    assert_eq!(
        eval_string(r#"stringify(partition_n(5, [1, 2]))"#),
        "[1, 2]"
    );
}

#[test]
fn partition_all_fixed_chunks_width_two_cj() {
    assert_eq!(
        eval_string(r#"stringify(partition_all(2, [1, 2, 3, 4, 5]))"#),
        "([1, 2], [3, 4], [5])"
    );
}

#[test]
fn batch_alias_partition_n_two_cj() {
    assert_eq!(
        eval_string(
            r#"(stringify(batch(2, [1, 2, 3, 4])) eq stringify(partition_n(2, [1, 2, 3, 4]))) ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn merge_sorted_two_runs_cj() {
    assert_eq!(
        eval_string(r#"stringify(merge_sorted([1, 3, 5], [2, 4]))"#),
        "(1, 2, 3, 4, 5)"
    );
}

#[test]
fn combinations_k_two_three_choices_cj() {
    assert_eq!(
        eval_string(r#"stringify(combinations(2, [1, 2, 3]))"#),
        "([1, 2], [1, 3], [2, 3])"
    );
}

#[test]
fn permutations_k_equals_list_length_three_cj() {
    assert_eq!(
        eval_string(r#"stringify(permutations(3, [1, 2, 3]))"#),
        "([1, 2, 3], [1, 3, 2], [2, 1, 3], [2, 3, 1], [3, 1, 2], [3, 2, 1])"
    );
}

#[test]
fn permutations_single_arrayref_numifies_to_zero_empty_bug_cj() {
    assert_eq!(
        eval_string(r#"stringify(permutations([1, 2, 3]))"#),
        "()"
    );
}

#[test]
fn sliding_pairs_singleton_source_empty_cj() {
    assert_eq!(eval_string(r#"stringify(sliding_pairs([1]))"#), "()");
}

#[test]
fn window_n_wider_than_list_source_empty_cj() {
    assert_eq!(
        eval_string(r#"stringify(window_n(5, [1, 2, 3]))"#),
        "()"
    );
}

#[test]
fn repeat_list_duplicates_segment_cj() {
    assert_eq!(
        eval_string(r#"stringify(repeat_list(2, [1, 2]))"#),
        "(1, 2, 1, 2)"
    );
}

#[test]
fn split_string_delimiters_tuple_cj() {
    assert_eq!(
        eval_string(r#"stringify(split("-", "a-b-c"))"#),
        r#"("a", "b", "c")"#
    );
}

#[test]
fn sum_lone_inline_array_zero_today_bug_cj() {
    assert_eq!(eval_string(r#"sprintf("%.0f", sum([1, 2, 3]))"#), "0");
}

#[test]
fn product_lone_inline_array_zero_today_bug_cj() {
    assert_eq!(eval_string(r#"sprintf("%.0f", product([2, 3, 4]))"#), "0");
}
