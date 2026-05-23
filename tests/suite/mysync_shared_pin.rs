//! Pin `mysync` shared-state semantics per `docs/STYLE_GUIDE.md`
//! §10: stryke's shared-state primitive that supports lockless reads
//! and lock-on-write — and unlike plain `my`, is **visible from
//! inside `fn` bodies** because closures capture `mysync` by
//! reference, not by value. Probed against the running interpreter
//! on 2026-05-23.
//!
//! These tests run single-threaded to pin the observational
//! contract; concurrency stress is covered by other files in the
//! suite (parallel_*.rs).

use crate::common::*;

#[test]
fn mysync_scalar_basic_assign_and_read() {
    let code = r#"
        mysync $count = 0;
        $count = 7;
        $count == 7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_scalar_compound_assign() {
    let code = r#"
        mysync $count = 0;
        $count += 5;
        $count += 10;
        $count == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_scalar_increment() {
    let code = r#"
        mysync $count = 0;
        $count++;
        $count++;
        $count++;
        $count == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_scalar_visible_from_inside_fn() {
    // The key difference from `my`: a `mysync` scalar IS visible
    // and mutable from inside a closure or fn body.
    let code = r#"
        mysync $shared = 0;
        fn Demo::Ms::bump { $shared++ }
        Demo::Ms::bump();
        Demo::Ms::bump();
        Demo::Ms::bump();
        $shared == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_array_push_and_read() {
    let code = r#"
        mysync @arr = (1, 2, 3);
        push @arr, 4;
        push @arr, 5;
        join(",", @arr) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_array_visible_to_fn() {
    let code = r#"
        mysync @log;
        fn Demo::Ms::record($v) { push @log, $v }
        Demo::Ms::record("a");
        Demo::Ms::record("b");
        Demo::Ms::record("c");
        join(",", @log) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_hash_add_and_read() {
    let code = r#"
        mysync %h = (a => 1);
        $h{b} = 2;
        $h{c} = 3;
        join(",", map { "$_=$h{$_}" } sort keys %h) eq "a=1,b=2,c=3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_hash_visible_to_fn_via_assign() {
    // Note: in-place `$h{k}++` on a `mysync %h` from inside a fn
    // emits "VM compile error (unsupported): mysync hash element
    // update". The supported form is a plain assignment to a key —
    // pin that contract so a future fix lands without a silent
    // regression.
    let code = r#"
        mysync %seen;
        fn Demo::Ms::mark($k) { $seen{$k} = 1 }
        Demo::Ms::mark("alpha");
        Demo::Ms::mark("beta");
        (exists $seen{alpha} && exists $seen{beta} && !exists $seen{gamma}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_scalar_independent_of_other_my_decls() {
    let code = r#"
        mysync $a_sync = 100;
        my $b_local = 5;
        $a_sync += $b_local;
        $a_sync == 105 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_scalar_overwrite_assign() {
    let code = r#"
        mysync $x = 10;
        $x = 99;
        $x == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mysync_initial_value_is_set_at_declaration() {
    let code = r#"
        mysync $x = 42;
        $x
    "#;
    assert_eq!(eval_int(code), 42);
}
