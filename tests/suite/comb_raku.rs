//! Pin the four Raku `comb` signatures stryke now supports:
//!   - `comb($str)`                       → chars
//!   - `comb($size:Int, $str [, $limit])` → fixed-size character chunks
//!   - `comb($needle:Str, $str [, $limit])` → every literal occurrence
//!   - `comb(qr/PAT/, $str [, $limit])`   → every regex match (`/g`-style)
//!
//! Regression here means an intentional or accidental behavioral change to
//! `builtin_comb` in `strykelang/builtins.rs`. `comb` was previously an
//! alias of `combinations`; the rename is the reason these pins exist.

use crate::common::*;

#[test]
fn comb_single_arg_returns_each_char() {
    let code = r#"
        my @r = comb("abc");
        len(@r) == 3 && $r[0] eq "a" && $r[1] eq "b" && $r[2] eq "c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn comb_single_arg_unicode_is_codepoint_per_element() {
    // Multi-byte glyphs must come out as one element each, not split per byte.
    let code = r#"
        my @r = comb("héllo");
        len(@r) == 5 && $r[1] eq "é" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn comb_size_form_fixed_chunks_with_remainder() {
    // Raku: "abcdefghijk".comb(3) -> ("abc","def","ghi","jk")
    let code = r#"
        my @r = comb(3, "abcdefghijk");
        join("|", @r)
    "#;
    assert_eq!(eval_string(code), "abc|def|ghi|jk");
}

#[test]
fn comb_size_form_size_larger_than_string_returns_whole_string() {
    let code = r#"
        my @r = comb(10, "abc");
        len(@r) == 1 && $r[0] eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn comb_size_form_with_limit_caps_chunk_count() {
    // Raku semantics: limit caps the number of pieces returned.
    let code = r#"
        my @r = comb(2, "abcdefgh", 2);
        join("|", @r) eq "ab|cd" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn comb_literal_needle_returns_every_occurrence() {
    let code = r#"
        my @r = comb("ab", "ababab");
        len(@r) == 3 && $r[0] eq "ab" && $r[2] eq "ab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn comb_literal_needle_no_match_returns_empty_array() {
    let code = r#"
        my @r = comb("zz", "abcdef");
        len(@r)
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn comb_literal_needle_with_limit_caps_match_count() {
    let code = r#"
        my @r = comb("a", "banana", 2);
        len(@r)
    "#;
    assert_eq!(eval_int(code), 2);
}

#[test]
fn comb_regex_form_returns_every_match_text() {
    let code = r#"
        my @r = comb(qr/\w+/, "a;b;;c");
        join("|", @r) eq "a|b|c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn comb_regex_form_with_limit_caps_match_count() {
    let code = r#"
        my @r = comb(qr/\d+/, "a1b22c333", 2);
        join("|", @r) eq "1|22" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn comb_regex_form_no_match_returns_empty_array() {
    let code = r#"
        my @r = comb(qr/\d+/, "no digits here");
        len(@r)
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn comb_limit_zero_returns_empty_array_for_every_form() {
    // limit=0 is a hard cap: every signature must respect it.
    assert_eq!(eval_int(r#"len(comb(2,  "abcdef",   0))"#), 0);
    assert_eq!(eval_int(r#"len(comb("a", "banana",  0))"#), 0);
    assert_eq!(eval_int(r#"len(comb(qr/./, "xyz",   0))"#), 0);
}

#[test]
fn comb_negative_limit_means_unlimited() {
    // Raku treats a negative/Inf limit as no cap; we mirror that.
    let code = r#"
        my @r = comb("a", "aaaa", -1);
        len(@r)
    "#;
    assert_eq!(eval_int(code), 4);
}

#[test]
fn comb_empty_input_yields_empty_array() {
    assert_eq!(eval_int(r#"len(comb(""))"#),               0);
    assert_eq!(eval_int(r#"len(comb(3, ""))"#),            0);
    assert_eq!(eval_int(r#"len(comb("x", ""))"#),          0);
    assert_eq!(eval_int(r#"len(comb(qr/./, ""))"#),        0);
}

#[test]
fn comb_is_no_longer_an_alias_of_combinations() {
    // Pin the breaking change explicitly: `comb(5, 3)` used to dispatch to
    // `combinations` (returning an empty array because 3 is treated as a
    // 1-element list with n=5). Now it dispatches to the Raku form, where
    // the int 5 is a chunk size applied to "3" stringified.
    let code = r#"
        my @r = comb(5, 3);
        len(@r) == 1 && $r[0] eq "3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn combinations_still_returns_all_n_element_combinations() {
    // Negative pin: the original `combinations(N, LIST...)` semantics must
    // survive the comb split.
    let code = r#"
        my @r = combinations(2, 1, 2, 3);
        len(@r) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
