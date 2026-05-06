//! Behavior-pinning batch AD (2026-05-05): String Helpers (Words, Cases, Search).

use crate::common::*;

// ── Word Extraction ──────────────────────────────────────────────────────────

#[test]
fn string_words_ad() {
    assert_eq!(eval_string(r#"first_word("  hello world  ")"#), "hello");
    assert_eq!(eval_string(r#"last_word("  hello world  ")"#), "world");
}

// ── Substring Helpers ────────────────────────────────────────────────────────

#[test]
fn string_substrings_ad() {
    // left_str(s, n)
    assert_eq!(eval_string(r#"left_str("hello", 2)"#), "he");
    // right_str(s, n)
    assert_eq!(eval_string(r#"right_str("hello", 2)"#), "lo");
    // mid_str(s, start, n)
    assert_eq!(eval_string(r#"mid_str("hello", 1, 3)"#), "ell");
}

// ── Case Conversions ─────────────────────────────────────────────────────────

#[test]
fn string_cases_ad() {
    assert_eq!(eval_string(r#"pascal_case("foo_bar_baz")"#), "FooBarBaz");
    assert_eq!(eval_string(r#"constant_case("foo_bar_baz")"#), "FOO_BAR_BAZ");
    assert_eq!(eval_string(r#"dot_case("foo_bar")"#), "foo.bar");
    assert_eq!(eval_string(r#"path_case("foo_bar")"#), "foo/bar");
    
    assert_eq!(eval_string(r#"lowercase("HELLO")"#), "hello");
    assert_eq!(eval_string(r#"uppercase("world")"#), "WORLD");
}

// ── Search & Replace ─────────────────────────────────────────────────────────

#[test]
fn string_search_replace_ad() {
    assert_eq!(eval_string(r#"join(",", indexes_of("banana", "a"))"#), "1,3,5");
    assert_eq!(eval_string(r#"replace_first("abcabc", "b", "z")"#), "azcabc");
    assert_eq!(eval_string(r#"replace_all_str("abcabc", "b", "z")"#), "azcazc");
}

// ── String Predicates (Extended) ─────────────────────────────────────────────

#[test]
fn string_predicates_ad() {
    assert_eq!(eval_int(r#"contains_any("hello", "e", "x")"#), 1);
    assert_eq!(eval_int(r#"contains_any("hello", "x", "y")"#), 0);
    
    assert_eq!(eval_int(r#"contains_all("hello", "e", "l")"#), 1);
    assert_eq!(eval_int(r#"contains_all("hello", "e", "x")"#), 0);
    
    assert_eq!(eval_int(r#"starts_with_any("hello", "h", "x")"#), 1);
    assert_eq!(eval_int(r#"ends_with_any("hello", "o", "x")"#), 1);
}
