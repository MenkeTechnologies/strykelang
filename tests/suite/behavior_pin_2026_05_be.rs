//! Behavior-pinning batch BE (2026-05-08): Sweeping unpinned errors in Web ORM builtins.

use crate::common::*;

#[test]
fn web_model_all_without_args_errors() {
    let err = eval_err_kind("web_model_all()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("table name required"),
        "expected runtime error for web_model_all arity, got {:?}",
        err
    );
}

#[test]
fn web_model_find_without_args_errors() {
    let err = eval_err_kind("web_model_find()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("table name required"),
        "expected runtime error for web_model_find arity, got {:?}",
        err
    );
}

#[test]
fn web_db_execute_without_args_errors() {
    let err = eval_err_kind("web_db_execute()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("sql required"),
        "expected runtime error for web_db_execute arity, got {:?}",
        err
    );
}

#[test]
fn web_model_create_without_args_errors() {
    let err = eval_err_kind("web_model_create()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("table name required"),
        "expected runtime error for web_model_create arity, got {:?}",
        err
    );
}

#[test]
fn web_model_destroy_without_args_errors() {
    let err = eval_err_kind("web_model_destroy()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("table name required"),
        "expected runtime error for web_model_destroy arity, got {:?}",
        err
    );
}

#[test]
fn web_migrate_without_db_connection_errors() {
    let err = eval_err_kind("web_migrate()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("no database connection"),
        "expected runtime error for web_migrate without db, got {:?}",
        err
    );
}

#[test]
fn web_create_table_without_args_errors() {
    let err = eval_err_kind("web_create_table()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("table name required"),
        "expected runtime error for web_create_table arity, got {:?}",
        err
    );
}

#[test]
fn web_validate_without_hashref_errors() {
    let err = eval_err_kind("web_validate()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("first arg must be a hashref"),
        "expected runtime error for web_validate arity, got {:?}",
        err
    );
}

#[test]
fn web_db_query_without_args_errors() {
    let err = eval_err_kind("web_db_query()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("sql required"),
        "expected runtime error for web_db_query arity, got {:?}",
        err
    );
}

#[test]
fn web_db_begin_without_db_connection_errors() {
    let err = eval_err_kind("web_db_begin()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("no database connection"),
        "expected runtime error for web_db_begin without db, got {:?}",
        err
    );
}
