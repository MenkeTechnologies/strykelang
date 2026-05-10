//! Behavior-pinning batch CK (2026-05-09): `without` / multiset / `zip_fill` / `outer_product`,
//! flatten tiers (`flatten_once`, `flatten_deep`), numeric list deltas (`diff`, `prefix_sums` / `cumsum`),
//! tail helpers (`take_last`, `drop_last`), `every_nth`, `compact`, `list_union` — plus
//! **`without([...], LIST)`** footgun (**BUG-149**) and multiset **stringify** order (**BUG-150**).

use crate::common::*;

#[test]
fn without_scalar_filters_by_string_equality_ck() {
    assert_eq!(eval_string(r#"stringify(without(2, [1, 2, 3]))"#), "(1, 3)");
}

#[test]
fn without_arrayref_first_compare_ref_display_no_drops_bug_ck() {
    assert_eq!(
        eval_string(r#"stringify(without([2, 9], [1, 2, 3, 9]))"#),
        "(1, 2, 3, 9)"
    );
}

#[test]
fn zip_fill_pairs_equal_length_ck() {
    assert_eq!(
        eval_string(r#"stringify(zip_fill(0, [1, 2], [9, 8]))"#),
        "([1, 9], [2, 8])"
    );
}

#[test]
fn zip_fill_pad_longer_second_list_ck() {
    assert_eq!(
        eval_string(r#"stringify(zip_fill(-1, [1], [9, 8, 7]))"#),
        "([1, 9], [-1, 8], [-1, 7])"
    );
}

#[test]
fn outer_product_two_numeric_vectors_ck() {
    assert_eq!(
        eval_string(r#"stringify(outer_product([1, 2], [10, 20]))"#),
        "((10, 20), (20, 40))"
    );
}

#[test]
fn flatten_once_peels_single_bracket_level_ck() {
    assert_eq!(
        eval_string(r#"stringify(flatten_once([[1, 2], [3]]))"#),
        "([1, 2], [3])"
    );
}

#[test]
fn flatten_deep_scalar_mix_ck() {
    assert_eq!(
        eval_string(r#"stringify(flatten_deep([[[1]], 2]))"#),
        "(1, 2)"
    );
}

#[test]
fn diff_successive_differences_ck() {
    assert_eq!(eval_string(r#"stringify(diff([1, 4, 9]))"#), "(3, 5)");
}

#[test]
fn prefix_sums_running_ck() {
    assert_eq!(
        eval_string(r#"stringify(prefix_sums([1, 2, 3]))"#),
        "(1, 3, 6)"
    );
}

#[test]
fn cumsum_matches_prefix_sums_ck() {
    assert_eq!(
        eval_string(
            r#"(stringify(cumsum([1, 2, 3])) eq stringify(prefix_sums([1, 2, 3]))) ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn list_union_two_lists_sorted_merge_ck() {
    assert_eq!(
        eval_string(r#"stringify(list_union([1, 2], [2, 3]))"#),
        "(\"1\", \"2\", \"3\")"
    );
}

#[test]
fn multiset_difference_sorted_join_counts_ck() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a cmp $b } multiset_difference([1, 1, 2, 3], [1, 2]))"#),
        "1,3"
    );
}

#[test]
fn multiset_intersection_sorted_join_counts_ck() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a cmp $b } multiset_intersection([1, 1, 2], [1, 2, 2]))"#),
        "1,2"
    );
}

#[test]
fn every_nth_two_with_list_operand_ck() {
    assert_eq!(
        eval_string(r#"stringify(every_nth(2, [10, 11, 12, 13, 14]))"#),
        "(10, 12, 14)"
    );
}

#[test]
fn compact_drops_undef_and_empty_string_keeps_zero_ck() {
    assert_eq!(
        eval_string(r#"stringify(compact(0, undef, 1, "", 2))"#),
        "(0, 1, 2)"
    );
}

#[test]
fn take_last_two_of_quad_ck() {
    assert_eq!(
        eval_string(r#"stringify(take_last(2, [1, 2, 3, 4]))"#),
        "(3, 4)"
    );
}

#[test]
fn drop_last_two_of_quad_ck() {
    assert_eq!(
        eval_string(r#"stringify(drop_last(2, [1, 2, 3, 4]))"#),
        "(1, 2)"
    );
}

#[test]
fn without_nth_removes_index_two_ck() {
    assert_eq!(
        eval_string(r#"stringify(without_nth(2, [qw(a b c d)]))"#),
        r#"("a", "b", "d")"#
    );
}
