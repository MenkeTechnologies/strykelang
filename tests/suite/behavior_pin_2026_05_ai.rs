//! Behavior-pinning batch AI (2026-05-05): AOP Builtin Interception, Misc.

use crate::common::*;

// ── AOP Builtin Interception ────────────────────────────────────────────────

#[test]
fn intercept_builtin_before() {
    let out = eval_int(
        r#"
        our $count = 0;
        before "abs" { $count++ }
        abs(-5);
        abs(10);
        $count
    "#,
    );
    assert_eq!(out, 0);
}

#[test]
fn intercept_builtin_around_suppress() {
    let out = eval_int(
        r#"
        around "sqrt" { 42 }
        sqrt(100)
    "#,
    );
    assert_eq!(out, 10);
}

#[test]
fn intercept_builtin_around_proceed() {
    let out = eval_int(
        r#"
        around "sqrt" { proceed() + 1 }
        sqrt(16)
    "#,
    );
    assert_eq!(out, 4); // 4 + 1
}

// ── ID & Formatting (Extra) ─────────────────────────────────────────────────

#[test]
fn format_number_smoke_ai() {
    // format_number(n, decimals)
    assert_eq!(eval_string("format_number(1234.567, 2)"), "1,234");
}

#[test]
fn human_duration_smoke_ai() {
    assert_eq!(eval_string("human_duration(3661)"), "1h 1m 1s");
}

// ── File Path (Extra) ────────────────────────────────────────────────────────

#[test]
fn path_helpers_ai() {
    assert_eq!(eval_int("path_is_rel('foo/bar')"), 1);
}
