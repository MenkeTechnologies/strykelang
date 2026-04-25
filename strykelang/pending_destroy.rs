//! Queue for Perl-style `DESTROY` callbacks when the last reference to a blessed object is dropped.
//!
//! [`crate::value::BlessedRef`]'s [`Drop`] enqueues `(class, inner payload)`; the
//! [`Interpreter`](crate::interpreter::Interpreter) drains the queue while running user code.
//!
//! **`NEEDS_VM_SYNC`:** `BlessedRef::drop` cannot call into the interpreter. The bytecode VM
//! therefore checks this flag **after each opcode** and runs [`Interpreter::drain_pending_destroys`]
//! so `DESTROY` runs before the next op (matching Perl's synchronous destructor semantics). The
//! VM already drains once per statement.
//!
//! **Thread-local storage:** Each thread has its own queue to avoid cross-test interference when
//! Cargo runs tests in parallel. Each Perl interpreter is single-threaded anyway.

use std::cell::RefCell;

use crate::value::PerlValue;

thread_local! {
    static PENDING: RefCell<Vec<(String, PerlValue)>> = const { RefCell::new(Vec::new()) };
    static NEEDS_VM_SYNC: RefCell<bool> = const { RefCell::new(false) };
}

pub(crate) fn enqueue(class: String, payload: PerlValue) {
    PENDING.with(|p| p.borrow_mut().push((class, payload)));
    NEEDS_VM_SYNC.with(|f| *f.borrow_mut() = true);
}

/// After a refcount drop may have enqueued `DESTROY`, the VM should drain before continuing.
#[inline]
pub(crate) fn pending_destroy_vm_sync_needed() -> bool {
    NEEDS_VM_SYNC.with(|f| *f.borrow())
}

pub(crate) fn take_queue() -> Vec<(String, PerlValue)> {
    NEEDS_VM_SYNC.with(|f| *f.borrow_mut() = false);
    PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()))
}
