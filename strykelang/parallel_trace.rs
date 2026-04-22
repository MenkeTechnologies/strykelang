//! Optional stderr tracing for `mysync` mutations under `trace { ... }` and `fan`.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::value::PerlValue;

static TRACE_ENABLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Fan / parallel worker label (`$_` index). `None` when not inside a labeled parallel block.
    static WORKER_INDEX: Cell<Option<i64>> = const { Cell::new(None) };
}

/// Begin a `trace { ... }` region (all threads see the same flag).
pub fn trace_enter() {
    TRACE_ENABLED.store(true, Ordering::SeqCst);
}

pub fn trace_leave() {
    TRACE_ENABLED.store(false, Ordering::SeqCst);
}

#[inline]
pub fn is_enabled() -> bool {
    TRACE_ENABLED.load(Ordering::SeqCst)
}

/// Set the current worker index for `fan [N] { }` (typically `$_` as integer).
pub fn fan_worker_set_index(i: Option<i64>) {
    WORKER_INDEX.with(|c| c.set(i));
}

/// Emit one line for a scalar mutation (mysync / atomic scalar).
pub fn emit_scalar_mutation(var: &str, old: &PerlValue, new: &PerlValue) {
    if !is_enabled() {
        return;
    }
    match WORKER_INDEX.with(|c| c.get()) {
        Some(i) => eprintln!("[thread {}] ${}: {} → {}", i, var, old, new),
        None => eprintln!("[main] ${}: {} → {}", var, old, new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_scalar_mutation_noops_when_trace_disabled() {
        fan_worker_set_index(Some(0));
        emit_scalar_mutation("x", &PerlValue::integer(1), &PerlValue::integer(2));
        fan_worker_set_index(None);
    }

    #[test]
    fn fan_worker_set_index_none_is_safe_after_some() {
        fan_worker_set_index(Some(42));
        fan_worker_set_index(None);
    }
}
