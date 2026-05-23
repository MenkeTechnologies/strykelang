//! Pin `state $x` semantics: initialized once on first call, persists
//! across subsequent calls. Probed against the running interpreter
//! on 2026-05-23.
//!
//! Note: stryke currently supports `state` only for **scalars** —
//! `state %h` and `state @a` reset on each call. This file pins
//! the scalar form only; tests for aggregate state would need to
//! wait for that support to land.

use crate::common::*;

#[test]
fn state_scalar_initializes_once() {
    // Counter increments across calls; the `= 0` initializer runs
    // exactly once on the first invocation.
    let code = r#"
        fn Demo::SV::next {
            state $n = 0;
            $n++;
            $n
        }
        Demo::SV::next();
        Demo::SV::next();
        Demo::SV::next()
    "#;
    assert_eq!(eval_int(code), 3);
}

#[test]
fn state_scalar_persists_value_between_calls() {
    let code = r#"
        fn Demo::SV::add($x) {
            state $sum = 0;
            $sum += $x;
            $sum
        }
        Demo::SV::add(10);
        Demo::SV::add(20);
        Demo::SV::add(30)
    "#;
    assert_eq!(eval_int(code), 60);
}

#[test]
fn state_scalar_first_call_returns_initializer() {
    let code = r#"
        fn Demo::SV::first {
            state $x = 42;
            $x
        }
        Demo::SV::first()
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn state_scalar_initializer_evaluated_once_not_each_call() {
    // If the initializer ran every call, `$counter` would always
    // stay at 0. The fact that we return 3 proves the initializer
    // ran exactly once.
    let code = r#"
        fn Demo::SV::tick {
            state $counter = 0;
            ++$counter
        }
        Demo::SV::tick();
        Demo::SV::tick();
        Demo::SV::tick()
    "#;
    assert_eq!(eval_int(code), 3);
}

#[test]
fn state_scalar_assigned_after_init_survives_to_next_call() {
    let code = r#"
        fn Demo::SV::set_get($v) {
            state $stored;
            $stored = $v if defined $v;
            defined($stored) ? $stored : -1
        }
        Demo::SV::set_get(99);
        # Next call passes undef — value must persist.
        Demo::SV::set_get(undef)
    "#;
    assert_eq!(eval_int(code), 99);
}

#[test]
fn state_scalar_each_named_fn_has_its_own_storage() {
    // Two different functions both declare `state $x` — they must
    // not share storage.
    let code = r#"
        fn Demo::SV::a { state $x = 0; ++$x }
        fn Demo::SV::b { state $x = 0; ++$x }
        Demo::SV::a();
        Demo::SV::a();
        Demo::SV::a();   # a's counter is now 3
        Demo::SV::b();   # b's counter is independent, returns 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn state_scalar_memo_idempotent_first_value_pin() {
    // Classic memoization: `state $cache //= compute(...)` — only
    // the first call evaluates the RHS.
    let code = r#"
        fn Demo::SV::memo($v) {
            state $cache;
            $cache //= $v;
            $cache
        }
        Demo::SV::memo(10);
        Demo::SV::memo(20);
        Demo::SV::memo(30)
    "#;
    assert_eq!(eval_int(code), 10);
}

#[test]
fn state_scalar_in_loop_increments_per_iteration() {
    let code = r#"
        fn Demo::SV::bump { state $n = 0; ++$n }
        my $last = 0;
        for my $i (1:5) { $last = Demo::SV::bump() }
        $last
    "#;
    assert_eq!(eval_int(code), 5);
}

#[test]
fn state_scalar_persists_across_distant_call_sites() {
    let code = r#"
        fn Demo::SV::id { state $n = 0; ++$n }
        my $first  = Demo::SV::id();
        my $middle = Demo::SV::id();
        my $last   = Demo::SV::id();
        ($first == 1 && $middle == 2 && $last == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
