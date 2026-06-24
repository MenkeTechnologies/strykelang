//! Pins for stryke's static typing: parametric container types (`List<T>` /
//! `Map<K, V>`), `fn ... : Type` return types, and the `--static` mandatory-
//! typing pass.
//!
//! Runtime-enforcement cases (return types, typed containers) run with the
//! shared read lock via [`crate::common::eval`] / `eval_err_kind`. The
//! `--static` cases flip the process-global `static_mode` flag and therefore
//! hold the write lock via [`crate::common::with_global_flags`].

use crate::common::{eval_int, eval_err_kind, with_global_flags};
use stryke::error::ErrorKind;

// ── Return-type enforcement (`fn f(): Type`) ─────────────────────────────────

#[test]
fn return_type_satisfied_runs() {
    assert_eq!(eval_int("fn addi($x: Int): Int { $x + 1 } addi(41)"), 42);
}

#[test]
fn return_type_violation_is_type_error() {
    // Body yields an Int but the declared return type is Str.
    assert_eq!(
        eval_err_kind("fn bad($x: Int): Str { $x + 1 } bad(2)"),
        ErrorKind::Type
    );
}

#[test]
fn return_type_str_from_interpolation_ok() {
    assert_eq!(
        eval_int("fn lbl($x: Int): Str { \"n=$x\" } length(lbl(99))"),
        4
    );
}

// ── Parametric container element typing (recursive `check_value`) ─────────────

#[test]
fn typed_list_good_elements_run() {
    assert_eq!(
        eval_int("var @a: List<Str> = (\"x\", \"y\", \"z\"); scalar(@a)"),
        3
    );
}

#[test]
fn typed_list_wrong_element_at_decl_is_type_error() {
    assert_eq!(
        eval_err_kind("var @a: List<Str> = (1, 2, 3); p \"@a\""),
        ErrorKind::Type
    );
}

#[test]
fn typed_list_push_violation_is_type_error() {
    assert_eq!(
        eval_err_kind("var @a: List<Str> = (\"x\"); push @a, 5; p \"@a\""),
        ErrorKind::Type
    );
}

#[test]
fn typed_list_element_store_violation_is_type_error() {
    assert_eq!(
        eval_err_kind("var @a: List<Str> = (\"x\"); $a[1] = 9; p \"@a\""),
        ErrorKind::Type
    );
}

#[test]
fn typed_map_value_violation_is_type_error() {
    assert_eq!(
        eval_err_kind("var %h: Map<Str, Int> = (a => \"notint\"); p \"ok\""),
        ErrorKind::Type
    );
}

#[test]
fn typed_map_element_write_violation_is_type_error() {
    assert_eq!(
        eval_err_kind("var %h: Map<Str, Int> = (a => 1); $h{b} = \"no\"; p \"ok\""),
        ErrorKind::Type
    );
}

#[test]
fn typed_map_good_runs() {
    assert_eq!(
        eval_int("var %h: Map<Str, Int> = (a => 1, b => 2); $h{a} + $h{b}"),
        3
    );
}

#[test]
fn nested_generic_return_violation_is_type_error() {
    // `List<Int>` returned containing a string — recursion into the element type.
    assert_eq!(
        eval_err_kind("fn mk(): List<Int> { (1, \"x\", 3) } join(\",\", mk())"),
        ErrorKind::Type
    );
}

// ── `--static` mandatory-typing pass (parse-time abort) ──────────────────────

/// Parse `code` with `--static` active, restoring the flag before asserting.
fn parse_static(code: &str) -> stryke::error::StrykeResult<stryke::ast::Program> {
    stryke::set_static_mode(true);
    let r = stryke::parse(code);
    stryke::set_static_mode(false);
    r
}

#[test]
fn static_fully_typed_program_accepts() {
    with_global_flags(|| {
        assert!(parse_static(
            "fn add($x: Int, $y: Int): Int { $x + $y } var $n: Int = 5 p add(2, 3)"
        )
        .is_ok());
    });
}

#[test]
fn static_missing_return_type_rejected() {
    with_global_flags(|| {
        assert!(parse_static("fn add($x: Int, $y: Int) { $x + $y } p add(2, 3)").is_err());
    });
}

#[test]
fn static_missing_param_type_rejected() {
    with_global_flags(|| {
        assert!(parse_static("fn add($x, $y: Int): Int { $x + $y } p add(2, 3)").is_err());
    });
}

#[test]
fn static_var_without_type_or_init_rejected() {
    with_global_flags(|| {
        assert!(parse_static("var $x: Int = 1 var $y p $x").is_err());
    });
}

#[test]
fn static_var_inferred_from_initializer_accepts() {
    // No annotation, but an initializer the type can be inferred from.
    with_global_flags(|| {
        assert!(parse_static("var $x = 5 p $x").is_ok());
    });
}

#[test]
fn static_literal_type_mismatch_rejected() {
    with_global_flags(|| {
        assert!(parse_static("val $y: Int = \"hi\" p $y").is_err());
    });
}

#[test]
fn static_anonymous_fn_requires_return_type() {
    with_global_flags(|| {
        assert!(parse_static("var $f = fn($x: Int) { $x } p \"x\"").is_err());
    });
}

#[test]
fn static_does_not_affect_untyped_program_without_flag() {
    // Same untyped program runs normally when `--static` is off.
    assert_eq!(eval_int("fn add($x, $y) { $x + $y } add(40, 2)"), 42);
}
