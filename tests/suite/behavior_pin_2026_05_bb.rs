//! Behavior-pinning batch BB (2026-05-08): Sweeping unpinned bugs in newly added builtins (AI, PTY, Web).

use crate::common::*;

#[test]
#[should_panic(expected = "range start index 1 out of range")]
fn ai_models_without_args_panics() {
    // BUG: `ai_models()` currently panics because it assumes an argument was passed.
    // We pin this panic so that when it is fixed, this test will fail and can be
    // updated to check for a proper runtime error or default behavior.
    eval_string("ai_models()");
}

#[test]
fn pty_spawn_without_args_errors() {
    let err = eval_err_kind("pty_spawn()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("usage"),
        "expected runtime error for pty_spawn arity, got {:?}",
        err
    );
}

#[test]
fn mcp_connect_without_args_errors() {
    let err = eval_err_kind("mcp_connect()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("usage"),
        "expected runtime error for mcp_connect arity, got {:?}",
        err
    );
}

#[test]
fn web_route_without_args_errors() {
    let err = eval_err_kind("web_route()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("usage"),
        "expected runtime error for web_route arity, got {:?}",
        err
    );
}

#[test]
fn ai_transcribe_without_args_errors() {
    let err = eval_err_kind("ai_transcribe()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("usage"),
        "expected runtime error for ai_transcribe arity, got {:?}",
        err
    );
}

#[test]
fn ai_file_upload_without_args_errors() {
    let err = eval_err_kind("ai_file_upload()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("path required"),
        "expected runtime error for ai_file_upload arity, got {:?}",
        err
    );
}
