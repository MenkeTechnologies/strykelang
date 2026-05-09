//! Behavior-pinning batch AY (2026-05-08): Sweeping unpinned parity bugs from parity/ docs.

use crate::common::*;

#[test]
fn indented_heredoc_does_not_strip_whitespace_today() {
    let out = eval_string("<<~EOF\n  test\nEOF\n");
    assert_eq!(out, "  test\n");
}

#[test]
fn uppercase_escape_works_now() {
    let out = eval_string(r#""\Uabc\E""#);
    assert_eq!(out, "ABC");
}

#[test]
fn inline_expression_interpolation_works_now() {
    let out = eval_string(r#""A ${\(1+2)} B""#);
    assert_eq!(out, "A 3 B");
}

#[test]
fn syswrite_requires_three_arguments_today() {
    let err = eval_err_kind(r#"open my $fh, '>', '/tmp/out'; syswrite($fh, "data");"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime"),
        "expected runtime error for syswrite arity, got {:?}",
        err
    );
}

#[test]
fn syswrite_with_stdout_bareword_errors_today() {
    let err = eval_err_kind(r#"syswrite(STDOUT, "data", 4);"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime"),
        "expected runtime error for unopened handle STDOUT, got {:?}",
        err
    );
}
