//! Array-operation pins. push/pop/shift/unshift/splice + slicing.
//! Cover the corners that demos rely on every round.

use crate::common::*;

// ── push ─────────────────────────────────────────────────────────────

#[test]
fn push_appends_to_end() {
    let code = r#"
        my @a = (1, 2, 3);
        push @a, 4;
        join(",", @a) eq "1,2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn push_multiple_in_one_call() {
    let code = r#"
        my @a = (1);
        push @a, 2, 3, 4;
        join(",", @a) eq "1,2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn push_returns_new_length() {
    let code = r#"
        my @a = (1, 2);
        my $n = push @a, 3, 4, 5;
        $n == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pop ──────────────────────────────────────────────────────────────

#[test]
fn pop_removes_and_returns_last() {
    let code = r#"
        my @a = (1, 2, 3);
        my $r = pop @a;
        ($r == 3 && len(@a) == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pop_on_empty_returns_undef() {
    let code = r#"
        my @a;
        my $r = pop @a;
        defined($r) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── shift ────────────────────────────────────────────────────────────

#[test]
fn shift_removes_and_returns_first() {
    let code = r#"
        my @a = (10, 20, 30);
        my $r = shift @a;
        ($r == 10 && $a[0] == 20) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn shift_on_empty_returns_undef() {
    let code = r#"
        my @a;
        my $r = shift @a;
        defined($r) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── unshift ──────────────────────────────────────────────────────────

#[test]
fn unshift_prepends_to_front() {
    let code = r#"
        my @a = (3, 4, 5);
        unshift @a, 1, 2;
        join(",", @a) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn unshift_returns_new_length() {
    let code = r#"
        my @a = (3, 4);
        my $n = unshift @a, 1, 2;
        $n == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── splice ───────────────────────────────────────────────────────────

#[test]
fn splice_removes_middle() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5);
        my @removed = splice(@a, 1, 2);
        (join(",", @a) eq "1,4,5"
            && join(",", @removed) eq "2,3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_insert_at_index() {
    let code = r#"
        my @a = (1, 2, 5, 6);
        splice(@a, 2, 0, 3, 4);
        join(",", @a) eq "1,2,3,4,5,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_replace_range() {
    let code = r#"
        my @a = (1, 2, 999, 999, 5);
        splice(@a, 2, 2, 3, 4);
        join(",", @a) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_remove_from_end_via_negative_index() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5);
        splice(@a, -2, 2);
        join(",", @a) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn splice_no_length_removes_to_end() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5);
        splice(@a, 2);
        join(",", @a) eq "1,2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Array slice ─────────────────────────────────────────────────────

#[test]
fn array_slice_with_indices() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @slice = @a[1, 3];
        join(",", @slice) eq "20,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_slice_with_range() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @slice = @a[1:3];
        join(",", @slice) eq "20,30,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_slice_negative_index() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my $last = $a[-1];
        my $second_last = $a[-2];
        ($last == 50 && $second_last == 40) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Array assignment from list ──────────────────────────────────────

#[test]
fn list_assignment_to_array() {
    let code = r#"
        my @a;
        @a = (1, 2, 3, 4, 5);
        len(@a) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn list_assignment_with_function_call() {
    let code = r#"
        my @a = sort { _0 <=> _1 } (3, 1, 4, 1, 5);
        join(",", @a) eq "1,1,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Array in scalar context ─────────────────────────────────────────

#[test]
fn array_in_scalar_context_returns_count() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5);
        my $n = scalar @a;
        $n == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn empty_array_in_scalar_context_returns_zero() {
    let code = r#"
        my @empty;
        scalar(@empty) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Array concatenation ─────────────────────────────────────────────

#[test]
fn array_literal_concat() {
    let code = r#"
        my @a = (1, 2, 3);
        my @b = (4, 5, 6);
        my @c = (@a, @b);
        join(",", @c) eq "1,2,3,4,5,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn push_array_into_array() {
    let code = r#"
        my @a = (1, 2);
        my @b = (3, 4);
        push @a, @b;
        join(",", @a) eq "1,2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── delete on array (sparse) ────────────────────────────────────────

#[test]
fn delete_array_element_leaves_undef() {
    let code = r#"
        my @a = (10, 20, 30, 40);
        delete $a[1];
        # Length unchanged (sparse). $a[1] now undef.
        (len(@a) == 4 && !defined($a[1])) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Array equality (compare via join or len) ────────────────────────

#[test]
fn arrays_equal_via_join() {
    let code = r#"
        my @a = (1, 2, 3);
        my @b = (1, 2, 3);
        join(",", @a) eq join(",", @b) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Array reversal ─────────────────────────────────────────────────

#[test]
fn array_reverse_in_place_via_assignment() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5);
        @a = reverse @a;
        join(",", @a) eq "5,4,3,2,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Array of strings ───────────────────────────────────────────────

#[test]
fn array_of_strings_sort_and_join() {
    let code = r#"
        my @a = ("banana", "apple", "cherry");
        @a = sort { _0 cmp _1 } @a;
        join("-", @a) eq "apple-banana-cherry" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Push then pop round-trip ───────────────────────────────────────

#[test]
fn push_pop_roundtrip() {
    let code = r#"
        my @a = (1, 2, 3);
        push @a, 99;
        my $r = pop @a;
        ($r == 99 && join(",", @a) eq "1,2,3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Modify via index assignment ────────────────────────────────────

#[test]
fn array_index_assignment() {
    let code = r#"
        my @a = (1, 2, 3, 4);
        $a[2] = 99;
        join(",", @a) eq "1,2,99,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_grows_when_writing_past_end() {
    let code = r#"
        my @a = (1, 2);
        $a[5] = 99;
        # Length now 6 with undef in slots 2,3,4.
        (len(@a) == 6 && $a[5] == 99 && !defined($a[3])) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── 2D array (array of arrayrefs) ──────────────────────────────────

#[test]
fn two_d_array_via_arrayrefs() {
    let code = r#"
        my @grid;
        for my $i (0:2) {
            $grid[$i] = [];
            for my $j (0:2) {
                $grid[$i]->[$j] = $i * 3 + $j;
            }
        }
        ($grid[0]->[0] == 0
            && $grid[1]->[1] == 4
            && $grid[2]->[2] == 8) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Wantarray-style context check ───────────────────────────────────

#[test]
fn list_context_vs_scalar_context_returns_different() {
    let code = r#"
        fn Demo::Ctx::make_list() { (10, 20, 30) }
        my @list = Demo::Ctx::make_list();
        my $count = Demo::Ctx::make_list();
        # In scalar context, comma-list returns last value (Perl rule).
        (len(@list) == 3 && $count == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
