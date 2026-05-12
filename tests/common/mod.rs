//! Shared helpers for integration tests. `cargo test` runs `tests/integration.rs` as its own crate
//! that imports this module and `tests/suite/*`.

use parking_lot::RwLock;
use stryke::error::ErrorKind;
use stryke::value::StrykeValue;
use stryke::vm_helper::VMHelper;

/// Reader/writer lock around mutations of process-global flags
/// (`set_compat_mode`, `set_no_interop_mode`, `set_bigint_pragma`). All
/// `eval*` helpers acquire a *read* lock for the duration of parse +
/// execute, so they run concurrently with each other. Tests that need to
/// flip a global must acquire the *write* lock via [`with_global_flags`]
/// — that blocks all readers for the duration of the test, eliminating
/// the race where a flag-mutator's "set true → run → set false" window
/// poisons concurrently-running readers.
///
/// Without this guard, parallel `cargo test` runs flake on tests that
/// read `compat_mode()` (the dispatch matrix in `vm.rs`/`vm_helper.rs`/
/// `map_stream.rs`) when a parallel test happens to be inside its
/// flag-flip window.
pub static GLOBAL_FLAGS_LOCK: RwLock<()> = RwLock::new(());

/// Run `f` while holding the *write* lock on global runtime flags. Use
/// this in any test that calls `stryke::set_compat_mode`,
/// `stryke::set_no_interop_mode`, or `stryke::set_bigint_pragma` — the
/// write lock blocks all concurrent eval readers, so the flag flip and
/// any execution that depends on it run atomically with respect to
/// every other test. Inside `f`, use [`eval_locked`] / [`eval_int_locked`]
/// rather than the standard `eval`/`eval_int` — those would deadlock
/// trying to take the read lock the caller already holds for write.
pub fn with_global_flags<R>(f: impl FnOnce() -> R) -> R {
    let _guard = GLOBAL_FLAGS_LOCK.write();
    f()
}

/// Like [`eval`] but does NOT acquire the global-flags lock. Caller
/// must already hold the write lock via [`with_global_flags`].
pub fn eval_locked(code: &str) -> StrykeValue {
    let program = stryke::parse(code).expect("parse failed");
    let mut interp = VMHelper::new();
    interp.execute(&program).expect("execution failed")
}

/// `i64` variant of [`eval_locked`] for use inside [`with_global_flags`].
pub fn eval_int_locked(code: &str) -> i64 {
    eval_locked(code).to_int()
}

/// `ErrorKind` variant of [`eval_locked`] for use inside [`with_global_flags`].
pub fn eval_err_kind_locked(code: &str) -> ErrorKind {
    let program = stryke::parse(code).expect("parse failed");
    let mut interp = VMHelper::new();
    interp.execute(&program).unwrap_err().kind
}

/// Parse and execute Perl code; panics on parse or runtime error.
pub fn eval(code: &str) -> StrykeValue {
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let program = stryke::parse(code).expect("parse failed");
    let mut interp = VMHelper::new();
    interp.execute(&program).expect("execution failed")
}

pub fn eval_string(code: &str) -> String {
    eval(code).to_string()
}

pub fn eval_int(code: &str) -> i64 {
    eval(code).to_int()
}

pub fn eval_err_kind(code: &str) -> ErrorKind {
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let program = stryke::parse(code).expect("parse failed");
    let mut interp = VMHelper::new();
    interp.execute(&program).unwrap_err().kind
}

pub fn parse_err_kind(code: &str) -> ErrorKind {
    let _guard = GLOBAL_FLAGS_LOCK.read();
    stryke::parse(code).unwrap_err().kind
}
