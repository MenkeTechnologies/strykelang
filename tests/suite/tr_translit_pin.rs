//! tr/// (aka y///) transliteration operator pins.
//!
//! Pins both the working features (count, range, basic translit, /d,
//! /r) AND the broken flags (/c, /s) — observed broken on stryke.
//! See BUG-251 (/c ignored) and BUG-252 (/s ignored) in docs/BUGS.md.

use crate::common::*;

// ── basic translit ────────────────────────────────────────────────

#[test]
fn tr_uppercase_via_range() {
    let code = r#"
        my $s = "Hello World";
        $s =~ tr/a-z/A-Z/;
        $s eq "HELLO WORLD" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_lowercase_via_range() {
    let code = r#"
        my $s = "Hello World";
        $s =~ tr/A-Z/a-z/;
        $s eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_swap_pairs() {
    let code = r#"
        my $s = "abcabc";
        $s =~ tr/abc/xyz/;
        $s eq "xyzxyz" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_single_char_to_single_char() {
    let code = r#"
        my $s = "fffff";
        $s =~ tr/f/g/;
        $s eq "ggggg" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_digits_to_underscores() {
    let code = r#"
        my $s = "abc123def";
        $s =~ tr/0-9/_/;
        $s eq "abc___def" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── count via empty replacement ────────────────────────────────────

#[test]
fn tr_count_with_empty_replacement_does_not_modify() {
    let code = r#"
        my $s = "Hello World";
        my $n = ($s =~ tr/aeiou//);
        ($n == 3 && $s eq "Hello World") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_count_digits() {
    let code = r#"
        my $s = "phone 555-1234";
        my $n = ($s =~ tr/0-9//);
        $n == 7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_count_all_alpha() {
    let code = r#"
        my $s = "Hello World";
        my $n = ($s =~ tr/a-zA-Z//);
        $n == 10 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_count_empty_string_is_zero() {
    let code = r#"
        my $s = "";
        my $n = ($s =~ tr/a-z//);
        $n == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── /d (delete) flag ──────────────────────────────────────────────

#[test]
fn tr_delete_alpha() {
    let code = r#"
        my $s = "Hello, World!";
        $s =~ tr/A-Za-z//d;
        $s eq ", !" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_delete_vowels() {
    let code = r#"
        my $s = "supercalifragilistic";
        $s =~ tr/aeiou//d;
        $s eq "sprclfrglstc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_delete_returns_count() {
    let code = r#"
        my $s = "abc123def";
        my $n = ($s =~ tr/0-9//d);
        ($n == 3 && $s eq "abcdef") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── /r (non-destructive return) flag ─────────────────────────────

#[test]
fn tr_r_returns_modified_keeps_source() {
    let code = r#"
        my $s = "abcdef";
        my $r = ($s =~ tr/a-f/X/r);
        ($s eq "abcdef" && $r eq "XXXXXX") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_r_with_range_to_range() {
    let code = r#"
        my $s = "abc";
        my $r = ($s =~ tr/a-z/A-Z/r);
        ($s eq "abc" && $r eq "ABC") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_r_count_via_assignment() {
    let code = r#"
        my $s = "aaabbb";
        my $r = ($s =~ tr/a/x/r);
        $r eq "xxxbbb" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── /c (complement) flag ─────────────────────────────────────────

#[test]
fn tr_c_flag_complements_search_list() {
    // `tr/0-9//c` counts the non-digit characters (5 in "abcde123").
    let code = r#"
        my $s = "abcde123";
        my $no_c = ($s =~ tr/0-9//);
        my $with_c = ($s =~ tr/0-9//c);
        ($no_c == 3 && $with_c == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_cd_combination_keeps_complement_and_deletes_others() {
    // `tr/A-Za-z//cd` keeps alphabetic characters and deletes everything
    // else, yielding "HelloWorld".
    let code = r#"
        my $s = "Hello, World!";
        $s =~ tr/A-Za-z//cd;
        $s eq "HelloWorld" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── /s (squeeze) flag ────────────────────────────────────────────

#[test]
fn tr_s_flag_squeezes_runs() {
    // `tr/abc//s` collapses consecutive matched characters to one each:
    // "aaabbbccc" → "abc".
    let code = r#"
        my $s = "aaabbbccc";
        $s =~ tr/abc//s;
        $s eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_translit_s_combo_translates_then_squeezes() {
    // `tr/abc/xyz/s` translates each character then squeezes the runs
    // of the translated output: "aaabbbccc" → "xyz".
    let code = r#"
        my $s = "aaabbbccc";
        $s =~ tr/abc/xyz/s;
        $s eq "xyz" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── range edge cases ─────────────────────────────────────────────

#[test]
fn tr_range_includes_endpoints() {
    let code = r#"
        my $s = "a..z";
        my $n = ($s =~ tr/a-z//);
        $n == 2 ? 1 : 0   # only the literal 'a' and 'z'
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_digit_range_subset() {
    let code = r#"
        my $s = "0123456789";
        $s =~ tr/3-7/_/;
        $s eq "012_____89" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── shorter replacement list (last char repeats in Perl, but…) ──

#[test]
fn tr_short_replacement_last_char_repeats() {
    let code = r#"
        # In Perl, the last char of the replacement list repeats to
        # cover the search list. tr/a-c/X/ -> "X" repeats: aaa => XXX.
        my $s = "abc";
        $s =~ tr/a-c/X/;
        $s eq "XXX" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── y/// is an alias for tr/// ────────────────────────────────────

#[test]
fn y_is_alias_for_tr() {
    let code = r#"
        my $s = "abc";
        $s =~ y/a-c/x-z/;
        $s eq "xyz" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── tr in list context ───────────────────────────────────────────

#[test]
fn tr_count_in_arithmetic() {
    let code = r#"
        my $s = "Mississippi";
        my $count = ($s =~ tr/s//);
        $count * 2 == 8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── tr in loops ───────────────────────────────────────────────────

#[test]
fn tr_with_loop_strings() {
    let code = r#"
        my @lines = ("foo", "BAR", "Baz");
        for my $line (@lines) {
            $line =~ tr/a-z/A-Z/;
        }
        join(",", @lines) eq "FOO,BAR,BAZ" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── tr-based hex digit count ──────────────────────────────────────

#[test]
fn tr_count_hex_chars() {
    let code = r#"
        my $s = "DEADBEEF";
        my $n = ($s =~ tr/0-9A-F//);
        $n == 8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── tr-based vowel count is a classic idiom ──────────────────────

#[test]
fn tr_vowel_count_idiom() {
    let code = r#"
        my $s = "supercalifragilisticexpialidocious";
        my $vowels = ($s =~ tr/aeiouAEIOU//);
        $vowels == 16 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── tr with newline in search ─────────────────────────────────────

#[test]
fn tr_newline_removal() {
    let code = r#"
        my $s = "a\nb\nc\n";
        $s =~ tr/\n//d;
        $s eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── tr counting on large string ───────────────────────────────────

#[test]
fn tr_count_10k_string() {
    let code = r#"
        my $s = "a" x 5000 . "b" x 5000;
        my $a_count = ($s =~ tr/a//);
        my $b_count = ($s =~ tr/b//);
        ($a_count == 5000 && $b_count == 5000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
