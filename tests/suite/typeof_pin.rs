//! Type-check predicate pins.

use crate::common::*;

// ── is_int ──────────────────────────────────────────────────────────

#[test]
fn is_int_true_for_integer() {
    let code = r#"
        is_int(42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_int_false_for_float() {
    let code = r#"
        is_int(3.14) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_int_false_for_string() {
    let code = r#"
        is_int("hello") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_int_true_for_negative_integer() {
    let code = r#"
        is_int(-42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_int_true_for_zero() {
    let code = r#"
        is_int(0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── is_float ────────────────────────────────────────────────────────

#[test]
fn is_float_true_for_decimal() {
    let code = r#"
        is_float(3.14) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_float_true_for_negative_decimal() {
    let code = r#"
        is_float(-2.5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_float_false_for_integer() {
    let code = r#"
        # Integer 42 stored as int, not float.
        is_float(42) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── is_str ──────────────────────────────────────────────────────────

#[test]
fn is_str_true_for_quoted_string() {
    let code = r#"
        is_str("hello") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_str_true_for_empty_string() {
    let code = r#"
        is_str("") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── is_arrayref ────────────────────────────────────────────────────

#[test]
fn is_arrayref_true_for_bracket_literal() {
    let code = r#"
        is_arrayref([1, 2, 3]) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_arrayref_false_for_bare_array() {
    let code = r#"
        my @a = (1, 2, 3);
        # @a in scalar context is count, not arrayref.
        is_arrayref(\@a) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_arrayref_false_for_scalar() {
    let code = r#"
        is_arrayref(42) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── is_hashref ─────────────────────────────────────────────────────

#[test]
fn is_hashref_true_for_curly_literal() {
    let code = r#"
        is_hashref(+{ a => 1 }) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_hashref_false_for_arrayref() {
    let code = r#"
        is_hashref([1, 2, 3]) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── is_code (or is_callable) ───────────────────────────────────────

#[test]
fn is_callable_true_for_coderef() {
    let code = r#"
        my $c = sub { 42 };
        is_callable($c) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_code_true_for_coderef() {
    let code = r#"
        my $c = sub { 42 };
        is_code($c) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── is_array (on bare array vs arrayref) ──────────────────────────

#[test]
fn is_array_distinguishes_from_arrayref() {
    let code = r#"
        my @a = (1, 2, 3);
        my $r = [1, 2, 3];
        # is_array tests on bare array vs is_arrayref on ref.
        # is_array(@a) — true for plain arrays.
        is_array(\@a) || is_arrayref(\@a) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Predicate composition ─────────────────────────────────────────

#[test]
fn is_int_and_is_str_mutually_exclusive_for_pure_types() {
    let code = r#"
        # An integer literal: is_int yes, is_str no (in pure-type sense).
        my $i = 42;
        my $s = "hi";
        (is_int($i) && !is_int($s) && is_str($s)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Predicates with refs ──────────────────────────────────────────

#[test]
fn is_arrayref_and_is_hashref_mutually_exclusive() {
    let code = r#"
        my $a = [1, 2];
        my $h = +{ x => 1 };
        (is_arrayref($a) && !is_hashref($a)
            && is_hashref($h) && !is_arrayref($h)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── is_ascii / is_alpha / is_blank ────────────────────────────────

#[test]
fn is_ascii_true_for_plain_ascii() {
    let code = r#"
        is_ascii("hello") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_ascii_false_for_unicode() {
    let code = r#"
        is_ascii("café") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_alpha_only_true_for_letters() {
    let code = r#"
        is_alpha_only("hello") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_alpha_only_false_for_mixed() {
    let code = r#"
        is_alpha_only("hello123") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_blank_true_for_whitespace_string() {
    let code = r#"
        is_blank("   ") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_blank_false_for_non_whitespace() {
    let code = r#"
        is_blank("hello") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Numeric range predicates ──────────────────────────────────────

#[test]
fn is_between_true_when_in_range() {
    let code = r#"
        is_between(5, 1, 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_between_false_when_out_of_range() {
    let code = r#"
        is_between(15, 1, 10) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── is_anagram ─────────────────────────────────────────────────────

#[test]
fn is_anagram_true_for_real_anagrams() {
    let code = r#"
        is_anagram("listen", "silent") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_anagram_false_for_non_anagrams() {
    let code = r#"
        is_anagram("hello", "world") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── is_base64 ─────────────────────────────────────────────────────

#[test]
fn is_base64_true_for_well_formed() {
    let code = r#"
        is_base64("SGVsbG8gV29ybGQ=") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── defined and ref combine ───────────────────────────────────────

#[test]
fn defined_check_with_is_arrayref() {
    let code = r#"
        my $r = [1, 2, 3];
        (defined($r) && is_arrayref($r)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn undef_is_not_any_specific_type() {
    let code = r#"
        my $u;
        (!is_int($u) && !is_str($u) && !is_arrayref($u) && !is_hashref($u)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
