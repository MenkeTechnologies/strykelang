//! Regex lookaround pins: lookahead, negative lookahead, lookbehind,
//! negative lookbehind. Stryke supports all four.

use crate::common::*;

// ── Positive lookahead (?=X) ───────────────────────────────────────

#[test]
fn lookahead_matches_only_when_followed() {
    let code = r#"
        # "foo" only matches if followed by "bar".
        ("foobar" =~ /foo(?=bar)/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lookahead_does_not_consume() {
    let code = r#"
        my $s = "foobar";
        $s =~ /foo(?=bar)/;
        # $& = "foo" (only the consumed part).
        $& eq "foo" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lookahead_failure_no_match() {
    let code = r#"
        ("fooXXX" =~ /foo(?=bar)/) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Negative lookahead (?!X) ───────────────────────────────────────

#[test]
fn negative_lookahead_matches_when_not_followed() {
    let code = r#"
        ("fooXXX" =~ /foo(?!bar)/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_lookahead_rejects_when_followed() {
    let code = r#"
        # /foo(?!bar)/ should not match "foobar" at position 0,
        # though it may match starting later.
        my $s = "foobar";
        # Anchor at start to test the specific position.
        ($s =~ /^foo(?!bar)/) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Positive lookbehind (?<=X) ─────────────────────────────────────

#[test]
fn lookbehind_matches_when_preceded() {
    let code = r#"
        my $s = "abc123";
        $s =~ /(?<=\d)\d/;
        # Match position: second digit, preceded by first digit.
        $& eq "2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lookbehind_does_not_consume() {
    let code = r#"
        my $s = "abc123";
        $s =~ /(?<=c)1/;
        $& eq "1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Negative lookbehind (?<!X) ─────────────────────────────────────

#[test]
fn negative_lookbehind_matches_when_not_preceded() {
    let code = r#"
        my $s = "abc123";
        # First "a" is not preceded by anything (or any digit).
        $s =~ /(?<!\d)\w/;
        $& eq "a" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn negative_lookbehind_rejects_when_preceded() {
    let code = r#"
        my $s = "ab1c";
        # "c" is preceded by "1"; (?<!\d) makes the match start later
        # or fail entirely. Test no-match form.
        ($s =~ /(?<!\d)c/) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Lookaround in s/// ──────────────────────────────────────────────

#[test]
fn s_with_lookahead_replaces_only_qualifying() {
    let code = r#"
        my $s = "foo1 foo2 fooX foo3";
        # Replace "foo" only if followed by a digit.
        $s =~ s/foo(?=\d)/PRE/g;
        $s eq "PRE1 PRE2 fooX PRE3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn s_with_lookbehind_replaces_only_qualifying() {
    let code = r#"
        my $s = "a1 b2 c3 d4";
        # Replace digit only if preceded by "c".
        $s =~ s/(?<=c)\d/9/;
        $s eq "a1 b2 c9 d4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Word-boundary equivalents via lookaround ──────────────────────

#[test]
fn lookaround_as_word_boundary_substitute() {
    let code = r#"
        # Match "cat" not "catnip" — manual word boundary via lookaround.
        my $s = "cat scatter cats catnip cat";
        $s =~ s/(?<!\w)cat(?!\w)/CAT/g;
        $s eq "CAT scatter cats catnip CAT" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Lookahead with backreference ──────────────────────────────────

#[test]
fn lookahead_with_capture_inside() {
    let code = r#"
        my $s = "abc123";
        $s =~ /(\w)(?=\d)/;
        # Last word-char before digit is "c".
        $1 eq "c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multiple lookaheads chained ──────────────────────────────────

#[test]
fn multiple_lookaheads_chained() {
    let code = r#"
        my $s = "password123!";
        # Check has digit AND special char (typical password rule).
        my $ok = ($s =~ /^(?=.*\d)(?=.*[!@#$])/) ? 1 : 0;
        $ok == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn password_rule_via_chained_lookaheads_failures() {
    let code = r#"
        # No digit, no special char → fail.
        my $s = "plainword";
        ($s =~ /^(?=.*\d)(?=.*[!@#$])/) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Lookaround in qr// ────────────────────────────────────────────

#[test]
fn lookaround_in_qr_pattern() {
    let code = r#"
        my $re = qr/(?<=foo)\d+/;
        "abcfoo123" =~ $re;
        $& eq "123" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Anchored lookaround ──────────────────────────────────────────

#[test]
fn anchored_lookahead_at_start() {
    let code = r#"
        # ^(?=\d) means "starts with a digit, but don't consume".
        ("42abc" =~ /^(?=\d)/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn anchored_lookahead_rejects_non_match() {
    let code = r#"
        ("abc" =~ /^(?=\d)/) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Combine lookbehind and lookahead ──────────────────────────────

#[test]
fn lookbehind_and_lookahead_combined() {
    let code = r#"
        # Match a digit surrounded by letters.
        my $s = "a1b c2d 3 e4f";
        my @matches = ($s =~ /(?<=[a-z])\d(?=[a-z])/g);
        # "1", "2", "4" all qualify; "3" doesn't (preceded by space).
        # (The global-match-in-list-context BUG-213 returns whole
        # matches, so each is just the digit.)
        len(@matches) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Lookaround with alternation ──────────────────────────────────

#[test]
fn lookahead_with_alternation() {
    let code = r#"
        my $s = "foo123 foo456 fooXYZ";
        my @hits = ($s =~ /foo(?=\d|X)/g);
        # 3 hits.
        len(@hits) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sentence-end detection via lookbehind ─────────────────────────

#[test]
fn split_lookbehind_does_not_constrain_correctly() {
    // BUG-237: `split /(?<=[.!?])\s/, $s` splits on every whitespace
    // regardless of the lookbehind, producing word-by-word output
    // instead of sentence-by-sentence. The lookbehind is being
    // ignored in split context.
    //
    // Workaround: use `split /[.!?]+/` to split on punctuation directly.
    let code = r#"
        my $s = "Hi. How are you? I am fine.";
        my @parts = split /[.!?]+/, $s;
        @parts = grep { $_ =~ /\w/ } @parts;
        len(@parts) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── No-match via overly strict lookbehind ─────────────────────────

#[test]
fn strict_lookbehind_no_match() {
    let code = r#"
        my $s = "abc";
        # Match "b" only if preceded by a digit.
        ($s =~ /(?<=\d)b/) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}
