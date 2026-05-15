//! Array-reference pins. \@arr, [LIST], @$ref, @{$ref} deref,
//! slicing through ref, push/pop via ref.

use crate::common::*;

// ── Constructors ────────────────────────────────────────────────────

#[test]
fn arrayref_literal_via_brackets() {
    let code = r#"
        my $r = [1, 2, 3];
        ref($r) =~ /ARRAY/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrayref_from_named_array_via_backslash() {
    let code = r#"
        my @a = (10, 20, 30);
        my $r = \@a;
        (ref($r) =~ /ARRAY/ && $r->[1] == 20) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Indexing ────────────────────────────────────────────────────────

#[test]
fn arrow_index_into_arrayref() {
    let code = r#"
        my $r = [10, 20, 30, 40, 50];
        ($r->[0] == 10 && $r->[4] == 50) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_index_into_arrayref() {
    let code = r#"
        my $r = [10, 20, 30];
        $r->[-1] == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Length ──────────────────────────────────────────────────────────

#[test]
fn scalar_deref_returns_length() {
    let code = r#"
        my $r = [1, 2, 3, 4, 5];
        scalar(@$r) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn len_on_deref_returns_length() {
    let code = r#"
        my $r = [1, 2, 3, 4, 5];
        len(@$r) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Deref-to-array ─────────────────────────────────────────────────

#[test]
fn full_deref_at_dollar_ref() {
    let code = r#"
        my $r = [1, 2, 3];
        my @copy = @$r;
        len(@copy) == 3 && $copy[2] == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn full_deref_at_curly_dollar_ref() {
    let code = r#"
        my $r = [10, 20, 30];
        my @copy = @{$r};
        join(",", @copy) eq "10,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── push / pop via ref ──────────────────────────────────────────────

#[test]
fn push_via_arrayref_deref() {
    let code = r#"
        my $r = [1, 2];
        push @$r, 3;
        push @$r, 4, 5;
        join(",", @$r) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pop_via_arrayref_deref() {
    let code = r#"
        my $r = [1, 2, 3, 4];
        my $last = pop @$r;
        ($last == 4 && len(@$r) == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn shift_via_arrayref_deref() {
    let code = r#"
        my $r = [10, 20, 30];
        my $first = shift @$r;
        ($first == 10 && len(@$r) == 2 && $r->[0] == 20) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn unshift_via_arrayref_deref() {
    let code = r#"
        my $r = [30, 40];
        unshift @$r, 10, 20;
        join(",", @$r) eq "10,20,30,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slicing through ref ─────────────────────────────────────────────

#[test]
fn slice_through_arrayref_via_at_curly() {
    let code = r#"
        my $r = [10, 20, 30, 40, 50];
        my @mid = @{$r}[1, 3];
        join(",", @mid) eq "20,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn range_slice_through_arrayref() {
    let code = r#"
        my $r = [10, 20, 30, 40, 50];
        my @mid = @{$r}[1:3];
        join(",", @mid) eq "20,30,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested arrayrefs ────────────────────────────────────────────────

#[test]
fn nested_arrayref_indexing() {
    let code = r#"
        my $r = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
        ($r->[0]->[0] == 1
            && $r->[1]->[1] == 5
            && $r->[2]->[2] == 9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn nested_arrayref_implicit_arrow() {
    let code = r#"
        my $r = [[1, 2], [3, 4]];
        # Perl: $r->[0][1] is shorthand for $r->[0]->[1].
        $r->[0][1] == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Map / grep through arrayref ─────────────────────────────────────

#[test]
fn map_over_arrayref_deref() {
    let code = r#"
        my $r = [1, 2, 3, 4];
        my @doubled = map { _ * 2 } @$r;
        join(",", @doubled) eq "2,4,6,8" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn grep_over_arrayref_deref() {
    let code = r#"
        my $r = [1, 2, 3, 4, 5, 6];
        my @evens = grep { _ % 2 == 0 } @$r;
        join(",", @evens) eq "2,4,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Modifying through ref mutates underlying ───────────────────────

#[test]
fn modifying_via_ref_mutates_underlying() {
    let code = r#"
        my @a = (1, 2, 3);
        my $r = \@a;
        $r->[1] = 999;
        $a[1] == 999 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pushing_via_ref_mutates_underlying() {
    let code = r#"
        my @a = (1, 2);
        my $r = \@a;
        push @$r, 3, 4;
        join(",", @a) eq "1,2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Pass arrayref to fn ─────────────────────────────────────────────

#[test]
fn arrayref_passed_to_fn_modifiable() {
    let code = r#"
        fn Demo::AR::add_one_to_each($r) {
            for my $i (0:len(@$r) - 1) {
                $r->[$i] = $r->[$i] + 1;
            }
        }
        my @a = (1, 2, 3);
        Demo::AR::add_one_to_each(\@a);
        join(",", @a) eq "2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrayref_returned_from_fn() {
    let code = r#"
        fn Demo::AR::make_range($n) {
            my @a;
            for my $i (1:$n) { push @a, $i }
            return \@a
        }
        my $r = Demo::AR::make_range(5);
        len(@$r) == 5 && $r->[4] == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty arrayref ─────────────────────────────────────────────────

#[test]
fn empty_arrayref_via_brackets() {
    let code = r#"
        my $r = [];
        (ref($r) =~ /ARRAY/ && len(@$r) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Array literal in list context produces flat list ──────────────

#[test]
fn arrayref_into_list_context_flattens() {
    let code = r#"
        my @list = (1, 2, @{[3, 4, 5]}, 6);
        join(",", @list) eq "1,2,3,4,5,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── splice on arrayref-deref ────────────────────────────────────────

#[test]
fn splice_via_arrayref_deref() {
    let code = r#"
        my $r = [1, 2, 3, 4, 5];
        splice(@$r, 1, 2);
        join(",", @$r) eq "1,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── join via deref ─────────────────────────────────────────────────

#[test]
fn join_through_arrayref_deref() {
    let code = r#"
        my $r = ["alpha", "beta", "gamma"];
        join("-", @$r) eq "alpha-beta-gamma" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sort through arrayref ──────────────────────────────────────────

#[test]
fn sort_through_arrayref_deref() {
    let code = r#"
        my $r = [3, 1, 4, 1, 5, 9, 2, 6];
        my @sorted = sort { _0 <=> _1 } @$r;
        join(",", @sorted) eq "1,1,2,3,4,5,6,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── arrayref equality (ref-identity) ───────────────────────────────

#[test]
fn arrayref_assigned_then_modified_visible_through_ref() {
    let code = r#"
        my @a = (1, 2, 3);
        my $r = \@a;
        push @a, 99;
        $r->[3] == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── reverse through deref ─────────────────────────────────────────

#[test]
fn reverse_through_arrayref_deref() {
    let code = r#"
        my $r = [1, 2, 3, 4, 5];
        my @rev = reverse @$r;
        join(",", @rev) eq "5,4,3,2,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
