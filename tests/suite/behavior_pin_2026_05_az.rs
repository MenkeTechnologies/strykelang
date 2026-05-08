//! Behavior-pinning batch AZ (2026-05-08): Sweeping remaining unpinned LSP doc bugs.

use crate::common::*;

#[test]
fn set_union_method_not_implemented() {
    let err = eval_err_kind(r#"my $s = set(1, 2); $s->union(set(3));"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("UnknownMethod"),
        "expected runtime error for missing union method on set, got {:?}",
        err
    );
}

#[test]
fn ppool_without_args_errors() {
    let err = eval_err_kind(r#"ppool()"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("Unsupported") || msg.contains("Compile"),
        "expected runtime error for ppool arity, got {:?}",
        err
    );
}

#[test]
fn datetime_parse_local_without_timezone_errors() {
    let err = eval_err_kind(r#"datetime_parse_local('2026-04-15 14:30:00')"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("UnknownTimezone"),
        "expected runtime error for empty timezone, got {:?}",
        err
    );
}

#[test]
fn retry_backoff_arg_validation_fails_parsing() {
    let err = parse_err_kind(r#"retry { 1 } times => 3, backoff => 'exponential'"#);
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Syntax") || msg.contains("Expected") || msg.contains("expected backoff mode"),
        "expected syntax error for retry backoff string, got {:?}",
        err
    );
}

#[test]
fn pipe_forward_on_my_decl_with_glob_parses_successfully() {
    // This used to fail parsing, but now it successfully parses
    let program = stryke::parse(r#"my @datasets = par_csv_read glob("data/*.csv") |> join(",");"#);
    assert!(program.is_ok(), "Expected pipe forward to parse successfully");
}
