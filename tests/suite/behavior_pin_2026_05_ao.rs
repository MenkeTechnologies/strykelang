//! Behavior-pinning batch AO (2026-05-06): test framework isolation.
//!
//! Pins the fixes for the `.stk` test framework bugs:
//!   - `test_run` no longer calls `std::process::exit(1)` — it sets a flag on
//!     the `VMHelper` instance so embedders can run failing tests without
//!     the host process being killed.
//!   - `test_pass`/`test_fail`/`test_skip` counters live on the `VMHelper`
//!     (per-instance `AtomicUsize`), not in `static` globals — counts no
//!     longer leak across runs in the same process.
//!   - The progress lines respect `interp.suppress_stdout`.

use crate::common::*;
use std::sync::atomic::Ordering;
use stryke::vm_helper::VMHelper;

#[test]
fn test_run_failure_sets_flag_does_not_exit_process() {
    // Failing assertions inside a `.stk` program must NOT call
    // `std::process::exit` — that would kill the test runner. They set
    // `interp.test_run_failed = true` instead. The `VMHelper::execute` call
    // returns normally; the embedder reads the flag.
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let code = r#"
        assert_eq 1, 2, "intentional fail";
        test_run();
    "#;
    let program = stryke::parse(code).expect("parse");
    let mut interp = VMHelper::new();
    interp.suppress_stdout = true;
    let _ = interp.execute(&program);
    assert!(
        interp.test_run_failed.load(Ordering::Relaxed),
        "test_run_failed flag must be set after a failing assertion"
    );
}

#[test]
fn test_run_success_leaves_flag_clear() {
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let code = r#"
        assert_eq 1, 1, "ok";
        test_run();
    "#;
    let program = stryke::parse(code).expect("parse");
    let mut interp = VMHelper::new();
    interp.suppress_stdout = true;
    let _ = interp.execute(&program);
    assert!(
        !interp.test_run_failed.load(Ordering::Relaxed),
        "passing assertions must leave the flag clear"
    );
}

#[test]
fn test_counters_are_per_instance_not_process_global() {
    // Run the same failing program in two separate VMHelpers. Each must see
    // exactly one failure — the counter from the first must NOT leak into the
    // second. Old bug: `static AtomicUsize` was shared across all runs in a
    // single process, so `b` would observe `a`'s counts.
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let code = r#"assert_eq 1, 2, "fail""#;
    let program = stryke::parse(code).expect("parse");

    let mut a = VMHelper::new();
    a.suppress_stdout = true;
    let _ = a.execute(&program);
    assert_eq!(a.test_fail_count.load(Ordering::Relaxed), 1);

    let mut b = VMHelper::new();
    b.suppress_stdout = true;
    // The fresh VMHelper starts at 0 — does NOT carry A's count over.
    assert_eq!(b.test_fail_count.load(Ordering::Relaxed), 0);
    let _ = b.execute(&program);
    assert_eq!(b.test_fail_count.load(Ordering::Relaxed), 1);
}
