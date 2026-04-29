//! AOP (`before` / `after` / `around` advice on user subs).
//! Mirrors zshrs's `intercept` builtin (zshrs/src/exec.rs:14656-14759).
//!
//! Bodies are lowered to bytecode at compile time and dispatched through the VM
//! (`run_block_region`) — not the tree-walker — so `our $x` and other compile-time
//! name resolutions work the same inside advice as outside it. The `our_*` tests
//! below are the behavioral proof; `tests/tree_walker_absent_aop.rs` is the
//! source-level proof that `dispatch_with_advice` never falls back to
//! `Interpreter::exec_block` for advice bodies.

use crate::common::*;

#[test]
fn before_advice_runs_before_sub() {
    let out = eval_int(
        r#"
        $count = 0;
        before "target" { $count = $count + 10 }
        sub target { $count = $count + 1; 42 }
        target();
        $count
    "#,
    );
    // Before runs first ($count=10), then sub runs ($count=11).
    assert_eq!(out, 11);
}

#[test]
fn before_does_not_change_return_value() {
    let out = eval_int(
        r#"
        before "target" { 999 }
        sub target { 42 }
        target()
    "#,
    );
    assert_eq!(out, 42);
}

#[test]
fn after_advice_runs_after_sub() {
    let out = eval_int(
        r#"
        $count = 0;
        after "target" { $count = $count + 100 }
        sub target { $count = $count + 1; 42 }
        target();
        $count
    "#,
    );
    // Sub runs first ($count=1), then after runs ($count=101).
    assert_eq!(out, 101);
}

#[test]
fn after_advice_sees_intercept_result() {
    let out = eval_int(
        r#"
        $captured = 0;
        after "target" { $captured = $INTERCEPT_RESULT }
        sub target { 7 }
        target();
        $captured
    "#,
    );
    assert_eq!(out, 7);
}

#[test]
fn after_advice_sees_timing() {
    // Check INTERCEPT_US is defined and non-negative.
    let out = eval_int(
        r#"
        $us = -1;
        after "target" { $us = $INTERCEPT_US }
        sub target { 0 }
        target();
        $us >= 0 ? 1 : 0
    "#,
    );
    assert_eq!(out, 1);
}

#[test]
fn intercept_name_visible_to_before() {
    let out = eval_string(
        r#"
        $seen = "";
        before "target" { $seen = $INTERCEPT_NAME }
        sub target { 0 }
        target();
        $seen
    "#,
    );
    assert_eq!(out, "target");
}

#[test]
fn intercept_args_visible_to_before() {
    // Use sum() rather than a foreach loop — the bytecode block lowering
    // (`Compiler::try_compile_block_region`) only accepts blocks whose final
    // statement is an expression, so a literal `for (…)` body trips the
    // "no-bytecode-fallback" rule. sum() is an expression and lowers cleanly.
    let out = eval_int(
        r#"
        $sum = 0;
        before "adder" { $sum = sum(@INTERCEPT_ARGS) }
        sub adder { 0 }
        adder(1, 2, 3, 4);
        $sum
    "#,
    );
    assert_eq!(out, 10);
}

#[test]
fn around_without_proceed_suppresses_original() {
    let out = eval_int(
        r#"
        $ran = 0;
        around "target" { 999 }
        sub target { $ran = 1; 42 }
        my $r = target();
        $ran
    "#,
    );
    assert_eq!(out, 0);
}

#[test]
fn around_without_proceed_returns_block_value() {
    // AspectJ-style: around's evaluated block value is the call's return. If `proceed()`
    // is not invoked, the original sub never runs and the block value replaces it.
    let out = eval_int(
        r#"
        around "target" { 999 }
        sub target { 42 }
        target()
    "#,
    );
    assert_eq!(out, 999);
}

#[test]
fn around_with_proceed_runs_original() {
    let out = eval_int(
        r#"
        $ran = 0;
        around "target" { my $r = proceed(); $r + 100 }
        sub target { $ran = 1; 42 }
        my $r = target();
        $ran + $r
    "#,
    );
    // ran=1, proceed returns 42, around returns 42+100=142, total = 1+142 = 143
    assert_eq!(out, 143);
}

