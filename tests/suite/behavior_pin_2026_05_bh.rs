//! Behavior-pinning batch BH (2026-05-08): Sweeping unpinned errors in Git builtins.

use crate::common::*;

#[test]
fn git_root_invalid_path_errors() {
    let err = eval_err_kind("git_root('/nonexistent')");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("failed to resolve path"),
        "expected runtime error for invalid git_root path, got {:?}",
        err
    );
}

#[test]
fn git_branches_invalid_path_errors() {
    let err = eval_err_kind("git_branches('/nonexistent')");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("failed to resolve path"),
        "expected runtime error for invalid git_branches path, got {:?}",
        err
    );
}

#[test]
fn git_files_invalid_path_errors() {
    let err = eval_err_kind("git_files('/nonexistent')");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("failed to resolve path"),
        "expected runtime error for invalid git_files path, got {:?}",
        err
    );
}

#[test]
fn git_authors_invalid_path_errors() {
    let err = eval_err_kind("git_authors('/nonexistent')");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("failed to resolve path"),
        "expected runtime error for invalid git_authors path, got {:?}",
        err
    );
}

#[test]
fn git_log_invalid_path_does_not_error_today() {
    // BUG: git_log doesn't error out on invalid paths currently, unlike the others.
    let out = eval_string("eval { git_log('/nonexistent') }; $@");
    assert_eq!(out, "", "expected no error for git_log on invalid path today");
}
