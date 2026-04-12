//! Queue for Perl-style `DESTROY` callbacks when the last reference to a blessed object is dropped.
//!
//! [`crate::value::BlessedRef`]'s [`Drop`] enqueues `(class, inner payload)`; the
//! [`Interpreter`](crate::interpreter::Interpreter) drains the queue while running user code.
//!
//! **`NEEDS_VM_SYNC`:** `BlessedRef::drop` cannot call into the interpreter. The bytecode VM
//! therefore checks this flag **after each opcode** and runs [`Interpreter::drain_pending_destroys`]
//! so `DESTROY` runs before the next op (matching Perl’s synchronous destructor semantics). The
//! tree walker already drains once per statement.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::value::PerlValue;

static PENDING: Mutex<Vec<(String, PerlValue)>> = Mutex::new(Vec::new());

static NEEDS_VM_SYNC: AtomicBool = AtomicBool::new(false);

pub(crate) fn enqueue(class: String, payload: PerlValue) {
    let mut g = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    g.push((class, payload));
    NEEDS_VM_SYNC.store(true, Ordering::Release);
}

/// After a refcount drop may have enqueued `DESTROY`, the VM should drain before continuing.
#[inline]
pub(crate) fn pending_destroy_vm_sync_needed() -> bool {
    NEEDS_VM_SYNC.load(Ordering::Acquire)
}

pub(crate) fn take_queue() -> Vec<(String, PerlValue)> {
    let mut g = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let v = std::mem::take(&mut *g);
    // Mutex queue is empty after this take; recursive enqueues during drain will set the flag again.
    NEEDS_VM_SYNC.store(false, Ordering::Release);
    v
}
