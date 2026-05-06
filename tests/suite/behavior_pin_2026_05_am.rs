//! Behavior-pinning batch AM (2026-05-06): String Helpers (Words, Cases, Search).

use crate::common::*;

// ── Word Extraction ──────────────────────────────────────────────────────────

#[test]
fn string_words_am() {
    assert_eq!(eval_string(r#"first_word("  hello world from rust  ")"#), "hello");
    assert_eq!(eval_string(r#"last_word("  hello world from rust  ")"#), "rust");
}

// ── Substring Helpers ────────────────────────────────────────────────────────

#[test]
fn string_substrings_am() {
    // left_str(s, n)
    assert_eq!(eval_string(r#"left_str("hello", 0)"#), "");
    assert_eq!(eval_string(r#"left_str("hello", 10)"#), "hello");
    // right_str(s, n)
    assert_eq!(eval_string(r#"right_str("hello", 0)"#), "");
    assert_eq!(eval_string(r#"right_str("hello", 10)"#), "hello");
    // mid_str(s, start, n)
    assert_eq!(eval_string(r#"mid_str("hello", 0, 3)"#), "hel");
    assert_eq!(eval_string(r#"mid_str("hello", 4, 3)"#), "o");
}

// ── Case Conversions ─────────────────────────────────────────────────────────

#[test]
fn string_cases_am() {
    assert_eq!(eval_string(r#"pascal_case("foo_bar_baz_qux")"#), "FooBarBazQux");
    assert_eq!(
        eval_string(r#"constant_case("foo_bar_baz_qux")"#),
        "FOO_BAR_BAZ_QUX"
    );
    assert_eq!(eval_string(r#"dot_case("foo_bar_baz")"#), "foo.bar.baz");
    assert_eq!(eval_string(r#"path_case("foo_bar_baz")"#), "foo/bar/baz");

    assert_eq!(eval_string(r#"lowercase("HELLO WORLD")"#), "hello world");
    assert_eq!(eval_string(r#"uppercase("hello world")"#), "HELLO WORLD");
}

// ── Search & Replace ─────────────────────────────────────────────────────────

#[test]
fn string_search_replace_am() {
    assert_eq!(
        eval_string(r#"join(",", indexes_of("bananana", "ana"))"#),
        "1,5"
    );
    assert_eq!(
        eval_string(r#"replace_first("abcabcabc", "b", "z")"#),
        "azcabcabc"
    );
    assert_eq!(
        eval_string(r#"replace_all_str("abcabcabc", "b", "z")"#),
        "azcazcazc"
    );
}

// ── String Predicates (Extended) ─────────────────────────────────────────────

#[test]
fn string_predicates_am() {
    assert_eq!(eval_int(r#"contains_any("hello world", "e", "x", "w")"#), 1);
    assert_eq!(eval_int(r#"contains_any("hello world", "x", "y", "z")"#), 0);

    assert_eq!(eval_int(r#"contains_all("hello world", "e", "l", "w")"#), 1);
    assert_eq!(eval_int(r#"contains_all("hello world", "e", "x", "w")"#), 0);

    assert_eq!(eval_int(r#"starts_with_any("hello world", "h", "x", "w")"#), 1);
    assert_eq!(eval_int(r#"ends_with_any("hello world", "d", "x", "w")"#), 1);
}
