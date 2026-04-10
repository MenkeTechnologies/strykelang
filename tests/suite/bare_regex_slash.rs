//! Bare slash-delimited regex `/…/` must behave like `$_ =~ /…/` in boolean and void (statement)
//! contexts: match against `$_`, set `$1`…`$n`, `$&`, `%-`/`@+`, etc. — not a regex object.
//! See `compile_boolean_rvalue_condition` / `compile_expr_ctx(..., Void)` / short-circuit `&&`/`||`.

use crate::common::*;

#[test]
fn bare_slash_if_and_elsif_set_numbered_captures() {
    assert_eq!(
        eval_string(
            r#"$_ = "12"; my $x = ""; if (/x/) { $x = "bad" } elsif (/(\d)/) { $x = $1 } $x"#
        ),
        "1"
    );
}

#[test]
fn bare_slash_if_else_branch() {
    assert_eq!(
        eval_string(
            r#"$_ = "no"; my $x = ""; if (/^yes$/) { $x = $1 } else { $x = "else" } $x"#
        ),
        "else"
    );
}

#[test]
fn bare_slash_unless_runs_body_when_match_fails() {
    assert_eq!(
        eval_string(r#"$_ = "nope"; my $x = ""; unless (/^yes$/) { $x = "ok" } $x"#),
        "ok"
    );
}

#[test]
fn bare_slash_unless_skips_body_when_match_succeeds() {
    assert_eq!(
        eval_string(r#"$_ = "yes"; my $x = "unset"; unless (/yes/) { $x = "bad" } $x"#),
        "unset"
    );
}

#[test]
fn bare_slash_log_and_short_circuits_and_sets_capture_from_right() {
    assert_eq!(
        eval_string(r#"$_ = "foo"; my $y = /(z)/ && $1; $y"#),
        "0"
    );
    assert_eq!(
        eval_string(r#"$_ = "foo"; my $y = /(o+)/ && $1; $y"#),
        "oo"
    );
}

#[test]
fn bare_slash_log_or_falls_through_to_right_when_left_fails() {
    assert_eq!(
        eval_string(r#"$_ = "a"; my $y = /(z)/ || "fallback"; $y"#),
        "fallback"
    );
}

#[test]
fn bare_slash_log_or_left_match_yields_boolean_not_capture() {
    // `&&` / `||` with bare `/pat/` on a side use boolean match semantics (truthy int), not the
    // capture string — same as Perl’s boolean short-circuit.
    assert_eq!(eval_int(r#"$_ = "ab"; my $y = /(a)/ || 0; $y"#), 1);
}

#[test]
fn bare_slash_unary_not_on_regex() {
    assert_eq!(eval_int(r#"$_ = "b"; !/a/"#), 1);
    assert_eq!(eval_int(r#"$_ = "a"; !/a/"#), 0);
}

#[test]
fn bare_slash_while_condition_log_and_with_bare_regex() {
    assert_eq!(
        eval_string(
            r#"my $out = ""; my $n = 0; $_ = "ab"; while ($n < 1 && /(a)/) { $out = $1; $n++ } $out"#
        ),
        "a"
    );
}

#[test]
fn bare_slash_until_runs_until_match() {
    assert_eq!(
        eval_int(
            r#"$_ = "x"; my $n = 0; until (/a/) { $n++; last } $n"#
        ),
        1
    );
}

#[test]
fn bare_slash_c_style_for_condition_sets_captures_each_match() {
    assert_eq!(
        eval_string(
            r#"my $out = ""; for ($_ = "ab"; /(a)(b)/; $_ = "") { $out = "$1$2"; } $out"#
        ),
        "ab"
    );
}

#[test]
fn bare_slash_do_while_tests_condition_against_underscore() {
    assert_eq!(
        eval_int(r#"$_ = "z"; my $n = 0; do { $n++ } while (/a/); $n"#),
        1
    );
}

#[test]
fn bare_slash_match_string_dollar_ampersand() {
    assert_eq!(eval_string(r#"$_ = "abc"; /b/; $&"#), "b");
}

#[test]
fn bare_slash_case_insensitive_flag() {
    assert_eq!(eval_int(r#"$_ = "ABC"; /abc/i ? 1 : 0"#), 1);
    assert_eq!(
        eval_string(r#"$_ = "xAbCy"; /(ab)/i; $1"#),
        "Ab"
    );
}

#[test]
fn bare_slash_sub_body_statement_match() {
    assert_eq!(
        eval_string(r#"my $c = sub { $_ = "ab"; /(a)/; $1 }; $c->()"#),
        "a"
    );
}

#[test]
fn bare_slash_postfix_while_condition() {
    assert_eq!(
        eval_int(
            r#"my $n = 0; $_ = "a"; $n++ while ($n < 2 && /a/); $n"#
        ),
        2
    );
}

#[test]
fn bare_slash_postfix_until_condition() {
    assert_eq!(
        eval_int(
            r#"my $n = 0; $_ = "x"; $n++ until ($n > 0 || /a/); $n"#
        ),
        1
    );
}

#[test]
fn bare_slash_ternary_uses_capture_from_match() {
    assert_eq!(
        eval_string(r#"$_ = "ab"; /(a)/ ? $1 : "no""#),
        "a"
    );
}

#[test]
fn bare_slash_log_or_second_regex_when_first_fails() {
    assert_eq!(
        eval_int(r#"$_ = "zz"; /(x)/ || /(z)/"#),
        1
    );
    assert_eq!(
        eval_string(r#"$_ = "zz"; /(x)/ || /(z)/; $1"#),
        "z"
    );
}

#[test]
fn bare_slash_sequential_matches_update_numbered_captures() {
    assert_eq!(
        eval_string(r#"$_ = "ab"; /(a)/; my $x = $1; /(b)/; "$x$1""#),
        "ab"
    );
}
