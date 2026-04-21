//! Parser rejects invalid Perl (explicit `#[test]` per case; no macro batching).

use crate::common::parse_err_kind;
use stryke::error::ErrorKind;

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
    // Perl: `fn { 1 }` is a valid statement (void-context coderef).
    assert!(stryke::parse("fn { 1 }").is_ok());
}

#[test]
fn rejects_unclosed_paren_expression() {
    assert!(stryke::parse("(1 + 2").is_err());
}

#[test]
fn rejects_unclosed_bracket_array() {
    assert!(stryke::parse("$a[1").is_err());
}

#[test]
fn rejects_unclosed_brace_hash() {
    assert!(stryke::parse("$h{1").is_err());
}

#[test]
fn rejects_unclosed_regex_delimiter() {
    assert!(stryke::parse("m/foo").is_err());
}

#[test]
fn rejects_incomplete_addition_at_eof() {
    assert!(stryke::parse("1 +").is_err());
}

#[test]
fn double_semicolon_only_parses() {
    stryke::parse(";").expect("parse");
}

#[test]
fn rejects_incomplete_mul_at_eof() {
    assert!(stryke::parse("3 *").is_err());
}

#[test]
fn rejects_unclosed_single_quote() {
    assert!(stryke::parse("'unterminated").is_err());
}

#[test]
fn rejects_sub_call_missing_paren_if_expected() {
    assert!(stryke::parse("my $x = (").is_err());
}

#[test]
fn comment_only_line_parses_as_empty_program() {
    let p = stryke::parse("# only comment\n").expect("parse");
    assert!(p.statements.is_empty());
}

#[test]
fn rejects_interpolated_eof_in_double_quote() {
    assert!(parse_err_kind(r#""${"#) == ErrorKind::Syntax || stryke::parse(r#""${"#).is_err());
}

#[test]
fn rejects_package_name_invalid() {
    assert!(stryke::parse("package 123").is_err());
}

#[test]
fn triple_semicolon_parses() {
    stryke::parse(";;").expect("semicolons only");
}

#[test]
fn rejects_eof_after_my() {
    assert!(stryke::parse("my").is_err());
}

#[test]
fn rejects_eof_after_comma_in_list() {
    assert!(stryke::parse("(1,").is_err());
}

#[test]
fn rejects_unclosed_q_brace() {
    assert!(stryke::parse(r#"q{no close"#).is_err());
}

#[test]
fn rejects_s_substitute_unclosed() {
    assert!(stryke::parse("s/a/").is_err());
}

#[test]
fn rejects_tr_unclosed() {
    assert!(stryke::parse("tr/a/").is_err());
}

#[test]
fn rejects_foreach_without_paren_or_keyword() {
    assert!(stryke::parse("foreach").is_err());
}

#[test]
fn rejects_if_without_paren_on_some_engines() {
    assert!(stryke::parse("if").is_err());
}

#[test]
fn rejects_do_string_unclosed_quote() {
    assert!(stryke::parse(r#"do "file"#).is_err());
}

#[test]
fn use_strict_without_trailing_semicolon_still_parses() {
    stryke::parse("use strict").expect("parse");
}

#[test]
fn rejects_open_missing_comma() {
    assert!(stryke::parse("open F").is_err());
}

#[test]
fn rejects_backslash_eof() {
    assert!(stryke::parse("\\").is_err());
}

#[test]
fn rejects_incomplete_qq_constructor() {
    assert!(stryke::parse("qq(").is_err());
}
