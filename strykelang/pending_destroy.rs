//! Queue for Perl-style `DESTROY` callbacks when the last reference to a blessed object is dropped.
//!
//! [`crate::value::BlessedRef`]'s [`Drop`] enqueues `(class, inner payload)`; the
//! [`Interpreter`](crate::vm_helper::VMHelper) drains the queue while running user code.
//!
//! **`NEEDS_VM_SYNC`:** `BlessedRef::drop` cannot call into the interpreter. The bytecode VM
//! therefore checks this flag **after each opcode** and runs [`Interpreter::drain_pending_destroys`]
//! so `DESTROY` runs before the next op (matching Perl's synchronous destructor semantics). The
//! VM already drains once per statement.
//!
//! **Thread-local storage:** Each thread has its own queue to avoid cross-test interference when
//! Cargo runs tests in parallel. Each Perl interpreter is single-threaded anyway.

use std::cell::RefCell;

use crate::value::StrykeValue;

thread_local! {
    static PENDING: RefCell<Vec<(String, StrykeValue)>> = const { RefCell::new(Vec::new()) };
    static NEEDS_VM_SYNC: RefCell<bool> = const { RefCell::new(false) };
}

pub(crate) fn enqueue(class: String, payload: StrykeValue) {
    PENDING.with(|p| p.borrow_mut().push((class, payload)));
    NEEDS_VM_SYNC.with(|f| *f.borrow_mut() = true);
}

/// After a refcount drop may have enqueued `DESTROY`, the VM should drain before continuing.
#[inline]
pub(crate) fn pending_destroy_vm_sync_needed() -> bool {
    NEEDS_VM_SYNC.with(|f| *f.borrow())
}

pub(crate) fn take_queue() -> Vec<(String, StrykeValue)> {
    NEEDS_VM_SYNC.with(|f| *f.borrow_mut() = false);
    PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── enqueue / take_queue / pending_destroy_vm_sync_needed ───────────
    //
    // Tests live on a single thread; thread-local queue isolates them from
    // each other only insofar as serial execution is preserved. Each test
    // first drains the queue to guarantee a clean start (Cargo may have
    // re-used this thread from a prior parallel test).

    fn drain_clean() {
        let _ = take_queue();
    }

    #[test]
    fn initial_state_after_drain_is_clean() {
        drain_clean();
        assert!(!pending_destroy_vm_sync_needed());
        assert!(take_queue().is_empty());
    }

    #[test]
    fn enqueue_sets_sync_flag_and_pushes_payload() {
        drain_clean();
        enqueue("Foo".into(), StrykeValue::integer(7));
        assert!(pending_destroy_vm_sync_needed());
        let q = take_queue();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].0, "Foo");
        assert_eq!(q[0].1.to_int(), 7);
    }

    #[test]
    fn take_queue_clears_sync_flag() {
        drain_clean();
        enqueue("Bar".into(), StrykeValue::UNDEF);
        let _ = take_queue();
        // After draining, the flag must be reset.
        assert!(!pending_destroy_vm_sync_needed());
    }

    #[test]
    fn enqueue_preserves_fifo_order() {
        drain_clean();
        enqueue("A".into(), StrykeValue::integer(1));
        enqueue("B".into(), StrykeValue::integer(2));
        enqueue("C".into(), StrykeValue::integer(3));
        let q = take_queue();
        assert_eq!(q.len(), 3);
        assert_eq!(q[0].0, "A");
        assert_eq!(q[1].0, "B");
        assert_eq!(q[2].0, "C");
        assert_eq!(q[0].1.to_int(), 1);
        assert_eq!(q[2].1.to_int(), 3);
    }

    #[test]
    fn second_take_after_full_drain_returns_empty() {
        drain_clean();
        enqueue("X".into(), StrykeValue::integer(0));
        let first = take_queue();
        assert_eq!(first.len(), 1);
        // Once taken, the queue is empty; another take returns nothing.
        let second = take_queue();
        assert!(second.is_empty());
    }

    #[test]
    fn empty_queue_take_leaves_sync_flag_clear() {
        drain_clean();
        // Even with nothing enqueued, take_queue resets the flag.
        let _ = take_queue();
        assert!(!pending_destroy_vm_sync_needed());
    }

    #[test]
    fn sync_flag_stays_true_until_explicit_take() {
        drain_clean();
        enqueue("Persistent".into(), StrykeValue::UNDEF);
        // Reading the flag must not clear it (only take_queue clears).
        assert!(pending_destroy_vm_sync_needed());
        assert!(pending_destroy_vm_sync_needed());
        // Drain.
        let _ = take_queue();
    }

    #[test]
    fn payload_kind_preserved_through_queue() {
        drain_clean();
        enqueue("S".into(), StrykeValue::string("hello".into()));
        let q = take_queue();
        assert_eq!(q[0].1.to_string(), "hello");
    }
}
