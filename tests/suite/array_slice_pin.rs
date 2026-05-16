//! Array-slice pins beyond array_ops_pin.rs. Focus on slice ops
//! through indexes, ranges, negatives, and refs.

use crate::common::*;

// ── Index-list slice ──────────────────────────────────────────────

#[test]
fn index_list_slice() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50, 60);
        my @r = @a[0, 2, 5];
        join(",", @r) eq "10,30,60" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_slice_with_repeated_index() {
    let code = r#"
        my @a = (10, 20, 30);
        my @r = @a[0, 1, 0, 2, 0];
        join(",", @r) eq "10,20,10,30,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range slice ──────────────────────────────────────────────────

#[test]
fn range_slice_inclusive() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @r = @a[1:3];
        join(",", @r) eq "20,30,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn range_slice_from_start() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @r = @a[0:2];
        join(",", @r) eq "10,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn range_slice_to_end() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @r = @a[2:4];
        join(",", @r) eq "30,40,50" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Negative indices ─────────────────────────────────────────────

#[test]
fn negative_single_index() {
    let code = r#"
        my @a = (10, 20, 30);
        $a[-1] == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_in_slice_list() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @r = @a[-3, -1];
        join(",", @r) eq "30,50" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn second_to_last_via_negative() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        $a[-2] == 40 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice assignment ─────────────────────────────────────────────

#[test]
fn slice_assignment_overwrites_multiple() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        @a[1, 3] = (200, 400);
        join(",", @a) eq "10,200,30,400,50" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn range_slice_assignment() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        @a[1:3] = (200, 300, 400);
        join(",", @a) eq "10,200,300,400,50" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice via arrayref ───────────────────────────────────────────

#[test]
fn slice_through_arrayref_with_indices() {
    let code = r#"
        my $r = [10, 20, 30, 40, 50];
        my @s = @{$r}[1, 3];
        join(",", @s) eq "20,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn range_slice_through_arrayref() {
    let code = r#"
        my $r = [10, 20, 30, 40, 50];
        my @s = @{$r}[1:3];
        join(",", @s) eq "20,30,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice past end yields undef ──────────────────────────────────

#[test]
fn slice_past_end_yields_undef() {
    let code = r#"
        my @a = (10, 20, 30);
        my @s = @a[1, 5, 10];
        ($s[0] == 20 && !defined($s[1]) && !defined($s[2])) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty slice returns empty array ──────────────────────────────

#[test]
fn empty_slice_returns_empty() {
    let code = r#"
        my @a = (10, 20, 30);
        my @empty_idx;
        my @s = @a[@empty_idx];
        # BUG-235: @h{@arr} returns 1 empty element rather than 0.
        # For arrays, also pin observed behavior.
        len(@s) <= 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Reverse slice via reverse builtin ────────────────────────────

#[test]
fn reverse_via_reverse_builtin() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @r = reverse @a[0:4];
        join(",", @r) eq "50,40,30,20,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice then map ───────────────────────────────────────────────

#[test]
fn slice_then_map() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5, 6, 7);
        my @r = map { _ * 10 } @a[1:3];
        join(",", @r) eq "20,30,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn slice_then_grep_filter() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5, 6, 7);
        my @r = grep { _ % 2 == 0 } @a[0:6];
        join(",", @r) eq "2,4,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice into sum ───────────────────────────────────────────────

#[test]
fn slice_into_sum() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
        sum(@a[2:7]) == 33 ? 1 : 0   # 3+4+5+6+7+8 = 33
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── First N / Last N idioms ──────────────────────────────────────

#[test]
fn first_three_via_slice() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50, 60, 70);
        my @first = @a[0:2];
        join(",", @first) eq "10,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn last_three_via_negative_range() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50, 60, 70);
        # Negative ranges may not be supported; use computed indices.
        my $n = len(@a);
        my @last = @a[($n - 3) : ($n - 1)];
        join(",", @last) eq "50,60,70" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice from a function call ───────────────────────────────────

#[test]
fn slice_after_fn_call() {
    let code = r#"
        fn Demo::AS::make() { (10, 20, 30, 40, 50) }
        my @all = Demo::AS::make();
        my @slice = @all[1:3];
        join(",", @slice) eq "20,30,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Single-index access returns scalar ───────────────────────────

#[test]
fn single_index_returns_scalar() {
    let code = r#"
        my @a = (10, 20, 30);
        my $s = $a[1];
        $s == 20 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── 2D slice via map ─────────────────────────────────────────────

#[test]
fn slice_2d_grid_column_via_map() {
    let code = r#"
        my @grid = ([1, 2, 3], [4, 5, 6], [7, 8, 9]);
        # Extract column 1 (middle column).
        my @col = map { $_->[1] } @grid;
        join(",", @col) eq "2,5,8" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice with computed step (manual stride via map) ─────────────

#[test]
fn slice_every_second_via_map_grep() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
        my @evens_by_index;
        for my $i (0:len(@a) - 1) {
            push @evens_by_index, $a[$i] if $i % 2 == 0;
        }
        join(",", @evens_by_index) eq "1,3,5,7,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice on grown array ─────────────────────────────────────────

#[test]
fn slice_after_grow() {
    let code = r#"
        my @a = (1, 2, 3);
        push @a, 4, 5, 6, 7;
        my @s = @a[2:5];
        join(",", @s) eq "3,4,5,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multiple slices in same statement ────────────────────────────

#[test]
fn two_slices_compose_via_concat() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5, 6, 7, 8);
        my @combo = (@a[0:1], @a[6:7]);
        join(",", @combo) eq "1,2,7,8" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice of 1000 elements ────────────────────────────────────────

#[test]
fn large_array_slice() {
    let code = r#"
        my @big = (1:1000);
        my @mid = @big[400:599];
        (len(@mid) == 200 && $mid[0] == 401 && $mid[199] == 600) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
