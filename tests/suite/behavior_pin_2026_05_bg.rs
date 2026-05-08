//! Behavior-pinning batch BG (2026-05-08): Sweeping unpinned errors in Web builtins.

use crate::common::*;

#[test]
fn web_model_increment_without_args_errors() {
    let err = eval_err_kind("web_model_increment()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("table name required"));
}

#[test]
fn web_model_with_without_args_errors() {
    let err = eval_err_kind("web_model_with()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("table name required"));
}

#[test]
fn web_content_for_without_args_errors() {
    let err = eval_err_kind("web_content_for()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("usage"));
}

#[test]
fn web_render_partial_without_args_errors() {
    let err = eval_err_kind("web_render_partial()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("usage"));
}

#[test]
fn web_token_for_without_args_errors() {
    let err = eval_err_kind("web_token_for()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("user_id required"));
}

#[test]
fn web_jsonapi_resource_without_args_errors() {
    let err = eval_err_kind("web_jsonapi_resource()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("type required"));
}

#[test]
fn web_jsonapi_collection_without_args_errors() {
    let err = eval_err_kind("web_jsonapi_collection()");
    let msg = format!("{:?}", err);
    assert!(msg.contains("Runtime") || msg.contains("type required"));
}
