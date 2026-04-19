//! Additional syntax-error cases (explicit `#[test]`; no macro batching).

use crate::common::parse_err_kind;
use forge::error::ErrorKind;

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
    assert!(forge::parse(r#"my $x = "@{"#).is_err());
}

#[test]
fn rejects_invalid_numeric_double_quote_interpolation() {
    // Perl: Numeric variables with more than one digit may not start with '0'
    assert!(forge::parse(r##"my $x = "$01""##).is_err());
}

#[test]
fn missing_comma_in_list() {
    assert!(forge::parse("(1 2)").is_err());
}

#[test]
fn statement_anonymous_sub_empty_prototype_parses() {
    assert!(forge::parse("sub () { }").is_ok());
}

#[test]
fn double_operator_eof() {
    assert_eq!(parse_err_kind("**"), ErrorKind::Syntax);
}

#[test]
fn slash_only() {
    assert!(forge::parse("/").is_err());
}

#[test]
fn m_regex_missing_closing_delimiter() {
    assert!(forge::parse("m/a").is_err());
}

#[test]
fn package_eof_after_keyword() {
    assert!(forge::parse("package").is_err());
}

#[test]
fn use_eof_after_keyword() {
    assert!(forge::parse("use").is_err());
}

#[test]
fn my_eof_after_paren_open() {
    assert!(forge::parse("my $x = (").is_err());
}

#[test]
fn hash_key_incomplete() {
    assert!(forge::parse("$h{").is_err());
}

#[test]
fn array_index_incomplete() {
    assert!(forge::parse("$a[").is_err());
}

#[test]
fn quotelike_unclosed_paren() {
    assert!(forge::parse("qq(").is_err());
}
