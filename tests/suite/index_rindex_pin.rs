//! `index` / `rindex` builtin pins — boundary cases and edge behavior.
//!
//! Pins include observed bugs:
//!   * BUG-254: `index($s, $needle, NEG)` panics on any negative offset.
//!   * BUG-255: `rindex($s, $needle, NEG)` panics/misbehaves on
//!     negative offset.
//!
//! See docs/BUGS.md for details. The pins use safe (non-negative)
//! offsets only; the bug-pin tests in behavior_pin assert the failure
//! mode separately (currently as panics — would need `should_panic`
//! to lock; deferred to a future round).

use crate::common::*;

// ── index basics ──────────────────────────────────────────────────

#[test]
fn index_finds_first_occurrence() {
    assert_eq!(eval_int(r#"index("abracadabra", "ab") == 0 ? 1 : 0"#), 1);
}

#[test]
fn index_with_start_skips_earlier_match() {
    assert_eq!(eval_int(r#"index("abracadabra", "ab", 1) == 7 ? 1 : 0"#), 1);
}

#[test]
fn index_not_found_returns_minus_one() {
    assert_eq!(eval_int(r#"index("abracadabra", "zz") == -1 ? 1 : 0"#), 1);
}

#[test]
fn index_exact_match_starts_at_zero() {
    assert_eq!(eval_int(r#"index("abc", "abc") == 0 ? 1 : 0"#), 1);
}

#[test]
fn index_longer_needle_returns_minus_one() {
    assert_eq!(eval_int(r#"index("abc", "abcdef") == -1 ? 1 : 0"#), 1);
}

#[test]
fn index_empty_needle_returns_zero() {
    // Empty string matches at position 0.
    assert_eq!(eval_int(r#"index("abc", "") == 0 ? 1 : 0"#), 1);
}

#[test]
fn index_empty_needle_with_start_returns_start() {
    // Empty matches everywhere; with offset returns that offset.
    assert_eq!(eval_int(r#"index("abcdef", "", 3) == 3 ? 1 : 0"#), 1);
}

#[test]
fn index_empty_haystack_empty_needle_zero() {
    assert_eq!(eval_int(r#"index("", "") == 0 ? 1 : 0"#), 1);
}

#[test]
fn index_empty_haystack_nonempty_needle_minus_one() {
    assert_eq!(eval_int(r#"index("", "a") == -1 ? 1 : 0"#), 1);
}

#[test]
fn index_start_at_match_position_finds_it() {
    assert_eq!(eval_int(r#"index("abcabc", "bc", 1) == 1 ? 1 : 0"#), 1);
}

#[test]
fn index_start_past_match_finds_next() {
    assert_eq!(eval_int(r#"index("abcabc", "bc", 2) == 4 ? 1 : 0"#), 1);
}

#[test]
fn index_overlapping_finds_first_only() {
    // "aaaa" with needle "aa": positions 0, 1, 2 all match; index
    // returns 0; with start=1 returns 1.
    let code = r#"
        my $a = index("aaaa", "aa");
        my $b = index("aaaa", "aa", 1);
        my $c = index("aaaa", "aa", 2);
        my $d = index("aaaa", "aa", 3);
        ($a == 0 && $b == 1 && $c == 2 && $d == -1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_with_newline_needle() {
    assert_eq!(eval_int(r#"index("a\nb\nc", "\n") == 1 ? 1 : 0"#), 1);
}

#[test]
fn index_at_exact_end_minus_one() {
    let code = r#"
        my $s = "hello";
        index($s, "o", 4) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── rindex basics ─────────────────────────────────────────────────

#[test]
fn rindex_finds_last_occurrence() {
    assert_eq!(eval_int(r#"rindex("abracadabra", "ab") == 7 ? 1 : 0"#), 1);
}

#[test]
fn rindex_single_char() {
    assert_eq!(eval_int(r#"rindex("abracadabra", "a") == 10 ? 1 : 0"#), 1);
}

#[test]
fn rindex_with_start_caps_search_position() {
    // rindex(s, n, pos) — find last match starting at or before pos.
    assert_eq!(eval_int(r#"rindex("abracadabra", "a", 5) == 5 ? 1 : 0"#), 1);
}

#[test]
fn rindex_not_found_returns_minus_one() {
    assert_eq!(eval_int(r#"rindex("abracadabra", "zz") == -1 ? 1 : 0"#), 1);
}

#[test]
fn rindex_empty_needle_returns_haystack_length() {
    // Empty matches "at end" — returns length.
    assert_eq!(eval_int(r#"rindex("hello", "") == 5 ? 1 : 0"#), 1);
}

#[test]
fn rindex_empty_haystack_empty_needle() {
    assert_eq!(eval_int(r#"rindex("", "") == 0 ? 1 : 0"#), 1);
}

#[test]
fn rindex_start_beyond_length_clamps() {
    // Stryke clamps the start offset to the haystack length.
    assert_eq!(
        eval_int(r#"rindex("abracadabra", "ab", 999) == 7 ? 1 : 0"#),
        1
    );
}

#[test]
fn rindex_with_start_zero_finds_only_first() {
    // pos=0 means "match must end at-or-before pos 0".
    assert_eq!(eval_int(r#"rindex("abcabc", "abc", 0) == 0 ? 1 : 0"#), 1);
}

#[test]
fn rindex_overlap_returns_rightmost() {
    let code = r#"
        my $r = rindex("aaaa", "aa");
        $r == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── derived idioms ────────────────────────────────────────────────

#[test]
fn count_occurrences_via_index_loop() {
    let code = r#"
        my $s = "abracadabra";
        my $needle = "a";
        my $count = 0;
        my $pos = 0;
        while ($pos < length($s)) {
            my $found = index($s, $needle, $pos);
            last if $found < 0;
            $count++;
            $pos = $found + 1;
        }
        $count == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn find_all_positions_via_index_loop() {
    let code = r#"
        my $s = "abracadabra";
        my @positions;
        my $pos = 0;
        while ($pos < length($s)) {
            my $found = index($s, "ab", $pos);
            last if $found < 0;
            push @positions, $found;
            $pos = $found + 1;
        }
        join(",", @positions) eq "0,7" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn extract_file_extension_via_rindex() {
    let code = r#"
        my $path = "report.tar.gz";
        my $dot = rindex($path, ".");
        my $ext = substr($path, $dot + 1);
        $ext eq "gz" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn extract_basename_via_rindex_slash() {
    let code = r#"
        my $path = "/usr/local/bin/perl";
        my $slash = rindex($path, "/");
        my $base = substr($path, $slash + 1);
        $base eq "perl" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn contains_check_via_index_minus_one() {
    let code = r#"
        my $contains_xyz = index("hello world", "xyz") >= 0 ? 1 : 0;
        my $contains_orl = index("hello world", "orl") >= 0 ? 1 : 0;
        (!$contains_xyz && $contains_orl) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_at_first_separator_via_index() {
    let code = r#"
        my $line = "key=value=more";
        my $eq = index($line, "=");
        my $key = substr($line, 0, $eq);
        my $val = substr($line, $eq + 1);
        ($key eq "key" && $val eq "value=more") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── relationship index ↔ rindex on unique needle ─────────────────

#[test]
fn index_eq_rindex_when_needle_appears_once() {
    let code = r#"
        my $s = "hello world";
        index($s, "w") == rindex($s, "w") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_le_rindex_when_needle_repeats() {
    let code = r#"
        my $s = "abcabc";
        index($s, "b") < rindex($s, "b") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── 10k haystack search ──────────────────────────────────────────

#[test]
fn index_in_10k_string_finds_planted_marker() {
    let code = r#"
        my $s = "a" x 4999 . "NEEDLE" . "b" x 4995;
        my $pos = index($s, "NEEDLE");
        $pos == 4999 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rindex_in_10k_string_finds_last_planted() {
    let code = r#"
        my $s = "FIRST" . ("x" x 5000) . "LAST";
        rindex($s, "LAST") == 5005 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── case-sensitivity ─────────────────────────────────────────────

#[test]
fn index_is_case_sensitive() {
    let code = r#"
        index("Hello", "h") == -1 && index("Hello", "H") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rindex_is_case_sensitive() {
    let code = r#"
        rindex("Hello", "l") == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── start = length is the same as length-1 boundary ──────────────

#[test]
fn index_start_at_length_returns_minus_one_for_nonempty_needle() {
    let code = r#"
        my $s = "abc";
        index($s, "a", 3) == -1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_start_at_length_with_empty_needle_returns_length() {
    let code = r#"
        my $s = "abc";
        index($s, "", 3) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
