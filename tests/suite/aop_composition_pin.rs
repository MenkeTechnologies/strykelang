//! AOP composition pins. The "AOP at the Call Site" design card in
//! `docs/report.html` claims zero-overhead-when-no-advice plus rich
//! composition; these pins lock the composition rules.

use crate::common::*;

// ── Before + after ordering ───────────────────────────────────────────

#[test]
fn before_runs_before_target_after_runs_after() {
    let code = r#"
        mysync $log = "";
        fn target { $log .= "T" }
        before "target" { $log .= "B" }
        after  "target" { $log .= "A" }
        target();
        $log eq "BTA" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn multiple_befores_run_in_registration_order() {
    let code = r#"
        mysync $log = "";
        fn target { $log .= "T" }
        before "target" { $log .= "1" }
        before "target" { $log .= "2" }
        before "target" { $log .= "3" }
        target();
        $log eq "123T" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn multiple_afters_run_in_registration_order() {
    let code = r#"
        mysync $log = "";
        fn target { $log .= "T" }
        after "target" { $log .= "A" }
        after "target" { $log .= "B" }
        after "target" { $log .= "C" }
        target();
        $log eq "TABC" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Around: proceed mechanics ─────────────────────────────────────────

#[test]
fn around_can_skip_original_by_not_calling_proceed() {
    let code = r#"
        mysync $called = 0;
        fn original { $called++; "real" }
        around "original" { "stub" }   # no proceed() → original skipped
        my $r = original();
        ($r eq "stub" && $called == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn around_with_proceed_returns_original_value() {
    let code = r#"
        fn original { 42 }
        around "original" { proceed() * 2 }
        original()
    "#;
    assert_eq!(eval_int(code), 84);
}

// ── Re-entrancy: calling target from inside its own advice ────────────

#[test]
fn calling_target_from_advice_body_bypasses_advice() {
    // Re-entrancy guard: the active intercept name is on a stack; if
    // the advice body calls the same name, the second call goes
    // straight to the original (no infinite recursion).
    let code = r#"
        mysync $calls = 0;
        fn original { $calls++; "x" }
        around "original" {
            original();   # re-entrant call: skips advice
            "wrapped"
        }
        my $r = original();
        ($r eq "wrapped" && $calls == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Glob-pattern intercepts ───────────────────────────────────────────

#[test]
fn glob_intercept_fires_on_multiple_matching_names() {
    let code = r#"
        mysync $count = 0;
        fn Foo::a { 1 }
        fn Foo::b { 2 }
        fn Foo::c { 3 }
        before "Foo::*" { $count++ }
        Foo::a();
        Foo::b();
        Foo::c();
        $count == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn glob_intercept_misses_non_matching_names() {
    let code = r#"
        mysync $count = 0;
        fn Foo::a { 1 }
        fn Bar::a { 2 }
        before "Foo::*" { $count++ }
        Foo::a();
        Bar::a();    # different namespace, advice should not fire
        $count == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn glob_intercept_captures_target_name_in_intercept_name() {
    let code = r#"
        mysync $seen = "";
        fn Lib::greet { "hi" }
        fn Lib::bye   { "bye" }
        before "Lib::*" { $seen .= $INTERCEPT_NAME . "," }
        Lib::greet();
        Lib::bye();
        $seen eq "Lib::greet,Lib::bye," ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── INTERCEPT_ARGS captures the arg list ──────────────────────────────

#[test]
fn intercept_args_captured_by_before() {
    let code = r#"
        mysync $sum = 0;
        fn adder { 0 }
        before "adder" { $sum = sum(@INTERCEPT_ARGS) }
        adder(1, 2, 3, 4);
        $sum
    "#;
    assert_eq!(eval_int(code), 10);
}

#[test]
fn intercept_args_visible_inside_around() {
    let code = r#"
        fn echo { "echo:" }
        around "echo" {
            my @a = @INTERCEPT_ARGS;
            "args=" . join(",", @a)
        }
        echo("x", "y", "z")
    "#;
    assert_eq!(eval_string(code), "args=x,y,z");
}

// ── intercept_list / clear ────────────────────────────────────────────

#[test]
fn intercept_list_count_grows_with_each_registration() {
    let code = r#"
        my $n0 = len(intercept_list());
        fn dummy { 1 }
        before "dummy" { 1 }
        after  "dummy" { 1 }
        around "dummy" { proceed() }
        my $n1 = len(intercept_list());
        ($n1 - $n0) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn intercept_clear_removes_all_advice() {
    // After clear, the original function should run unintercepted.
    let code = r#"
        mysync $log = "";
        fn target { $log .= "T" }
        before "target" { $log .= "B" }
        target();              # log: "BT"
        intercept_clear();
        target();              # log: "BTT"
        $log eq "BTT" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Stacking before + after + around — all three fire ────────────────

#[test]
fn before_around_after_all_fire_in_order() {
    let code = r#"
        mysync $log = "";
        fn target { $log .= "T" }
        before "target" { $log .= "B" }
        around "target" {
            $log .= "[";
            proceed();
            $log .= "]";
        }
        after  "target" { $log .= "A" }
        target();
        # before runs first, then around wraps the call, then after runs.
        $log eq "B[T]A" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
