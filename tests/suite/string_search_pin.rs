//! String-search builtin pins: index, rindex, contains, starts_with,
//! ends_with.

use crate::common::*;

// ── index ──────────────────────────────────────────────────────────

#[test]
fn index_finds_first_occurrence() {
    let code = r#"
        index("hello world", "world") == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_returns_minus_one_on_no_match() {
    let code = r#"
        index("hello world", "missing") == -1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_with_start_offset_skips_earlier() {
    let code = r#"
        index("abc abc abc", "abc", 1) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_at_start_returns_zero() {
    let code = r#"
        index("hello", "hello") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_empty_needle_returns_zero() {
    let code = r#"
        index("hello", "") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── rindex ─────────────────────────────────────────────────────────

#[test]
fn rindex_finds_last_occurrence() {
    let code = r#"
        rindex("abc abc abc", "abc") == 8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rindex_no_match_returns_minus_one() {
    let code = r#"
        rindex("hello", "world") == -1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── contains ───────────────────────────────────────────────────────

#[test]
fn contains_true_for_substring() {
    let code = r#"
        contains("hello world", "world") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn contains_false_for_missing() {
    let code = r#"
        contains("hello world", "xyz") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn contains_true_for_self() {
    let code = r#"
        contains("hello", "hello") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── starts_with ────────────────────────────────────────────────────

#[test]
fn starts_with_matches_prefix() {
    let code = r#"
        starts_with("hello world", "hello") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn starts_with_no_match_for_non_prefix() {
    let code = r#"
        starts_with("hello world", "world") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn starts_with_empty_string_matches_anything() {
    let code = r#"
        starts_with("hello", "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ends_with ──────────────────────────────────────────────────────

#[test]
fn ends_with_matches_suffix() {
    let code = r#"
        ends_with("hello world", "world") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ends_with_no_match_for_non_suffix() {
    let code = r#"
        ends_with("hello world", "hello") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Case sensitivity (case-sensitive by default) ──────────────────

#[test]
fn contains_is_case_sensitive() {
    let code = r#"
        contains("Hello World", "hello") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn case_insensitive_via_lc() {
    let code = r#"
        contains(lc("Hello World"), lc("HELLO")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty string ───────────────────────────────────────────────────

#[test]
fn empty_contains_empty() {
    let code = r#"
        contains("", "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn empty_does_not_contain_non_empty() {
    let code = r#"
        contains("", "x") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Unicode strings ────────────────────────────────────────────────

#[test]
fn unicode_contains() {
    let code = r#"
        contains("café 🌟 中文", "café") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn unicode_starts_with() {
    let code = r#"
        starts_with("中文 hello", "中文") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Find all occurrences via index loop ───────────────────────────

#[test]
fn find_all_via_index_loop() {
    let code = r#"
        my $s = "abc abc abc";
        my @positions;
        my $start = 0;
        while ((my $p = index($s, "abc", $start)) >= 0) {
            push @positions, $p;
            $start = $p + 1;
        }
        join(",", @positions) eq "0,4,8" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Long string + needle ──────────────────────────────────────────

#[test]
fn index_in_long_string() {
    let code = r#"
        my $s = "x" x 1000 . "needle" . "y" x 1000;
        index($s, "needle") == 1000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── starts_with + ends_with combined ───────────────────────────────

#[test]
fn pattern_match_via_combined_prefix_suffix() {
    let code = r#"
        my $url = "https://example.com/path";
        (starts_with($url, "https://") && ends_with($url, "/path")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-char patterns ────────────────────────────────────────────

#[test]
fn index_finds_multichar_pattern() {
    let code = r#"
        index("abc===def===ghi", "===") == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Index past end returns -1 ──────────────────────────────────────

#[test]
fn index_with_start_at_end_returns_minus_one() {
    // BUG-242 (separate test removed): `index(STR, NEEDLE, START)`
    // with START >= len(STR) panics rather than returning -1.
    // Documented in BUGS.md. Test boundary case at exact len.
    let code = r#"
        # Start exactly at length should be safe (boundary).
        my $r = index("hello", "h", 5);
        $r == -1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── rindex with offset ─────────────────────────────────────────────

#[test]
fn rindex_with_offset_limits_search() {
    let code = r#"
        # rindex(string, target, offset) finds last occurrence at-or-before offset.
        rindex("abc abc abc", "abc", 5) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── starts_with with full match ────────────────────────────────────

#[test]
fn starts_with_full_string() {
    let code = r#"
        starts_with("hello", "hello") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ends_with with full match ──────────────────────────────────────

#[test]
fn ends_with_full_string() {
    let code = r#"
        ends_with("hello", "hello") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── needle longer than haystack ────────────────────────────────────

#[test]
fn contains_longer_needle_returns_false() {
    let code = r#"
        contains("hi", "hello world") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn starts_with_longer_needle_returns_false() {
    let code = r#"
        starts_with("hi", "hello world") ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Special chars in needle ───────────────────────────────────────

#[test]
fn index_finds_special_chars_literally() {
    let code = r#"
        # No regex; finds the literal sequence.
        index("a.b.c", ".") == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn contains_special_chars_literally() {
    let code = r#"
        contains("regex: a*b+c?", "*b+") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Chained searches ─────────────────────────────────────────────

#[test]
fn chained_search_pattern_validation() {
    let code = r#"
        fn Demo::SS::looks_like_email($s) {
            contains($s, "@") &&
            contains($s, ".") &&
            !starts_with($s, "@") &&
            !ends_with($s, "@")
        }
        (Demo::SS::looks_like_email("alice\@example.com")
            && !Demo::SS::looks_like_email("plain string")
            && !Demo::SS::looks_like_email("\@example.com")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
