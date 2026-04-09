//! Regex flags, captures, and substitutions beyond the basic suite.

use crate::common::*;

#[test]
fn match_is_case_insensitive_with_i_flag() {
    assert_eq!(eval_int(r#"my $s = "HELLO"; $s =~ /hello/i"#), 1);
}

#[test]
fn match_populates_numbered_captures() {
    assert_eq!(
        eval_int(r#"my $s = "a92c"; $s =~ /(\d)(\d)/; $1 * 10 + $2"#),
        92
    );
}

#[test]
fn substitution_respects_word_boundaries_without_g() {
    assert_eq!(
        eval_string(r#"my $s = "foo foo"; $s =~ s/foo/bar/; $s"#),
        "bar foo"
    );
}

#[test]
fn match_dot_matches_newline_with_s_flag() {
    // Perl double-quoted `\n` must appear in the source; avoid raw-string line continuations
    // (they inject stray backslashes into the Perl program).
    assert_eq!(eval_int("my $s = \"a\\nb\"; $s =~ /a.b/s"), 1);
}
