//! Behavior-pinning batch AX (2026-05-08): Sweeping unpinned parity bugs from parity/ docs.

use crate::common::*;

#[test]
fn try_block_without_catch_errors() {
    let err = parse_err_kind(r#"try { die "error" };"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Syntax") || msg.contains("Expected") || msg.contains("expected 'catch' after try block"),
        "expected syntax error for try without catch, got {:?}",
        err
    );
}

#[test]
fn eof_without_parens_parses_wrong() {
    let err = parse_err_kind(r#"until (eof $fh) { }"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Syntax") || msg.contains("Expected") || msg.contains("Expected RParen, got ScalarVar"),
        "expected syntax error for eof parsing, got {:?}",
        err
    );
}

#[test]
fn deque_with_args_errors() {
    let err = eval_err_kind(r#"deque(1, 2, 3)"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("Unsupported"),
        "expected runtime error for deque with args, got {:?}",
        err
    );
}

#[test]
fn heap_without_comparator_errors() {
    let err = eval_err_kind(r#"heap(5, 3, 8, 1)"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("Unsupported"),
        "expected runtime error for heap without comparator, got {:?}",
        err
    );
}

#[test]
fn use_fcntl_flock_errors() {
    let err = eval_err_kind(r#"use Fcntl ':flock';"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("Syntax") || msg.contains("Expected"),
        "expected runtime or syntax error when using Fcntl, got {:?}",
        err
    );
}
