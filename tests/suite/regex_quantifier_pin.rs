//! Regex quantifier pins.

use crate::common::*;

// ── Star (0 or more) ───────────────────────────────────────────────

#[test]
fn star_matches_zero_occurrences() {
    let code = r#"
        ("xyz" =~ /a*xyz/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn star_matches_many_occurrences() {
    let code = r#"
        ("aaaaxyz" =~ /a*xyz/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Plus (1 or more) ───────────────────────────────────────────────

#[test]
fn plus_requires_at_least_one() {
    let code = r#"
        ("xyz" =~ /^a+xyz/) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn plus_matches_with_one_occurrence() {
    let code = r#"
        ("axyz" =~ /^a+xyz/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn plus_matches_with_many() {
    let code = r#"
        ("aaaxyz" =~ /^a+xyz/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Question (0 or 1) ──────────────────────────────────────────────

#[test]
fn question_matches_zero_or_one() {
    let code = r#"
        my $r1 = ("xyz" =~ /^a?xyz/);
        my $r2 = ("axyz" =~ /^a?xyz/);
        my $r3 = ("aaxyz" =~ /^a?xyz/);
        ($r1 && $r2 && !$r3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Exact count {N} ────────────────────────────────────────────────

#[test]
fn exact_count_requires_exactly_n() {
    let code = r#"
        my $r1 = ("aaaxyz" =~ /^a{3}xyz/);
        my $r2 = ("aaxyz" =~ /^a{3}xyz/);
        my $r3 = ("aaaaxyz" =~ /^a{3}xyz/);
        # {3} requires exactly 3; {3} as min-only on r3 also matches 3.
        # In Perl, /a{3}/ is exactly 3 in default. But /a{3}/ followed
        # by anything more than aaa still matches at the right anchor.
        # r1 anchored ^a{3}xyz matches; r3 has 4 a's so ^a{3}xyz doesn't
        # match at position 0 (4 a's followed by xyz, ^a{3} consumes 3
        # then xyz expected — there's still an 'a' before xyz).
        ($r1 && !$r2 && !$r3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range count {N,M} ──────────────────────────────────────────────

#[test]
fn range_count_lower_upper() {
    let code = r#"
        my $r1 = ("aaxyz" =~ /^a{2,4}xyz/);
        my $r2 = ("aaaaxyz" =~ /^a{2,4}xyz/);
        my $r3 = ("axyz" =~ /^a{2,4}xyz/);
        ($r1 && $r2 && !$r3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn open_range_min_only() {
    let code = r#"
        # {2,} means 2 or more.
        my $r1 = ("aaxyz" =~ /^a{2,}xyz/);
        my $r2 = ("aaaaaaaxyz" =~ /^a{2,}xyz/);
        my $r3 = ("axyz" =~ /^a{2,}xyz/);
        ($r1 && $r2 && !$r3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Greedy vs non-greedy ──────────────────────────────────────────

#[test]
fn greedy_consumes_max() {
    let code = r#"
        my $s = "aaabbb";
        $s =~ /^(a+)(b+)/;
        # Greedy: $1 = "aaa", $2 = "bbb".
        ($1 eq "aaa" && $2 eq "bbb") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn non_greedy_consumes_min() {
    let code = r#"
        my $s = "aaabbb";
        $s =~ /^(a+?)(.+)/;
        # Non-greedy: $1 = "a", $2 = "aabbb".
        ($1 eq "a" && $2 eq "aabbb") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn non_greedy_star() {
    let code = r#"
        my $s = "<a><b>";
        $s =~ /^<(.*?)>/;
        # Non-greedy: $1 = "a", not "a><b".
        $1 eq "a" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn greedy_star_consumes_all_possible() {
    let code = r#"
        my $s = "<a><b>";
        $s =~ /^<(.*)>/;
        # Greedy: $1 = "a><b".
        $1 eq "a><b" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Zero-width matches ────────────────────────────────────────────

#[test]
fn word_boundary_zero_width() {
    let code = r#"
        my $s = "the cat sat";
        my @hits = ($s =~ /\bcat\b/g);
        len(@hits) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn anchored_zero_width_assertion() {
    let code = r#"
        # ^ is zero-width.
        ("abc" =~ /^abc/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Quantifier on group ───────────────────────────────────────────

#[test]
fn quantifier_on_capture_group() {
    let code = r#"
        my $s = "ababab";
        $s =~ /^(ab)+$/;
        # Last iteration of repeated capture wins (Perl rule).
        $1 eq "ab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn count_repeats_via_quantifier_on_group() {
    let code = r#"
        my $s = "ababab";
        ($s =~ /^(?:ab){3}$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn count_repeats_fails_for_wrong_count() {
    let code = r#"
        my $s = "abab";
        ($s =~ /^(?:ab){3}$/) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Quantifier with character class ───────────────────────────────

#[test]
fn quantifier_on_character_class() {
    let code = r#"
        ("abc123" =~ /^[a-z]+\d+$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn quantifier_on_negated_class() {
    let code = r#"
        ("abc123" =~ /^[^0-9]+/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── \d / \w / \s with quantifiers ─────────────────────────────────

#[test]
fn quantified_digit_class() {
    let code = r#"
        ("123abc" =~ /^\d{3}/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn quantified_word_class() {
    let code = r#"
        ("hello_42" =~ /^\w+$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn quantified_whitespace_class() {
    let code = r#"
        ("hello   world" =~ /^hello\s+world$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Dot-star + anchor combinations ────────────────────────────────

#[test]
fn dot_star_matches_any_inner_content() {
    let code = r#"
        ("foo123bar" =~ /^foo.*bar$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_does_not_match_newline_by_default() {
    let code = r#"
        # /a.b/ does NOT match "a\nb" without /s.
        ("a\nb" =~ /^a.b$/) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_with_s_flag_matches_newline() {
    let code = r#"
        # /s makes . match newlines.
        ("a\nb" =~ /^a.b$/s) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Long-running pattern smoke test ───────────────────────────────

#[test]
fn many_quantifiers_chained() {
    let code = r#"
        my $s = "abc123def456ghi789";
        ($s =~ /^[a-z]+\d+[a-z]+\d+[a-z]+\d+$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Backreference with quantifier ─────────────────────────────────

#[test]
fn backref_with_quantifier() {
    let code = r#"
        # (\w+) followed by same word once more.
        my $s = "hello hello";
        ($s =~ /^(\w+) \1$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Match longest substring in /g loop ────────────────────────────

#[test]
fn longest_substring_g_loop_count() {
    let code = r#"
        my $s = "aaabbbcccddd";
        my @hits = ($s =~ /(\w)\1+/g);
        # 4 runs of same letter.
        len(@hits) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Lazy quantifier in capture ────────────────────────────────────

#[test]
fn lazy_quantifier_captures_min_string() {
    let code = r#"
        my $s = "<aaa><bbb>";
        my @tags = ($s =~ /<(.*?)>/g);
        # 2 tags.
        len(@tags) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── + with character class ────────────────────────────────────────

#[test]
fn plus_with_negated_charclass_for_csv_field() {
    let code = r#"
        my $row = "alice,30,admin";
        my @fields = ($row =~ /([^,]+)/g);
        len(@fields) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Quantifier in s/// pattern ────────────────────────────────────

#[test]
fn collapse_repeated_letters_via_quantifier() {
    let code = r#"
        my $s = "heeello woorld";
        $s =~ s/(.)\1+/$1/g;
        $s eq "helo world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
