use crate::common::*;

#[test]
fn regex_match() {
    assert_eq!(eval_int(r#"my $s = "hello123"; $s =~ /(\d+)/; $1"#), 123);
}

#[test]
fn regex_backreference_fancy_fallback() {
    assert_eq!(eval_int(r#"my $s = "aa"; $s =~ /(.)\1/ ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "ab"; $s =~ /(.)\1/ ? 1 : 0"#), 0);
}

#[test]
fn regex_named_captures_plus_hash_and_scalar_brace() {
    assert_eq!(
        eval_string(r#"my $s = "ab"; $s =~ /(?<foo>a)(?<bar>b)/; $+{"foo"} . $+{"bar"}"#),
        "ab"
    );
    // Rust `regex` also accepts Python-style `(?P<name>…)` names.
    assert_eq!(
        eval_string(r#"my $s = "cd"; $s =~ /(?P<foo>c)(?P<bar>d)/; $+{"foo"} . $+{"bar"}"#),
        "cd"
    );
    assert_eq!(
        eval_int(r#"my $s = "xy"; $s =~ /(?<n>x)/; scalar keys %+"#),
        1
    );
    assert_eq!(
        eval_string(r#"my $s = "ax"; $s =~ s/(?<x>a)/b/; $+{"x"}"#),
        "a"
    );
}

#[test]
fn regex_negated_match_operator() {
    assert_eq!(eval_int(r#"my $s = "abc"; $s !~ /xyz/ ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "abc"; $s !~ /a/ ? 1 : 0"#), 0);
}

#[test]
fn regex_dynamic_bind_string_pattern() {
    assert_eq!(
        eval_int(r#"my $s = "hello"; my $p = "ell"; $s =~ $p ? 1 : 0"#),
        1
    );
    assert_eq!(
        eval_int(r#"my $s = "hello"; my $p = "xyz"; $s !~ $p ? 1 : 0"#),
        1
    );
}

#[test]
fn regex_substitution() {
    assert_eq!(
        eval_string(r#"my $s = "foo bar"; $s =~ s/bar/baz/; $s"#),
        "foo baz"
    );
}

#[test]
fn regex_substitution_global() {
    assert_eq!(
        eval_string(r#"my $s = "a a a"; $s =~ s/a/b/g; $s"#),
        "b b b"
    );
}

#[test]
fn transliterate_tr() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; $s =~ tr/abc/ABC/; $s"#),
        "ABC"
    );
}

#[test]
fn transliterate_y_statement_on_dollar_underscore() {
    assert_eq!(eval_string(r#"$_ = "z"; y/z/Z/; $_"#), "Z");
}

#[test]
fn postfix_if_bare_regex_matches_underscore_not_regex_truthiness() {
    assert_eq!(
        eval_string(r#"$_ = "foo.txt"; my $out = ""; $out .= "x" if /\.rs$/; $out"#),
        ""
    );
    assert_eq!(
        eval_string(r#"$_ = "foo.rs"; my $out = ""; $out .= "x" if /\.rs$/; $out"#),
        "x"
    );
}

#[test]
fn postfix_unless_bare_regex_matches_underscore() {
    assert_eq!(
        eval_string(r#"$_ = "foo.rs"; my $out = ""; $out .= "x" unless /\.rs$/; $out"#),
        ""
    );
    assert_eq!(
        eval_string(r#"$_ = "foo.txt"; my $out = ""; $out .= "x" unless /\.rs$/; $out"#),
        "x"
    );
}

#[test]
fn expr_statement_bare_regex_matches_underscore_sets_numbered_captures() {
    assert_eq!(
        eval_string(r#"$_ = "hello world"; /(\w+) (\w+)/; "$1 $2""#),
        "hello world"
    );
}

#[test]
fn expr_statement_bare_regex_bracket_captures_like_perl_minus_e() {
    assert_eq!(
        eval_string(r#"$_ = "hello world"; /(\w+) (\w+)/; "[$1] [$2]""#),
        "[hello] [world]"
    );
}

#[test]
fn expr_statement_bare_regex_last_statement_returns_match_success_scalar() {
    // Last top-level statement is bare `/pat/` — VM must compile like `$_ =~ /pat/`, not `LoadRegex` only.
    assert_eq!(eval_int(r#"$_ = "ab"; /(a)(b)/"#), 1);
    assert_eq!(eval_int(r#"$_ = "xx"; /(a)(b)/"#), 0);
}

#[test]
fn expr_statement_bare_regex_inside_do_block_sets_captures() {
    assert_eq!(
        eval_string(r#"do { $_ = "hi there"; /(\w+) (\w+)/; "$1/$2" }"#),
        "hi/there"
    );
}

/// Sed-style regex flip-flop: operands match `$_`; `$.` drives exclusive `...` (see `perlop`).
#[test]
fn regex_flipflop_two_dot_matches_lines_between_patterns() {
    assert_eq!(
        eval_string(
            r#"my $acc = "";
            my $n = 0;
            for my $line (qw(x a m b y)) {
              $_ = $line;
              $n = $n + 1;
              $. = $n;
              $acc .= $_ if /a/../b/;
            }
            $acc"#
        ),
        "amb"
    );
}

#[test]
fn regex_flipflop_three_dot_matches_lines_between_patterns() {
    assert_eq!(
        eval_string(
            r#"my $acc = "";
            my $n = 0;
            for my $line (qw(x a m b y)) {
              $_ = $line;
              $n = $n + 1;
              $. = $n;
              $acc .= $_ if /a/.../b/;
            }
            $acc"#
        ),
        "amb"
    );
}