#[test]
fn glob_star_matches_all() {
    let out = eval_int(
        r#"
        $count = 0;
        before "*" { $count = $count + 1 }
        sub foo { 0 }
        sub bar { 0 }
        foo();
        bar();
        $count
    "#,
    );
    assert_eq!(out, 2);
}

#[test]
fn glob_prefix_matches() {
    let out = eval_int(
        r#"
        $count = 0;
        before "log_*" { $count = $count + 1 }
        sub log_foo { 0 }
        sub log_bar { 0 }
        sub other { 0 }
        log_foo();
        log_bar();
        other();
        $count
    "#,
    );
    assert_eq!(out, 2);
}

#[test]
fn intercept_list_returns_registrations() {
    let out = eval_int(
        r#"
        before "foo" { 0 }
        after "bar*" { 0 }
        my @list = intercept_list();
        len(@list)
    "#,
    );
    assert_eq!(out, 2);
}

#[test]
fn intercept_remove_drops_advice() {
    let out = eval_int(
        r#"
        $count = 0;
        before "target" { $count = $count + 1 }
        sub target { 0 }
        target();
        my @list = intercept_list();
        my $id = $list[0]->[0];
        intercept_remove($id);
        target();
        $count
    "#,
    );
    assert_eq!(out, 1);
}

#[test]
fn intercept_clear_drops_all() {
    let out = eval_int(
        r#"
        $count = 0;
        before "target" { $count = $count + 1 }
        after "target" { $count = $count + 10 }
        sub target { 0 }
        target();
        intercept_clear();
        target();
        $count
    "#,
    );
    // First call: before(+1) + after(+10) = 11. Second call after clear: nothing.
    assert_eq!(out, 11);
}

#[test]
fn recursion_guard_self_call_in_before() {
    // Calling the same sub from inside its own before-advice must NOT recurse forever.
    let out = eval_int(
        r#"
        $count = 0;
        before "target" { $count = $count + 1; target() if $count < 5 }
        sub target { 0 }
        target();
        $count
    "#,
    );
    // Without re-entrancy guard this would infinite-loop. With guard, recursive
    // target() calls inside the advice run the original directly (no re-fire).
    assert_eq!(out, 1);
}

#[test]
fn multiple_before_advices_all_fire() {
    let out = eval_int(
        r#"
        $count = 0;
        before "target" { $count = $count + 1 }
        before "target" { $count = $count + 10 }
        before "target" { $count = $count + 100 }
        sub target { 0 }
        target();
        $count
    "#,
    );
    assert_eq!(out, 111);
}

#[test]
fn our_visible_in_before_body() {
    // Before-advice mutates an `our`-declared scalar; the toplevel read must see it.
    // Tree-walker execution would write to a different storage key (`count` vs
    // `main::count`); this test only passes when the body runs through bytecode.
    // The sub body is intentionally a no-op so this test isolates the BODY's name
    // resolution — the surrounding sub-call path is exercised by the bare-global
    // `before_advice_runs_before_sub` test above.
    let out = eval_int(
        r#"
        our $count = 0;
        before "target" { $count = $count + 10 }
        sub target { 42 }
        target();
        $count
    "#,
    );
    assert_eq!(out, 10);
}

#[test]
fn our_visible_in_after_body() {
    let out = eval_int(
        r#"
        our $captured = 0;
        after "target" { $captured = $INTERCEPT_RESULT }
        sub target { 7 }
        target();
        $captured
    "#,
    );
    assert_eq!(out, 7);
}

#[test]
fn our_visible_in_around_body_with_proceed() {
    let out = eval_int(
        r#"
        our $ran = 0;
        around "target" { $ran = 5; proceed() + 100 }
        sub target { 42 }
        my $r = target();
        $ran + $r
    "#,
    );
    // ran = 5 (set by around), r = proceed()+100 = 142, total = 147
    assert_eq!(out, 147);
}

#[test]
fn before_and_after_compose() {
    let out = eval_int(
        r#"
        $log = 0;
        before "target" { $log = $log * 10 + 1 }
        sub target { $log = $log * 10 + 2 }
        after "target" { $log = $log * 10 + 3 }
        target();
        $log
    "#,
    );
    // Order: before(1) → sub(2) → after(3) gives log = 0→1→12→123.
    assert_eq!(out, 123);
}
