//! Colon-range pins. `N:M` is stryke's idiomatic range form (replaces
//! Perl's `N..M`). Every demo uses it; pin the corners so a parser
//! refactor can't silently drop edge cases.

use crate::common::*;

// ── Basic ascending range ─────────────────────────────────────────────

#[test]
fn range_one_to_five_yields_five_elements() {
    let code = r#"
        my @r = (1:5);
        scalar(@r) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn range_one_to_five_is_inclusive() {
    let code = r#"
        my @r = (1:5);
        ($r[0] == 1 && $r[4] == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn range_zero_to_ten_yields_eleven() {
    let code = r#"
        my @r = (0:10);
        scalar(@r) == 11 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Single-element range ─────────────────────────────────────────────

#[test]
fn range_five_to_five_yields_single_element() {
    let code = r#"
        my @r = (5:5);
        (scalar(@r) == 1 && $r[0] == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Reversed range yields empty array (Perl semantics) ──────────────

#[test]
fn descending_range_is_empty() {
    let code = r#"
        my @r = (5:1);
        scalar(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── for-loop iteration ──────────────────────────────────────────────

#[test]
fn for_loop_over_range_visits_each_in_order() {
    let code = r#"
        my @visited;
        for my $i (1:5) {
            push @visited, $i;
        }
        join(",", @visited) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_loop_sum_of_one_to_100_is_5050() {
    let code = r#"
        my $sum = 0;
        for my $i (1:100) {
            $sum += $i;
        }
        $sum == 5050 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range in scalar context returns element count ───────────────────

#[test]
fn range_via_len_returns_element_count() {
    // `scalar(1:100)` returns empty (range doesn't materialize through
    // scalar() — separate bug). `len(1:100)` works correctly. Pin the
    // working form.
    let code = r#"
        my $n = len(1:100);
        $n == 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range with negative bounds ──────────────────────────────────────

#[test]
fn negative_to_positive_range() {
    let code = r#"
        my @r = (-3:3);
        # -3, -2, -1, 0, 1, 2, 3 = 7 elements.
        (scalar(@r) == 7 && $r[0] == -3 && $r[6] == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_to_negative_range() {
    let code = r#"
        my @r = (-5:-1);
        (scalar(@r) == 5 && $r[0] == -5 && $r[4] == -1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range as map/grep input ─────────────────────────────────────────

#[test]
fn map_over_range_squares() {
    let code = r#"
        my @sq = map { _ * _ } (1:5);
        join(",", @sq) eq "1,4,9,16,25" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn grep_over_range_filters_even() {
    let code = r#"
        my @evens = grep { _ % 2 == 0 } (1:10);
        join(",", @evens) eq "2,4,6,8,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range as sum input ───────────────────────────────────────────────

#[test]
fn sum_of_range_one_to_n() {
    let code = r#"
        sum(1:1000) == 500_500 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sum_of_squares_one_to_ten() {
    let code = r#"
        my $s = sum(map { _ * _ } (1:10));
        $s == 385 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested ranges ────────────────────────────────────────────────────

#[test]
fn nested_range_yields_cartesian_product_count() {
    let code = r#"
        my $count = 0;
        for my $i (1:5) {
            for my $j (1:5) {
                $count++;
            }
        }
        $count == 25 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range slice into array ──────────────────────────────────────────

#[test]
fn array_slice_via_range() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50, 60, 70, 80, 90);
        my @mid = @a[2:5];
        join(",", @mid) eq "30,40,50,60" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pipe-forward into range ─────────────────────────────────────────

#[test]
fn pipe_forward_range_into_len() {
    let code = r#"
        (1:42) |> len
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn pipe_forward_range_through_map_sum() {
    let code = r#"
        (1:10) |> map { _ * 2 } |> sum
    "#;
    assert_eq!(eval_int(code), 110); // 2+4+...+20 = 110
}

// ── Range with variable bounds ──────────────────────────────────────

#[test]
fn range_with_variable_bounds() {
    let code = r#"
        my $lo = 5;
        my $hi = 10;
        my @r = ($lo:$hi);
        (scalar(@r) == 6 && $r[0] == 5 && $r[5] == 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn range_with_expression_bounds() {
    let code = r#"
        my $n = 4;
        my @r = (($n - 2):($n + 2));
        join(",", @r) eq "2,3,4,5,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty range when lo > hi ────────────────────────────────────────

#[test]
fn variable_range_empty_when_lo_greater_than_hi() {
    let code = r#"
        my $lo = 10;
        my $hi = 5;
        my @r = ($lo:$hi);
        scalar(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range to product / fold ─────────────────────────────────────────

#[test]
fn factorial_via_reduce_over_range() {
    let code = r#"
        my $f = reduce { _0 * _1 } 1, (1:6);
        $f == 720 ? 1 : 0   # 6!
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range cannot be mutated like an array ──────────────────────────

#[test]
fn range_materialized_into_array_then_mutated() {
    let code = r#"
        my @r = (1:5);
        push @r, 99;
        (scalar(@r) == 6 && $r[5] == 99) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Large range memory safety smoke test ───────────────────────────

#[test]
fn ten_thousand_range_sum() {
    let code = r#"
        sum(1:10000) == 50_005_000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range in array literal context ─────────────────────────────────

#[test]
fn range_inside_array_literal_with_other_elements() {
    let code = r#"
        my @r = (0, 1:3, 99);
        # 0, 1, 2, 3, 99 = 5 elements
        (scalar(@r) == 5 && $r[0] == 0 && $r[4] == 99) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
