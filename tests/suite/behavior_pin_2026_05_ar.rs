//! Behavior-pinning batch AR (2026-05-06): closure parameter shadowing
//! across slot-vs-hash storage boundary.
//!
//! Pins a fix for a bug surfaced by the project's
//! `test_first_class_functions.stk`:
//!
//!   my $f = $double;     # outer slot-stored my $f
//!   fn pipe($g, $f) {    # param $f shadows outer
//!     fn { $f($g($x)) }  # inner closure body — should see param $f
//!   }
//!
//! `Scope::capture()` walks both slot-stored AND hash-stored scalars when
//! building a closure's environment. The outer `my $f` is slot-stored
//! (`DeclareScalarSlot`); the sub parameter `$f` is hash-stored (declared
//! via `apply_sub_signature` → `scope.declare_scalar`). Both got pushed
//! into the captured Vec under the same name. On `restore_capture`, both
//! were declared in the closure's call frame — slot 0 named "f" (outer)
//! AND `scalars["f"]` (param). `Frame::get_scalar` checks slots BEFORE
//! `scalars`, so the slot-stored OUTER value won every lookup, breaking
//! parameter shadowing.
//!
//! Fix: when capturing slot-stored scalars, skip names already captured
//! as hash-stored from an inner frame.

use crate::common::*;

#[test]
fn fn_param_shadows_outer_my_in_inner_closure() {
    // Direct repro of the project test failure: outer `my $f` reassigned
    // to one fn, parameter `$f` of `pipe_fns` then carries a different fn.
    // The inner closure must see the PARAM, not the outer my.
    let code = r#"
        my $f = fn ($x) { $x * $x };  # outer = square (irrelevant value)
        fn pipe_fns($g, $f) {
            fn ($x) { $f->($g->($x)) }
        }
        my $inc = fn ($x) { $x + 1 };
        my $dbl = fn ($x) { $x * 2 };
        my $h = pipe_fns($inc, $dbl);
        $h->(5)                        # should be dbl(inc(5)) = dbl(6) = 12
    "#;
    assert_eq!(eval_int(code), 12);
}

#[test]
fn fn_param_shadows_outer_my_minimal() {
    // Minimal repro: outer slot-stored $f, fn param $f, inner closure
    // returns $f. Should return the PARAM value, not the outer.
    let code = r#"
        my $f = "outer";
        fn make($f) { fn { $f } }
        my $cb = make("param");
        $cb->()
    "#;
    assert_eq!(eval_string(code), "param");
}

#[test]
fn fn_param_does_not_break_when_no_outer_collision() {
    // Sanity: the cross-storage dedup must not break the no-collision case.
    let code = r#"
        fn make($f) { fn { $f } }
        my $cb = make("param");
        $cb->()
    "#;
    assert_eq!(eval_string(code), "param");
}

#[test]
fn outer_my_visible_when_inner_closure_uses_different_name() {
    // The fix only suppresses slot-stored capture when the SAME name is
    // hash-stored at an inner frame. Different names must still capture
    // both — outer `my $a` should be visible in a closure that uses `$a`
    // when there's no parameter named `$a`.
    let code = r#"
        my $base = "OUTER";
        fn make($p) { fn { "$base/$p" } }
        my $cb = make("PARAM");
        $cb->()
    "#;
    assert_eq!(eval_string(code), "OUTER/PARAM");
}

#[test]
fn pipe_fns_compose_pattern_locks_in() {
    // The full failing pattern from project/t/test_first_class_functions.stk
    // — `pipe_fns` and `compose_fns` both rely on parameter shadowing
    // working through inner closures.
    let code = r#"
        fn compose_fns($f, $g) {
            fn ($x) { $f->($g->($x)) }
        }
        fn pipe_fns($g, $f) {
            fn ($x) { $f->($g->($x)) }
        }
        my $inc = fn ($x) { $x + 1 };
        my $dbl = fn ($x) { $x * 2 };

        my $inc_then_dbl = pipe_fns($inc, $dbl);
        my $dbl_then_inc = pipe_fns($dbl, $inc);
        my $composed    = compose_fns($dbl, $inc);

        $inc_then_dbl->(5) . "," . $dbl_then_inc->(5) . "," . $composed->(5)
    "#;
    assert_eq!(eval_string(code), "12,11,12");
}
