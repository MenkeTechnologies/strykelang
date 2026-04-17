//! Parser rejects invalid Perl (explicit `#[test]` per case; no macro batching).

use crate::common::parse_err_kind;
use perlrs::error::ErrorKind;

#[test]
fn rejects_unclosed_sub_brace() {
    assert_eq!(parse_err_kind("sub f {"), ErrorKind::Syntax);
}

#[test]
fn rejects_lone_close_brace() {
    assert_eq!(parse_err_kind("}"), ErrorKind::Syntax);
}

#[test]
fn rejects_unterminated_double_quote() {
    assert_eq!(parse_err_kind(r#"my $x = "open"#), ErrorKind::Syntax);
}

#[test]
fn rejects_unexpected_eof_after_operator() {
    assert_eq!(parse_err_kind("++"), ErrorKind::Syntax);
}

#[test]
fn rejects_invalid_token_triple_dollar() {
    assert!(parse_err_kind("$$$").eq(&ErrorKind::Syntax));
}

#[test]
fn statement_anonymous_sub_block_parses() {
    // Perl: `sub { 1 }` is a valid statement (void-context coderef).
    assert!(perlrs::parse("sub { 1 }").is_ok());
}

#[test]
fn rejects_unclosed_paren_expression() {
    assert!(perlrs::parse("(1 + 2").is_err());
}

#[test]
fn rejects_unclosed_bracket_array() {
    assert!(perlrs::parse("$a[1").is_err());
}

#[test]
fn rejects_unclosed_brace_hash() {
    assert!(perlrs::parse("$h{1").is_err());
}

#[test]
fn rejects_unclosed_regex_delimiter() {
    assert!(perlrs::parse("m/foo").is_err());
}

#[test]
fn rejects_incomplete_addition_at_eof() {
    assert!(perlrs::parse("1 +").is_err());
}

#[test]
fn double_semicolon_only_parses() {
    perlrs::parse(";").expect("parse");
}

#[test]
fn rejects_incomplete_mul_at_eof() {
    assert!(perlrs::parse("3 *").is_err());
}

#[test]
fn rejects_unclosed_single_quote() {
    assert!(perlrs::parse("'unterminated").is_err());
}

#[test]
fn rejects_sub_call_missing_paren_if_expected() {
    assert!(perlrs::parse("my $x = (").is_err());
}

#[test]
fn comment_only_line_parses_as_empty_program() {
    let p = perlrs::parse("# only comment\n").expect("parse");
    assert!(p.statements.is_empty());
}

#[test]
fn rejects_interpolated_eof_in_double_quote() {
    assert!(parse_err_kind(r#""${"#) == ErrorKind::Syntax || perlrs::parse(r#""${"#).is_err());
}

#[test]
fn rejects_package_name_invalid() {
    assert!(perlrs::parse("package 123").is_err());
}

#[test]
fn triple_semicolon_parses() {
    perlrs::parse(";;").expect("semicolons only");
}

#[test]
fn rejects_eof_after_my() {
    assert!(perlrs::parse("my").is_err());
}

#[test]
fn rejects_eof_after_comma_in_list() {
    assert!(perlrs::parse("(1,").is_err());
}

#[test]
fn rejects_unclosed_q_brace() {
    assert!(perlrs::parse(r#"q{no close"#).is_err());
}

#[test]
fn rejects_s_substitute_unclosed() {
    assert!(perlrs::parse("s/a/").is_err());
}

#[test]
fn rejects_tr_unclosed() {
    assert!(perlrs::parse("tr/a/").is_err());
}

#[test]
fn rejects_foreach_without_paren_or_keyword() {
    assert!(perlrs::parse("foreach").is_err());
}

#[test]
fn rejects_if_without_paren_on_some_engines() {
    assert!(perlrs::parse("if").is_err());
}

#[test]
fn rejects_do_string_unclosed_quote() {
    assert!(perlrs::parse(r#"do "file"#).is_err());
}

#[test]
fn use_strict_without_trailing_semicolon_still_parses() {
    perlrs::parse("use strict").expect("parse");
}

#[test]
fn rejects_open_missing_comma() {
    assert!(perlrs::parse("open F").is_err());
}

#[test]
fn rejects_backslash_eof() {
    assert!(perlrs::parse("\\").is_err());
}

#[test]
fn rejects_incomplete_qq_constructor() {
    assert!(perlrs::parse("qq(").is_err());
}
