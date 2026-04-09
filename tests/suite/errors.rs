use crate::common::*;
use perlrs::error::ErrorKind;

#[test]
fn parse_unclosed_brace_is_syntax_error() {
    let err = perlrs::parse("sub f {").unwrap_err();
    assert_eq!(err.kind, ErrorKind::Syntax);
}

#[test]
fn parse_lone_brace_is_syntax_error() {
    assert_eq!(parse_err_kind("}"), ErrorKind::Syntax);
}

#[test]
fn division_by_zero_is_runtime_error() {
    assert_eq!(eval_err_kind("1 / 0"), ErrorKind::Runtime);
}

#[test]
fn modulus_zero_is_runtime_error() {
    assert_eq!(eval_err_kind("1 % 0"), ErrorKind::Runtime);
}

#[test]
fn die_is_die_kind() {
    assert_eq!(eval_err_kind(r#"die "stop""#), ErrorKind::Die);
}
