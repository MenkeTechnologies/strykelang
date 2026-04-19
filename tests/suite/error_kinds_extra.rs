//! Extra `ErrorKind` coverage via `parse_err_kind` / `eval_err_kind` (explicit tests; no batching).

use crate::common::{eval_err_kind, parse_err_kind};
use stryke::error::ErrorKind;

#[test]
fn parse_eof_after_open_paren_is_syntax() {
    assert_eq!(parse_err_kind("("), ErrorKind::Syntax);
}

#[test]
fn parse_eof_after_open_brace_is_syntax() {
    assert_eq!(parse_err_kind("{"), ErrorKind::Syntax);
}

#[test]
fn parse_invalid_sigil_sequence_is_syntax() {
    assert_eq!(parse_err_kind("@"), ErrorKind::Syntax);
}

#[test]
fn division_by_zero_runtime() {
    assert_eq!(eval_err_kind("7 / 0"), ErrorKind::Runtime);
}

#[test]
fn modulo_by_zero_runtime() {
    assert_eq!(eval_err_kind("7 % 0"), ErrorKind::Runtime);
}

#[test]
fn die_string_runtime_die_kind() {
    assert_eq!(eval_err_kind(r#"die "x""#), ErrorKind::Die);
}

#[test]
fn die_bare_is_die_kind() {
    assert_eq!(eval_err_kind("die"), ErrorKind::Die);
}

#[test]
fn exit_nonzero_is_exit_kind() {
    assert_eq!(eval_err_kind("exit(42)"), ErrorKind::Exit(42));
}

#[test]
fn exit_negative_is_exit_kind() {
    assert_eq!(eval_err_kind("exit(-1)"), ErrorKind::Exit(-1));
}

#[test]
fn bare_exit_statement_is_exit_zero_kind() {
    assert_eq!(eval_err_kind("exit"), ErrorKind::Exit(0));
}

#[test]
fn undefined_bare_subroutine_call_is_runtime_kind() {
    assert_eq!(
        eval_err_kind("lib_api_undefined_sub_xyz999()"),
        ErrorKind::Runtime
    );
}
