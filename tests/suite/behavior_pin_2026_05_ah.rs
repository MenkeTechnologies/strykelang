//! Behavior-pinning batch AH (2026-05-05): Trivial String Ops, Predicates, Padding.

use crate::common::*;

// ── Trivial String Ops ───────────────────────────────────────────────────────

#[test]
fn string_trivial_ah() {
    assert_eq!(eval_string("repeat('a', 3)"), "aaa");
    assert_eq!(eval_string("capitalize('hello')"), "Hello");
    assert_eq!(eval_string("title_case('hello world')"), "Hello World");
    assert_eq!(eval_string("swap_case('aBc')"), "AbC");
    
    assert_eq!(eval_string("squish('  a  b  ')"), "a b");
}

// ── Padding & Truncation ─────────────────────────────────────────────────────

#[test]
fn string_padding_ah() {
    // center(s, width, pad)
    assert_eq!(eval_string("center('abc', 7, '-')"), "--abc--");
    
    // pad_left(s, width, pad)
    assert_eq!(eval_string("pad_left('5', 3, '0')"), "005");
    // pad_right(s, width, pad)
    assert_eq!(eval_string("pad_right('5', 3, '!')"), "5!!");
    
    // shorten(s, max_len)
    assert_eq!(eval_string("shorten('hello world', 7)"), "hello …");
}

// ── String Predicates ────────────────────────────────────────────────────────

#[test]
fn string_predicates_ah() {
    assert_eq!(eval_int("is_alpha('A')"), 1);
    assert_eq!(eval_int("is_alpha('1')"), 0);
    assert_eq!(eval_int("is_digit('1')"), 1);
    assert_eq!(eval_int("is_alnum('A1')"), 1);
    
    assert_eq!(eval_int("is_upper('A')"), 1);
    assert_eq!(eval_int("is_lower('a')"), 1);
    
    assert_eq!(eval_int("is_blank('  ')"), 1);
    assert_eq!(eval_int("is_empty('')"), 1);
}

// ── Character & Word Counts ──────────────────────────────────────────────────

#[test]
fn string_counts_ah() {
    assert_eq!(eval_int("char_count('hello')"), 5);
    assert_eq!(eval_int("word_count('hello world')"), 2);
    assert_eq!(eval_int("line_count(\"a\nb\nc\")"), 3);
}

// ── Identity ─────────────────────────────────────────────────────────────────

#[test]
fn identity_ah() {
    assert_eq!(eval_int("identity(42)"), 42);
    assert_eq!(eval_string("id('foo')"), "foo");
}
