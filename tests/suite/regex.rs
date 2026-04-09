use crate::common::*;

#[test]
fn regex_match() {
    assert_eq!(eval_int(r#"my $s = "hello123"; $s =~ /(\d+)/; $1"#), 123);
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
