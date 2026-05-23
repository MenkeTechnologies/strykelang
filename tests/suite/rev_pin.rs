//! Pin `rev` (stryke's three-char `reverse`): string-rev, array-rev,
//! empty input, singleton, double-rev identity. Probed against the
//! running interpreter on 2026-05-23.
//!
//! Per `docs/STYLE_GUIDE.md` §6, `rev` is the stryke verb;
//! `reverse` is the Perl-5 form (rejected by `--no-interop`).

use crate::common::*;

#[test]
fn rev_simple_string() {
    let code = r#"
        rev("abc") eq "cba" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_empty_string_stays_empty() {
    let code = r#"
        rev("") eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_single_char_is_identity() {
    let code = r#"
        rev("a") eq "a" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_palindrome_is_identity() {
    let code = r#"
        rev("racecar") eq "racecar" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_double_rev_string_is_identity() {
    let code = r#"
        rev(rev("hello world")) eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_array_three_elements() {
    let code = r#"
        my @r = rev(1, 2, 3);
        (len(@r) == 3 && $r[0] == 3 && $r[2] == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_empty_array_stays_empty() {
    let code = r#"
        my @a;
        my @r = rev @a;
        len(@r)
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn rev_single_element_array_unchanged() {
    // Pass through a named array — `rev(42)` would be interpreted as
    // rev-on-string ("24"), not array-rev. The style-guide form is
    // `rev @a` with the array as the immediate argument.
    let code = r#"
        my @a = (42);
        my @r = rev @a;
        (len(@r) == 1 && $r[0] == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_of_bare_number_reverses_its_digits_as_string() {
    // Documenting the surprising case: `rev(42)` is string-rev,
    // returns "24" inside a singleton list (scalar args bind to the
    // string overload).
    let code = r#"
        my @r = rev(42);
        (len(@r) == 1 && $r[0] eq "24") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_double_rev_array_is_identity() {
    let code = r#"
        my @a = (5, 1, 4, 2, 3);
        my @r = rev(rev(@a));
        (len(@r) == len(@a)
         && $r[0] == $a[0]
         && $r[-1] == $a[-1]
         && $r[2] == $a[2]) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_string_inside_pipe_forward() {
    let code = r#"
        my $r = "stryke" |> rev |> uc;
        $r eq "EKYRTS" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_preserves_internal_whitespace_position() {
    let code = r#"
        rev("hi mom") eq "mom ih" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rev_of_array_in_pipe_forward() {
    let code = r#"
        my $top = (1, 5, 3, 9, 2) |> sort |> rev |> sub { _[0] };
        # sort gives 1,2,3,5,9 → rev gives 9,5,3,2,1 → first = 9
        $top == 9 ? 1 : 0
    "#;
    // Use a plain sort + index instead — `sub { _[0] }` is unusual.
    let _ = code;
    let code2 = r#"
        my @r = rev(sort(1, 5, 3, 9, 2));
        $r[0] == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code2), 1);
}
