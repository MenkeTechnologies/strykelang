//! Behavior-pinning batch AW (2026-05-08): Sweeping unpinned parity bugs from parity/ docs.

use crate::common::*;

#[test]
fn to_toml_nested_hash_works_now() {
    let out = eval_string(r#"to_toml({a => {b => 1}})"#);
    assert!(out.contains("b = 1"));
}

#[test]
fn pairmap_with_hash_variable_parses_as_modulus() {
    let err = eval_err_kind("my %h = (a=>1); pairmap { 1 } %h;");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("DivisionByZero") || msg.contains("IllegalModulusZero"),
        "expected error about modulus zero, got {:?}",
        err
    );
}

#[test]
fn unpack_b_format_unsupported() {
    let err = eval_err_kind(r#"unpack("B8", "a")"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("Unsupported"),
        "expected runtime error for unsupported pack type, got {:?}",
        err
    );
}

#[test]
fn open_plus_less_than_mode_unknown() {
    let err = eval_err_kind(r#"open my $fh, '+<', "file.txt""#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("UnknownOpenMode"),
        "expected runtime error for unknown open mode, got {:?}",
        err
    );
}

#[test]
fn pmap_chunked_parses_wrong() {
    // `my @out = pmap_chunked 100, { $_ ** 2 } 1..10;`
    let err = parse_err_kind(r#"my @out = pmap_chunked 100, { $_ ** 2 } 1..10;"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Syntax") || msg.contains("Expected") || msg.contains("Unexpected"),
        "expected syntax error for pmap_chunked macro parsing, got {:?}",
        err
    );
}

#[test]
fn fn_signature_hash_works_now() {
    let out = eval_string(r#"fn my_func($a, %h) { return $h{k} } my_func(1, k => 2)"#);
    assert_eq!(out, "2");
}

#[test]
fn format_write_parser_issue() {
    let err = parse_err_kind(r#"format REPORT ="#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Syntax") || msg.contains("Extra tokens in format") || msg.contains("Expected"),
        "expected syntax error for format parsing, got {:?}",
        err
    );
}
