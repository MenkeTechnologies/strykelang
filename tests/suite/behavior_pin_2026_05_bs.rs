//! Behavior-pinning batch BS (2026-05-08): list transforms (take/drop predicates, pairwise, window_n,
//! inits/tails), grouping (group_by chunk_while batched chunk_n unzip transpose), hashing/mapping
//! (uniq_by merge_sorted sum_by flat_map enumerate), deltas (adjacent_difference diff), RLE,
//! slicing (slice_when), numeric stats (variance harmonic geometric median quantile), regex helpers
//! (match_all capture_groups), small utilities (times_fn step then_fn binary_insert).

use crate::common::*;

#[test]
fn drop_while_take_while_bs() {
    assert_eq!(
        eval_string(r#"join(",", drop_while { $_ < 3 } (1, 2, 3, 4, 5))"#),
        "3,4,5"
    );
    assert_eq!(
        eval_string(r#"join(",", take_while { $_ < 4 } (1, 5, 2, 6))"#),
        "1"
    );
}

#[test]
fn reject_predicate_bs() {
    assert_eq!(
        eval_string(r#"join(",", reject { $_ % 2 == 0 } (10, 11, 12, 13))"#),
        "11,13"
    );
}

#[test]
fn inits_tails_bs() {
    assert_eq!(
        eval_string(r#"stringify(inits(1, 2, 3))"#),
        "([], [1], [1, 2], [1, 2, 3])"
    );
    assert_eq!(
        eval_string(r#"stringify(tails(10, 20, 30))"#),
        "([10, 20, 30], [20, 30], [30], [])"
    );
}

#[test]
fn group_by_key_bs() {
    assert_eq!(
        eval_string(r#"stringify(group_by { int($_ / 3) } (1, 2, 5, 7, 8))"#),
        "([1, 2], [5], [7, 8])"
    );
}

#[test]
fn uniq_by_key_fn_bs() {
    assert_eq!(
        eval_string(r#"join(",", uniq_by sub { int($_[0] / 10) }, (5, 14, 23, 31))"#),
        "5,14,23,31"
    );
}

#[test]
fn merge_sorted_pair_bs() {
    assert_eq!(
        eval_string(r#"stringify(merge_sorted([1, 9, 90], [2, 8, 100]))"#),
        "(1, 2, 8, 9, 90, 100)"
    );
}

#[test]
fn pairwise_zip_pairs_bs() {
    assert_eq!(
        eval_string(r#"stringify(pairwise (1, 2, 3, 4, 5, 6))"#),
        "([1, 2], [2, 3], [3, 4], [4, 5], [5, 6])"
    );
}

#[test]
fn window_sliding_three_bs() {
    assert_eq!(
        eval_string(r#"stringify(window_n(3, -1, 0, 1, 2))"#),
        "([-1, 0, 1], [0, 1, 2])"
    );
}

#[test]
fn binary_insert_sorted_bs() {
    assert_eq!(
        eval_string(r#"join(",", binary_insert(15, [10, 20, 25, 30]))"#),
        "10,15,20,25,30"
    );
}

#[test]
fn run_length_encode_plain_bs() {
    assert_eq!(
        eval_string(r#"stringify(run_length_encode(1, 1, 2, 3, 3))"#),
        r#"(["1", 2], ["2", 1], ["3", 2])"#
    );
}

#[test]
fn adjacent_difference_diff_neighbors_bs() {
    assert_eq!(
        eval_string(r#"stringify(adjacent_difference(100, 90, 95, 100))"#),
        "(-10, 5, 5)"
    );
    assert_eq!(
        eval_string(r#"stringify(diff([1, 10, 9, 12]))"#),
        "(9, -1, 3)"
    );
    assert_eq!(
        eval_string(r#"stringify(adjacent_pairs(7, 8, 9))"#),
        "([7, 8], [8, 9])"
    );
}

#[test]
fn unzip_alternating_bs() {
    assert_eq!(
        eval_string(r#"stringify(unzip("a", 1, "b", 2, "c", 3))"#),
        r##"(["a", "b", "c"], [1, 2, 3])"##
    );
}

#[test]
fn combinations_pick_two_bs() {
    assert_eq!(eval_string(r#"scalar(combinations(2, [1, 2, 3, 4]))"#), "6");
}

#[test]
fn sum_by_squares_bs() {
    assert_eq!(eval_int(r#"int(sum_by sub { $_[0] * $_[0] }, (3, 4))"#), 25);
}

#[test]
fn flat_map_fn_square_bs() {
    assert_eq!(
        eval_string(r#"join(",", flat_map_fn sub { $_[0] * $_[0] }, (6, -3))"#),
        "36,9"
    );
}

#[test]
fn enumerate_array_bs() {
    assert_eq!(
        eval_string(r#"stringify(enumerate(qw/foo bar baz/))"#),
        r##"([0, "foo"], [1, "bar"], [2, "baz"])"##
    );
}

#[test]
fn transpose_two_rows_bs() {
    assert_eq!(
        eval_string(r#"stringify(transpose([1, 80], [2, 90], [3, 100]))"#),
        "([1, 2, 3], [80, 90, 100])"
    );
}

#[test]
fn chunk_n_fixed_bs() {
    assert_eq!(
        eval_string(r#"stringify(chunk_n(3, 1..10))"#),
        "([1, 2, 3], [4, 5, 6], [7, 8, 9], [10])"
    );
}

#[test]
fn batched_slices_bs() {
    assert_eq!(eval_string(r#"scalar(batched(3, 10, 20, 30, 40))"#), "2");
}

#[test]
fn batched_batches_stringify_bs() {
    assert_eq!(
        eval_string(r#"stringify(batched(3, 100, 200, 300, 400, 500, 601))"#),
        "([100, 200, 300], [400, 500, 601])"
    );
}

#[test]
fn chunk_while_bucket_bs() {
    assert_eq!(
        eval_string(
            r#"my @c = chunk_while sub { int($_[0] / 3) eq int($_[1] / 3) }, (10, 11, 29, 31);
               stringify(\@c)"#
        ),
        "[[10, 11], [29], [31]]"
    );
}

#[test]
fn slice_when_sign_bs() {
    assert_eq!(
        eval_string(
            r#"my @r = slice_when sub { $_[0] < 0 && $_[1] >= 0 }, (-2, -1, 0, 1);
               stringify(\@r)"#
        ),
        "[[-2, -1], [0, 1]]"
    );
}

#[test]
fn sliding_pairs_overlap_bs() {
    assert_eq!(
        eval_string(r#"stringify(sliding_pairs(0, -1, 2))"#),
        "([0, -1], [-1, 2])"
    );
}

#[test]
fn variance_medians_bs() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", variance(10, 20, 30))"#),
        "66.6667"
    );
    assert_eq!(eval_int(r#"median(4, 1, 10, 2)"#), 3);
    assert_eq!(
        eval_string(r#"sprintf("%.6f", quantile(0.25, 2, 4, 6, 8, 100))"#),
        "8.000000"
    );
}

#[test]
fn harmonic_geometric_mean_bs() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", harmonic_mean(1, 2, 4))"#),
        "1.714286"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.6f", geometric_mean(1, 3, 9))"#),
        "3.000000"
    );
}

#[test]
fn regex_match_capture_bs() {
    assert_eq!(
        eval_string(r#"join("|", match_all("x+", "axxxbcdxx"))"#),
        "xxx|xx"
    );
    assert_eq!(
        eval_string(r#"stringify(capture_groups("(\\w+)=(\\d+)", " zz=777 tail"))"#),
        r##"["zz=777", "zz", "777"]"##
    );
}

#[test]
fn times_fn_indices_bs() {
    assert_eq!(
        eval_string(r#"join(",", times_fn 5, sub { $_[0] * 2 })"#),
        "0,2,4,6,8"
    );
}

#[test]
fn step_range_float_bs() {
    assert_eq!(
        eval_string(r#"join(",", step(10, 19, 2.5))"#),
        "10,12.5,15,17.5"
    );
}

#[test]
fn then_fn_pipe_bs() {
    assert_eq!(eval_int(r#"int(then_fn sub { $_[0] + 7 }, (-4))"#), 3);
}

#[test]
fn partition_all_three_bs() {
    assert_eq!(eval_string(r#"scalar(partition_all(2, [1..7]))"#), "4");
}

#[test]
fn minmax_pair_bs() {
    assert_eq!(
        eval_string(r#"stringify(minmax(40, -9, 0, 8))"#),
        "(-9, 40)"
    );
}

#[test]
fn iota_short_bs() {
    assert_eq!(eval_string(r#"join(",", iota(3, 999))"#), "0,1,2");
}
