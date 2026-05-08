//! Behavior-pinning batch BF (2026-05-08): Sweeping unpinned errors in AI builtins.

use crate::common::*;

#[test]
fn ai_file_get_without_args_errors() {
    let err = eval_err_kind("ai_file_get()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("file_id required"));
}

#[test]
fn ai_file_delete_without_args_errors() {
    let err = eval_err_kind("ai_file_delete()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("file_id required"));
}

#[test]
fn ai_image_edit_without_args_errors() {
    let err = eval_err_kind("ai_image_edit()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("prompt required"));
}

#[test]
fn ai_image_variation_without_args_errors() {
    let err = eval_err_kind("ai_image_variation()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("pass image =>"));
}

#[test]
fn ai_chunk_without_args_errors() {
    let err = eval_err_kind("ai_chunk()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("text required"));
}

#[test]
fn ai_compare_without_args_errors() {
    let err = eval_err_kind("ai_compare()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("first input required"));
}

#[test]
fn ai_describe_without_args_errors() {
    let err = eval_err_kind("ai_describe()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("image path/url required"));
}

#[test]
fn ai_session_send_without_args_errors() {
    let err = eval_err_kind("ai_session_send()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("first arg must be a session handle"));
}

#[test]
fn ai_session_history_without_args_errors() {
    let err = eval_err_kind("ai_session_history()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("session handle required"));
}

#[test]
fn ai_batch_without_args_errors() {
    let err = eval_err_kind("ai_batch()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("prompt list required"));
}

#[test]
fn ai_pmap_without_args_errors() {
    let err = eval_err_kind("ai_pmap()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("items array required"));
}

#[test]
fn ai_budget_without_args_errors() {
    let err = eval_err_kind("ai_budget()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("usd cap required"));
}

#[test]
fn ai_summarize_without_args_errors() {
    let err = eval_err_kind("ai_summarize()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("text required"));
}

#[test]
fn ai_translate_without_args_errors() {
    let err = eval_err_kind("ai_translate()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("text required"));
}

#[test]
fn ai_extract_without_args_errors() {
    let err = eval_err_kind("ai_extract()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("prompt required"));
}
