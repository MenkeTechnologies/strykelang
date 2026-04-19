//! Additional syntax-error cases (explicit `#[test]`; no macro batching).

use crate::common::parse_err_kind;
use stryke::error::ErrorKind;

#[test]
fn eof_after_minus() {
    assert_eq!(parse_err_kind("-"), ErrorKind::Syntax);
}

#[test]
fn eof_after_dot() {
    assert_eq!(parse_err_kind("."), ErrorKind::Syntax);
}

#[test]
fn eof_after_comma() {
    assert_eq!(parse_err_kind(","), ErrorKind::Syntax);
}

#[test]
fn unclosed_interpolation_brace_in_string() {
    assert!(stryke::parse(r#"my $x = "@{"#).is_err());
}

#[test]
fn rejects_invalid_numeric_double_quote_interpolation() {
    // Perl: Numeric variables with more than one digit may not start with '0'
    assert!(stryke::parse(r##"my $x = "$01""##).is_err());
}

#[test]
fn missing_comma_in_list() {
    assert!(stryke::parse("(1 2)").is_err());
}

#[test]
fn statement_anonymous_sub_empty_prototype_parses() {
    assert!(stryke::parse("sub () { }").is_ok());
}

#[test]
fn double_operator_eof() {
    assert_eq!(parse_err_kind("**"), ErrorKind::Syntax);
}

#[test]
fn slash_only() {
    assert!(stryke::parse("/").is_err());
}

#[test]
fn m_regex_missing_closing_delimiter() {
    assert!(stryke::parse("m/a").is_err());
}

#[test]
fn package_eof_after_keyword() {
    assert!(stryke::parse("package").is_err());
}

#[test]
fn use_eof_after_keyword() {
    assert!(stryke::parse("use").is_err());
}

#[test]
fn my_eof_after_paren_open() {
    assert!(stryke::parse("my $x = (").is_err());
}

#[test]
fn hash_key_incomplete() {
    assert!(stryke::parse("$h{").is_err());
}

#[test]
fn array_index_incomplete() {
    assert!(stryke::parse("$a[").is_err());
}

#[test]
fn quotelike_unclosed_paren() {
    assert!(stryke::parse("qq(").is_err());
}
