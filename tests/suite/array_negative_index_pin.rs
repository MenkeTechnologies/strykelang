//! Pin negative-index semantics on arrays: read, write, slice with
//! a negative range. Probed against the running interpreter on
//! 2026-05-23.

use crate::common::*;

#[test]
fn negative_one_is_last_element() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        $a[-1] == 50 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_two_is_penultimate() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        $a[-2] == 40 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_len_is_first_element() {
    // For a 5-element array, $a[-5] == $a[0].
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        $a[-5] == 10 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_index_out_of_bounds_read_returns_undef() {
    // -99 against a 3-element array — past the front edge.
    let code = r#"
        my @a = (10, 20, 30);
        my $v = $a[-99];
        defined($v) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn negative_index_write_modifies_in_place() {
    let code = r#"
        my @a = (10, 20, 30);
        $a[-1] = 99;
        ($a[2] == 99 && len(@a) == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_index_write_to_middle_via_neg_two() {
    let code = r#"
        my @a = (10, 20, 30, 40);
        $a[-2] = 999;
        ($a[2] == 999 && $a[3] == 40 && len(@a) == 4) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_range_slice_tail() {
    // @a[-3..-1] = last three elements.
    let code = r#"
        my @a = (1, 2, 3, 4, 5);
        my @s = @a[-3..-1];
        (len(@s) == 3 && $s[0] == 3 && $s[2] == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_index_in_slice_with_positive_index() {
    // Mixed positive/negative — pick first and last.
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @s = @a[0, -1];
        (len(@s) == 2 && $s[0] == 10 && $s[1] == 50) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_index_on_singleton_array() {
    let code = r#"
        my @a = (42);
        ($a[-1] == 42 && $a[0] == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_index_after_push() {
    let code = r#"
        my @a = (1, 2, 3);
        push @a, 99;
        $a[-1] == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_index_after_pop() {
    let code = r#"
        my @a = (1, 2, 3, 4);
        pop @a;
        $a[-1] == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_one_consistent_with_dollar_hash_a() {
    // $#a is the last valid index; $a[$#a] must equal $a[-1].
    let code = r#"
        my @a = (10, 20, 30);
        $a[$#a] == $a[-1] ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
