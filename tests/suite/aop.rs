//! AOP (`before` / `after` / `around` advice on user subs).
//! Mirrors zshrs's `intercept` builtin (zshrs/src/exec.rs:14656-14759).

use crate::common::*;

#[test]
fn before_advice_runs_before_sub() {
    let out = eval_int(
        r#"
        our $count = 0;
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
        our $count = 0;
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
fn after_advice_sees_timing() {
    // Just check INTERCEPT_US is defined and non-negative.
    let out = eval_int(
        r#"
        our $us = -1;
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
        our $seen = "";
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
    let out = eval_int(
        r#"
        our $sum = 0;
        before "adder" { for my $a (@INTERCEPT_ARGS) { $sum = $sum + $a } }
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
        our $ran = 0;
        around "target" { 999 }
        sub target { $ran = 1; 42 }
        my $r = target();
        $ran
    "#,
    );
    assert_eq!(out, 0);
}

#[test]
fn around_without_proceed_returns_undef() {
    let out = eval_int(
        r#"
        around "target" { 999 }
        sub target { 42 }
        defined(target()) ? 1 : 0
    "#,
    );
    assert_eq!(out, 0);
}

#[test]
fn around_with_proceed_runs_original() {
    let out = eval_int(
        r#"
        our $ran = 0;
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
        our $count = 0;
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
        our $count = 0;
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
        scalar(@list)
    "#,
    );
    assert_eq!(out, 2);
}

#[test]
fn intercept_remove_drops_advice() {
    let out = eval_int(
        r#"
        our $count = 0;
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
        our $count = 0;
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
        our $count = 0;
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
        our $count = 0;
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
fn before_and_after_compose() {
    let out = eval_int(
        r#"
        our $log = 0;
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
