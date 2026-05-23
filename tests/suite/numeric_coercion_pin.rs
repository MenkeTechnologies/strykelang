//! Additional pins for Perl-style numeric coercion that complement
//! the broader `number_coercion_pin.rs`. Focus here: leading
//! whitespace, leading sign-only strings, leading-dot decimals,
//! scientific-with-trailing-garbage, two-string arithmetic, empty
//! string. Probed against the running interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn coerce_skips_leading_whitespace() {
    let code = r#"
        ("   5" + 0) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coerce_leading_whitespace_signed_scientific() {
    let code = r#"
        my $x = "  -1.5e3 foo" + 0;
        $x == -1500 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coerce_with_leading_plus_sign() {
    let code = r#"
        ("+12def" + 0) == 12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coerce_with_leading_dot_decimal() {
    let code = r#"
        my $x = ".5xyz" + 0;
        ($x > 0.499 && $x < 0.501) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coerce_scientific_with_trailing_garbage() {
    let code = r#"
        my $x = "5.5e2abc" + 0;
        $x == 550 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coerce_arithmetic_between_two_strings() {
    let code = r#"
        ("5" + "7") == 12 && ("10" * "3") == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coerce_addition_with_non_numeric_string() {
    // "abc" + 1 → 0 + 1 → 1.
    let code = r#"
        ("abc" + 1) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coerce_only_signs_yields_zero() {
    let code = r#"
        my $a = "+" + 0;
        my $b = "-" + 0;
        ($a == 0 && $b == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coerce_empty_string_is_zero() {
    let code = r#"
        ("" + 0) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coerce_zero_string_is_zero() {
    let code = r#"
        ("0" + 0) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
