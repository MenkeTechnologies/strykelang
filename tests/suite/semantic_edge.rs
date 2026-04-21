//! Tight runtime checks for operators and builtins that are easy to regress (no macro batching).

use crate::common::*;

#[test]
fn compound_add_assign_scalar() {
    assert_eq!(eval_int("my $x = 10; $x += 7; $x"), 17);
}

#[test]
fn compound_mul_assign_scalar() {
    assert_eq!(eval_int("my $x = 3; $x *= 4; $x"), 12);
}

#[test]
fn postincrement_returns_old_value() {
    assert_eq!(eval_int("my $x = 5; $x++"), 5);
}

#[test]
fn preincrement_returns_new_value() {
    assert_eq!(eval_int("my $x = 5; ++$x"), 6);
}

#[test]
fn string_repeat_zero_is_empty() {
    assert_eq!(eval_string(r#""ab" x 0"#), "");
}

#[test]
fn concat_preserves_utf8_literals() {
    assert_eq!(eval_string(r#""α" . "β""#), "αβ");
}

#[test]
fn lcfirst_ucfirst() {
    assert_eq!(eval_string(r#"lcfirst("Hello")"#), "hello");
    assert_eq!(eval_string(r#"ucfirst("hello")"#), "Hello");
}

#[test]
fn reverse_string_vs_list_context() {
    assert_eq!(eval_string(r#"scalar rev "abc""#), "cba");
    assert_eq!(eval_int("scalar rev (1, 2, 3)"), 321);
}

#[test]
fn join_default_empty_string() {
    assert_eq!(eval_string(r#"join("", "a", "b", "c")"#), "abc");
}

#[test]
fn sort_numeric_block() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } (10, 2, 1))"#),
        "1,2,10"
    );
}

#[test]
fn grep_block_filters() {
    assert_eq!(eval_int(r#"scalar grep { $_ > 2 } (1, 2, 3, 4)"#), 2);
}

#[test]
fn map_doubles() {
    assert_eq!(
        eval_string(r#"join("-", map { $_ * 2 } (1, 2, 3))"#),
        "2-4-6"
    );
}

#[test]
fn keys_in_scalar_context_is_count() {
    assert_eq!(
        eval_int(r#"my %h = (a => 1, b => 2, c => 3); scalar keys %h"#),
        3
    );
}

#[test]
fn defined_array_element() {
    assert_eq!(eval_int(r#"my @a = (1); defined $a[0] ? 1 : 0"#), 1);
}

#[test]
fn ternary_nested() {
    assert_eq!(eval_int("1 ? (0 ? 1 : 2) : 3"), 2);
}

#[test]
fn floating_division_truncates_toward_zero() {
    assert_eq!(eval_int("7 / 2"), 3);
}

#[test]
fn modulo_preserves_sign_of_dividend() {
    assert_eq!(eval_int("-7 % 3"), -1);
}

#[test]
fn bitwise_not_uses_truncated_int() {
    assert_eq!(eval_int("~0"), -1);
}

#[test]
fn shift_right_integer() {
    assert_eq!(eval_int("32 >> 3"), 4);
    assert_eq!(eval_int("-8 >> 1"), -4);
}

#[test]
fn chr_ord_roundtrip_ascii() {
    assert_eq!(eval_int(r#"ord("A")"#), 65);
    assert_eq!(eval_string(r#"chr(65)"#), "A");
}

#[test]
fn sprintf_percent_d_integer() {
    assert_eq!(eval_string(r#"sprintf("%d", 42)"#), "42");
}

#[test]
fn bitwise_xor_integer() {
    assert_eq!(eval_int("0b101 ^ 0b011"), 6);
}

#[test]
fn hex_literal_integer() {
    assert_eq!(eval_int("0xFF"), 255);
}

#[test]
fn octal_literal_integer() {
    assert_eq!(eval_int("0377"), 255);
}
