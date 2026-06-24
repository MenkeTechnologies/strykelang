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
fn static_allows_anonymous_and_eval_blocks() {
    // Anonymous `fn` / `eval` / `map` blocks all desugar to CodeRef and are NOT
    // required to declare a return type — only named subs and methods are.
    with_global_flags(|| {
        assert!(parse_static("var $f = fn($x: Int) { $x } p \"x\"").is_ok());
        assert!(parse_static("var @a: List<Int> = (1, 2); eval { push @a, 3 } p \"x\"").is_ok());
    });
}

#[test]
fn static_does_not_affect_untyped_program_without_flag() {
    // Same untyped program runs normally when `--static` is off.
    assert_eq!(eval_int("fn add($x, $y) { $x + $y } add(40, 2)"), 42);
}

// ── Parametric value-type containers: Set<T> / Heap<T> / Deque<T> ─────────────

#[test]
fn typed_set_good_runs() {
    assert_eq!(eval_int("var $s: Set<Int> = set(1, 2, 3); 7"), 7);
}

#[test]
fn typed_set_wrong_member_is_type_error() {
    assert_eq!(
        eval_err_kind("var $s: Set<Int> = set(1, \"x\", 3); 1"),
        ErrorKind::Type
    );
}

#[test]
fn typed_heap_decl_ok() {
    assert_eq!(eval_int("var $h: Heap<Int> = heap { $a <=> $b }; 5"), 5);
}

#[test]
fn typed_deque_decl_ok() {
    assert_eq!(eval_int("var $d: Deque<Str> = deque(); 9"), 9);
}

#[test]
fn typed_container_shape_mismatch_is_type_error() {
    // A Set value assigned to a Heap<Int> scalar — wrong container shape.
    assert_eq!(
        eval_err_kind("var $h: Heap<Int> = set(1, 2); 1"),
        ErrorKind::Type
    );
}

// ── Nominal element types: List<STRUCT> / Set<STRUCT> / STRUCT scalar ─────────

#[test]
fn list_of_struct_accepts_instances() {
    assert_eq!(
        eval_int("struct Pt { x => Int } var @ps: List<Pt> = (Pt(x=>1), Pt(x=>2)); scalar(@ps)"),
        2
    );
}

#[test]
fn list_of_struct_rejects_non_struct() {
    assert_eq!(
        eval_err_kind("struct Pt { x => Int } var @ps: List<Pt> = (\"nope\"); 1"),
        ErrorKind::Type
    );
}

#[test]
fn struct_typed_scalar_ok() {
    assert_eq!(
        eval_int("struct Pt { x => Int } var $p: Pt = Pt(x=>5); $p->{x}"),
        5
    );
}

#[test]
fn set_of_struct_accepts_instances() {
    assert_eq!(
        eval_int("struct Pt { x => Int } var $s: Set<Pt> = set(Pt(x=>1)); 4"),
        4
    );
}

// ── `val` collections are immutable (no in-place mutation) ───────────────────

#[test]
fn val_deque_mutation_rejected() {
    // `val` forbids the in-place mutating methods of the collection it holds.
    assert_eq!(
        eval_err_kind("val $q = deque(); $q->push_back(1)"),
        ErrorKind::Runtime
    );
}

#[test]
fn val_heap_mutation_rejected() {
    assert_eq!(
        eval_err_kind("val $h = heap { $a <=> $b }; $h->push(1)"),
        ErrorKind::Runtime
    );
}

#[test]
fn val_deque_pop_rejected() {
    assert_eq!(
        eval_err_kind("val $q = deque(); $q->pop_front"),
        ErrorKind::Runtime
    );
}

#[test]
fn var_deque_mutation_allowed() {
    assert_eq!(eval_int("var $q = deque(); $q->push_back(1); $q->len"), 1);
}

#[test]
fn val_deque_read_methods_allowed() {
    // Read-only methods on a `val` collection are fine.
    assert_eq!(eval_int("val $q = deque(); $q->len"), 0);
}
