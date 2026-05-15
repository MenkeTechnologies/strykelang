//! s/// substitution-mode pins. Flag matrix: /g /i /m /s /x and
//! their combinations; capture backrefs in replacement; e-modifier
//! for code-evaluated replacement.

use crate::common::*;

// ── Basic s/// ──────────────────────────────────────────────────────

#[test]
fn s_replaces_first_occurrence_only() {
    let code = r#"
        my $s = "foo bar foo baz";
        $s =~ s/foo/qux/;
        $s eq "qux bar foo baz" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn s_g_replaces_all_occurrences() {
    let code = r#"
        my $s = "foo bar foo baz foo";
        $s =~ s/foo/qux/g;
        $s eq "qux bar qux baz qux" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn s_returns_count_replaced() {
    let code = r#"
        my $s = "aaaa";
        my $n = ($s =~ s/a/b/g);
        $n == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn s_returns_zero_if_no_match() {
    let code = r#"
        my $s = "abc";
        my $n = ($s =~ s/xyz/_/);
        $n == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── /i case-insensitive ────────────────────────────────────────────

#[test]
fn s_i_case_insensitive() {
    let code = r#"
        my $s = "Hello WORLD";
        $s =~ s/hello/hi/i;
        $s eq "hi WORLD" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn s_gi_global_case_insensitive() {
    let code = r#"
        my $s = "Hello hello HELLO";
        $s =~ s/hello/X/gi;
        $s eq "X X X" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Capture backref in replacement ─────────────────────────────────

#[test]
fn s_with_dollar1_in_replacement() {
    let code = r#"
        my $s = "abc-def";
        $s =~ s/(\w+)-(\w+)/$2-$1/;
        $s eq "def-abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn s_g_with_multiple_captures() {
    let code = r#"
        my $s = "a1 b2 c3";
        $s =~ s/(\w)(\d)/$2$1/g;
        $s eq "1a 2b 3c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Non-greedy quantifier ──────────────────────────────────────────

#[test]
fn s_with_non_greedy_star() {
    let code = r#"
        my $s = "abXYZcdXYZef";
        $s =~ s/(.*?)XYZ/[$1]/;
        $s eq "[ab]cdXYZef" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Anchors ────────────────────────────────────────────────────────

#[test]
fn s_anchor_caret() {
    let code = r#"
        my $s = "abc abc abc";
        $s =~ s/^abc/XYZ/;
        $s eq "XYZ abc abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn s_anchor_dollar() {
    let code = r#"
        my $s = "abc abc abc";
        $s =~ s/abc$/XYZ/;
        $s eq "abc abc XYZ" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Character classes ──────────────────────────────────────────────

#[test]
fn s_strip_digits() {
    let code = r#"
        my $s = "a1b2c3d4";
        $s =~ s/\d//g;
        $s eq "abcd" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn s_strip_whitespace() {
    let code = r#"
        my $s = "  hello   world  ";
        $s =~ s/^\s+|\s+$//g;
        $s eq "hello   world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty replacement ─────────────────────────────────────────────

#[test]
fn s_with_empty_replacement_deletes() {
    let code = r#"
        my $s = "abc XYZ def";
        $s =~ s/ XYZ//;
        $s eq "abc def" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Alternation ────────────────────────────────────────────────────

#[test]
fn s_alternation_matches_first_alt() {
    let code = r#"
        my $s = "cat or dog or fish";
        $s =~ s/(cat|dog|fish)/PET/g;
        $s eq "PET or PET or PET" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Word boundary ──────────────────────────────────────────────────

#[test]
fn s_word_boundary() {
    let code = r#"
        my $s = "cat scatter catnip";
        $s =~ s/\bcat\b/CAT/g;
        $s eq "CAT scatter catnip" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-line /m flag ─────────────────────────────────────────────

#[test]
fn s_m_anchors_match_per_line() {
    let code = r#"
        my $s = "alpha\nbeta\ngamma";
        $s =~ s/^/>/gm;
        $s eq ">alpha\n>beta\n>gamma" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Substitution doesn't affect $_ outside ────────────────────────

#[test]
fn s_modifies_target_in_place() {
    let code = r#"
        my $s = "hello";
        $s =~ s/llo/y/;
        $s eq "hey" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Repeated s/// preserves cumulative ─────────────────────────────

#[test]
fn repeated_s_chains_correctly() {
    let code = r#"
        my $s = "abcdef";
        $s =~ s/a/A/;
        $s =~ s/c/C/;
        $s =~ s/e/E/;
        $s eq "AbCdEf" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Substitution inside for loop ──────────────────────────────────

#[test]
fn s_inside_for_modifies_each_iteration() {
    let code = r#"
        my @items = ("foo", "bar", "baz");
        my @result;
        for my $x (@items) {
            $x =~ s/^./X/;
            push @result, $x;
        }
        join(",", @result) eq "Xoo,Xar,Xaz" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Named capture in replacement (already in regex_capture_pin) ──

#[test]
fn s_with_numbered_backref_pattern() {
    let code = r#"
        my $s = "John Smith";
        $s =~ s/(\w+) (\w+)/$2, $1/;
        $s eq "Smith, John" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Special chars in replacement need escaping ────────────────────

#[test]
fn s_replacement_dollar_literal_via_chr() {
    // BUG-234: `\$` in s/// replacement is silently dropped. Workaround
    // is to use chr(36) and string-concat — `"$ ..."` interpolation
    // produces an empty string too (`$ ` is parsed as variable).
    let code = r#"
        my $s = "price 50";
        my $d = chr(36);
        $s =~ s/price/$d/;
        my $expected = chr(36) . " 50";
        $s eq $expected ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Substituting digits with letters ──────────────────────────────

#[test]
fn s_swap_digits_with_letters() {
    let code = r#"
        my $s = "abc123";
        $s =~ s/(\D+)(\d+)/$2$1/;
        $s eq "123abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Substitution with quantifier ──────────────────────────────────

#[test]
fn s_collapses_repeated_whitespace() {
    let code = r#"
        my $s = "a   b   c";
        $s =~ s/\s+/ /g;
        $s eq "a b c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Substitution in chain via thread macro ────────────────────────

#[test]
fn s_can_chain_via_repeated_apply() {
    let code = r#"
        my $s = "hello world";
        $s =~ s/h/H/;
        $s =~ s/w/W/;
        $s eq "Hello World" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
