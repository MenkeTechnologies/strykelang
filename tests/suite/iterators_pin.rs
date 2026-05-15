//! Iterator-builtin pins. take/drop/take_while/cycle/zip/etc are core
//! to stryke's batteries-included story. Pin the semantics so a
//! future iterator refactor preserves caller expectations.

use crate::common::*;

// ── take_n / drop_n ──────────────────────────────────────────────────

#[test]
fn take_n_returns_first_n_elements() {
    let code = r#"
        my @r = take_n(3, 1, 2, 3, 4, 5);
        join(",", @r) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn take_n_with_n_larger_than_input_returns_all() {
    let code = r#"
        my @r = take_n(100, 1, 2, 3);
        join(",", @r) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn take_n_zero_returns_empty() {
    let code = r#"
        my @r = take_n(0, 1, 2, 3);
        scalar(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn drop_n_removes_first_n_elements() {
    let code = r#"
        my @r = drop_n(2, 1, 2, 3, 4, 5);
        join(",", @r) eq "3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn drop_n_with_n_larger_than_input_returns_empty() {
    let code = r#"
        my @r = drop_n(100, 1, 2, 3);
        scalar(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── take_n on a cycle iterator ──────────────────────────────────────

#[test]
fn take_n_from_cycle_works_without_infinite_loop() {
    let code = r#"
        my @r = take_n(7, cycle(1, 2, 3));
        # 7 items from cycling (1,2,3): 1,2,3,1,2,3,1
        join(",", @r) eq "1,2,3,1,2,3,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── zip / unzip ─────────────────────────────────────────────────────

#[test]
fn zip_two_arrays_pairs_them() {
    let code = r#"
        my @r = zip([1, 2, 3], ["a", "b", "c"]);
        # Each output is a [num, str] pair.
        (scalar(@r) == 3
            && $r[0]->[0] == 1
            && $r[0]->[1] eq "a"
            && $r[2]->[0] == 3
            && $r[2]->[1] eq "c") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn zip_arrays_of_unequal_length_pads_to_longer() {
    // Stryke divergence: zip pads the shorter side with undef/empty
    // rather than stopping at the shorter (Perl `each_arrayref` style).
    // BUG-223: pinning observed behavior; List::Util `pairs` semantics
    // would stop at shorter.
    let code = r#"
        my @r = zip([1, 2, 3, 4, 5], ["a", "b"]);
        scalar(@r) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chunk / chunk_n ─────────────────────────────────────────────────
// Note: BUG-224 — `chunk(N, LIST)` and `chunk_n(LIST, N)` both return
// `[N]` (a single arrayref containing N) instead of N-sized groups.
// `chunked` (different builtin) is the working form; pin it.

#[test]
fn chunked_3_splits_into_groups_of_three() {
    // chunked takes a LIST (not arrayref) + N. Per builtin diagnostic.
    let code = r#"
        my @groups = chunked((1, 2, 3, 4, 5, 6, 7, 8, 9), 3);
        (scalar(@groups) == 3
            && $groups[0]->[0] == 1
            && $groups[2]->[2] == 9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sliding_pairs ────────────────────────────────────────────────────
// Note: `sliding_window` appears in %b but errors as "Undefined
// subroutine" (BUG-225). `sliding_pairs` (size=2 only) works.

#[test]
fn sliding_pairs_yields_overlapping_pairs() {
    let code = r#"
        my @pairs = sliding_pairs(1, 2, 3, 4, 5);
        # (1,2), (2,3), (3,4), (4,5) = 4 pairs.
        (scalar(@pairs) == 4
            && $pairs[0]->[0] == 1
            && $pairs[0]->[1] == 2
            && $pairs[3]->[1] == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── enumerate / pairs ───────────────────────────────────────────────

#[test]
fn enumerate_yields_index_value_pairs() {
    let code = r#"
        my @pairs = enumerate("a", "b", "c");
        # 3 pairs: [0,"a"], [1,"b"], [2,"c"].
        (scalar(@pairs) == 3
            && $pairs[0]->[0] == 0
            && $pairs[0]->[1] eq "a"
            && $pairs[2]->[0] == 2
            && $pairs[2]->[1] eq "c") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── reverse ─────────────────────────────────────────────────────────

#[test]
fn reverse_array_flips_order() {
    let code = r#"
        my @r = reverse(1, 2, 3, 4, 5);
        join(",", @r) eq "5,4,3,2,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reverse_empty_array_yields_empty() {
    // `reverse()` (no args) is a parse error (existing BUG-099).
    // Use `reverse @empty` form which works.
    let code = r#"
        my @empty;
        my @r = reverse @empty;
        scalar(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sort by custom comparator ───────────────────────────────────────

#[test]
fn sort_ascending_numerically() {
    let code = r#"
        my @r = sort { _0 <=> _1 } (3, 1, 4, 1, 5, 9, 2, 6);
        join(",", @r) eq "1,1,2,3,4,5,6,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sort_descending_numerically() {
    let code = r#"
        my @r = sort { _1 <=> _0 } (3, 1, 4, 1, 5, 9);
        join(",", @r) eq "9,5,4,3,1,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sort_alphabetically_with_cmp() {
    let code = r#"
        my @r = sort { _0 cmp _1 } ("delta", "alpha", "charlie", "bravo");
        join(",", @r) eq "alpha,bravo,charlie,delta" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── reduce ──────────────────────────────────────────────────────────

#[test]
fn reduce_sum_of_array() {
    let code = r#"
        reduce { _0 + _1 } 0, (1, 2, 3, 4, 5)
    "#;
    assert_eq!(eval_int(code), 15);
}

#[test]
fn reduce_max_of_array() {
    let code = r#"
        reduce { _0 > _1 ? _0 : _1 } 0, (3, 1, 4, 1, 5, 9, 2, 6)
    "#;
    assert_eq!(eval_int(code), 9);
}

#[test]
fn reduce_product_of_array() {
    let code = r#"
        reduce { _0 * _1 } 1, (1, 2, 3, 4, 5)
    "#;
    assert_eq!(eval_int(code), 120);
}

// ── uniq ─────────────────────────────────────────────────────────────

#[test]
fn uniq_removes_consecutive_duplicates() {
    let code = r#"
        my @r = uniq(1, 1, 2, 3, 3, 3, 4, 1);
        # uniq preserves order but dedups all occurrences.
        join(",", @r) eq "1,2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uniq_with_strings() {
    let code = r#"
        my @r = uniq("a", "b", "a", "c", "b", "a");
        join(",", @r) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── min / max ────────────────────────────────────────────────────────

#[test]
fn min_of_numbers() {
    let code = r#"
        min(3, 1, 4, 1, 5, 9, 2, 6) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn max_of_numbers() {
    let code = r#"
        max(3, 1, 4, 1, 5, 9, 2, 6) == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sum / avg ────────────────────────────────────────────────────────

#[test]
fn sum_of_numbers() {
    let code = r#"
        sum(1, 2, 3, 4, 5) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── first / find ─────────────────────────────────────────────────────

#[test]
fn first_matching_predicate() {
    let code = r#"
        my $r = first { $_ > 5 } (1, 3, 5, 7, 9);
        $r == 7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn first_no_match_returns_undef() {
    let code = r#"
        my $r = first { $_ > 100 } (1, 2, 3);
        defined($r) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── partition ────────────────────────────────────────────────────────

#[test]
fn partition_via_grep_split() {
    // BUG-059 (existing): `partition(sub {}, LIST)` returns empty.
    // Idiomatic workaround: grep twice with inverted predicate.
    let code = r#"
        my @nums = (1:10);
        my @evens = grep {  _ % 2 == 0 } @nums;
        my @odds  = grep { !(_ % 2 == 0) } @nums;
        (scalar(@evens) == 5 && scalar(@odds) == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pipe-forward composition ────────────────────────────────────────

#[test]
fn array_slice_for_first_n() {
    let code = r#"
        my @full = (1:20);
        my @r = @full[0:4];
        scalar(@r) == 5 && $r[0] == 1 && $r[4] == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
