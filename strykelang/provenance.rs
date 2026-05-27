//! `provenance($val)` — value lineage as a first-class builtin.
//!
//! No existing scripting language (Perl, Python, Ruby, JavaScript, Lua, PHP)
//! ships automatic value-lineage tracking. Closest analogs are research
//! dataflow languages (LIO, Adapton) — none expose it as a user-callable
//! `provenance($x)` builtin. This module is the ledger that makes the
//! claim hold.
//!
//! Surface (see `builtins.rs` dispatch arms):
//!
//!   * `mark($val)`        — tag a value's heap Arc so subsequent ops touching
//!                            it accumulate a lineage record. Returns `$val`.
//!   * `provenance($val)`  — look up the lineage attached to `$val`'s Arc.
//!                            Returns a hashref `{ origin, ops => [...] }` or
//!                            `undef` if the value was never marked.
//!   * `unmark($val)`      — drop the ledger entry for `$val`'s Arc. Returns
//!                            `$val`.
//!
//! Hook point: `builtins::try_builtin` inspects its `args` BEFORE dispatch.
//! If any heap arg is in the ledger AND the call's result is a heap value,
//! `record_op` runs after dispatch returns — appending the call site, op
//! name, and a god-style arg summary to the result's ledger entry, and
//! propagating the marked status to the result so the lineage chains.
//!
//! Cost model (zero-cost when unused):
//!   * `LEDGER_ACTIVE` is an `AtomicBool` flipped to `true` on the first
//!     `mark(...)` call. Every dispatch path checks it via a relaxed load
//!     — single inlined branch when nobody has ever called `mark`.
//!   * When active, the hook does one `HashMap::contains_key` per heap arg
//!     (O(1)); only on a hit does it pay the `record_op` Mutex acquire.
//!
//! v1 limitations (intentional, documented in the LSP hover doc):
//!   * Tracks builtin-call op chains only. User-sub call boundaries are
//!     not recorded (v2 could hook the same way at sub entry).
//!   * `mark` keys on the heap Arc pointer, so two structurally-equal
//!     values with different origins have independent lineages. Two refs
//!     to the SAME Arc share lineage (the aliasing-visible model `god`
//!     already uses).
//!   * Immediates (integers, floats, undef) have no Arc → `provenance`
//!     on them always returns `undef`. Wrap in a hash/array if you need
//!     lineage tracking on a scalar number.
//!   * **String results from builtins** (`to_json`, `sha256`, `base64_*`,
//!     etc.) do NOT propagate. The VM's scalar-return path re-Arcs the
//!     string between dispatch return and assignment, so the post-hook
//!     stores under a now-stale pointer. Heap-container results — arrays,
//!     hashes, atomics, sets, deques, byte buffers — preserve Arc identity
//!     and propagate correctly. v2 would fix this with a deeper VM hook
//!     at the assignment boundary.
//!   * Ledger entries persist until `unmark($val)` OR a periodic sweep
//!     (currently: explicit `unmark` only; sweep is a v2 follow-up).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use indexmap::IndexMap;
// StrykeValue's array/hash refs are parking_lot RwLocks — use the same so
// `array_ref` / `hash_ref` constructors accept the produced `Arc<RwLock<…>>`.
use parking_lot::RwLock;

use crate::value::{HeapObject, StrykeValue};

/// Set to `true` on first `mark(...)` call. Every post-dispatch hook in
/// `try_builtin` checks this via `LEDGER_ACTIVE.load(Relaxed)` — when false
/// (the universal case for scripts that never opt into provenance) the
/// entire `record_op` path elides.
pub static LEDGER_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Each ledger entry pairs the lineage record with a `Weak<HeapObject>`
/// that points at the SAME heap object the entry's `usize` key refers to.
/// On lookup we upgrade the weak ref and verify the resulting `Arc`'s
/// pointer still equals the key — if not, the original object was dropped
/// and the address has been recycled by a fresh allocation, so the entry
/// is stale and must be discarded.
///
/// Without this check, a script (or test) can hit a false positive when
/// the allocator reuses an address. Pre-fix symptom: parallel tests
/// occasionally fail `provenance($unmarked)` returning a hashref from a
/// long-dropped sibling test. The weak-ref check is the GC mechanism the
/// v1 docstring deferred to "v2" — promoted to v1.1 because pointer reuse
/// is a real correctness issue, not just a test artifact.
struct LedgerEntry {
    weak: Weak<HeapObject>,
    node: ProvNode,
}

fn ledger() -> &'static Mutex<HashMap<usize, LedgerEntry>> {
    static LEDGER: OnceLock<Mutex<HashMap<usize, LedgerEntry>>> = OnceLock::new();
    LEDGER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Return a `Weak<HeapObject>` for a heap value. `None` for immediates
/// (no Arc exists to downgrade). Used by `mark` / `record_op` to capture
/// a non-owning reference we can later verify on lookup.
fn value_arc_weak(v: &StrykeValue) -> Option<Weak<HeapObject>> {
    v.with_heap(|_| ())?;
    let arc = v.heap_arc();
    Some(Arc::downgrade(&arc))
}

/// One operation in a value's lineage chain. Lines correspond to the
/// `line` field that `try_builtin` already threads through dispatch.
#[derive(Debug, Clone)]
pub struct ProvOp {
    pub op: String,
    pub args: Vec<String>,
    pub line: usize,
}

/// Lineage record for a single heap value.
#[derive(Debug, Clone)]
pub struct ProvNode {
    /// Site that called `mark($val)`. The string is a god-style short form
    /// of the value at mark time (e.g. `"INTEGER 42"`, `"HASH entries=3"`).
    pub origin: String,
    pub origin_line: usize,
    /// Append-only chain of operations that touched the value AFTER it was
    /// marked. Latest op last.
    pub ops: Vec<ProvOp>,
}

/// Stable heap pointer for a value, or `None` for immediates (integers,
/// floats, undef). Uses the value's underlying `Arc<HeapObject>` so every
/// heap variant — string, bytes, array, hash, atomic, generator, pipeline,
/// regex, blessed, code-ref, etc. — gets a single uniform key. Without the
/// universal pointer, we'd have to add a per-variant accessor for every new
/// `HeapObject` enum case in `value.rs` and silently miss any we forgot;
/// the heap-arc approach picks them up automatically.
pub fn value_arc_ptr(v: &StrykeValue) -> Option<usize> {
    // `with_heap` returns `None` for non-heap (immediate) values, so a single
    // gate covers the "this is not trackable" branch.
    v.with_heap(|_| ())?;
    let arc = v.heap_arc();
    Some(Arc::as_ptr(&arc) as usize)
}

/// Short, single-line summary of a value for use in a `ProvOp`'s args
/// vector. Keeps the ledger tiny — the full value is reachable via the
/// Arc, this is just a debug-readable handle.
pub fn short_summary(v: &StrykeValue) -> String {
    if v.is_undef() {
        return "undef".into();
    }
    if let Some(n) = v.as_integer() {
        return format!("{}", n);
    }
    if let Some(f) = v.as_float() {
        return format!("{}", f);
    }
    if let Some(b) = v.as_bytes_arc() {
        return format!("BYTES len={}", b.len());
    }
    if let Some(arc) = v.as_hash_ref() {
        // parking_lot RwLock guard — no Result wrapper, no need for unwrap.
        return format!("HASH entries={}", arc.read().len());
    }
    if let Some(arc) = v.as_array_ref() {
        return format!("ARRAY len={}", arc.read().len());
    }
    // Fall back to the canonical type tag.
    v.type_name()
}

/// Mark a value: register an entry in the ledger keyed by its heap Arc.
/// No-op for immediates (returns false; caller signals "not trackable" via
/// the builtin return value).
pub fn mark(v: &StrykeValue, line: usize) -> bool {
    let Some(ptr) = value_arc_ptr(v) else {
        return false;
    };
    let Some(weak) = value_arc_weak(v) else {
        return false;
    };
    let node = ProvNode {
        origin: short_summary(v),
        origin_line: line,
        ops: Vec::new(),
    };
    if let Ok(mut g) = ledger().lock() {
        g.insert(ptr, LedgerEntry { weak, node });
        LEDGER_ACTIVE.store(true, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// Return true if the value's Arc is currently in the ledger AND the
/// weak-ref check confirms the entry isn't stale (pointer reuse from a
/// dropped Arc). Stale entries are reaped during this check.
pub fn is_marked(v: &StrykeValue) -> bool {
    let Some(ptr) = value_arc_ptr(v) else {
        return false;
    };
    let Ok(mut g) = ledger().lock() else {
        return false;
    };
    let still_valid = match g.get(&ptr) {
        Some(entry) => match entry.weak.upgrade() {
            Some(arc) => Arc::as_ptr(&arc) as usize == ptr,
            None => false,
        },
        None => return false,
    };
    if !still_valid {
        g.remove(&ptr);
    }
    still_valid
}

/// Record a builtin-call op on the result's ledger entry. Caller must
/// have already verified at least one arg was marked. Propagates the
/// caller's accumulated ops chain so transitive lineage works:
///
///   $a = mark({ ... })            ← origin
///   $b = some_builtin($a)         ← record_op(b, "some_builtin", [a]) carries ops from a
///   $c = another_builtin($b)      ← record_op(c, ...) carries ops from b (which has a's origin)
pub fn record_op(result: &StrykeValue, op: &str, args: &[StrykeValue], line: usize) {
    let Some(result_ptr) = value_arc_ptr(result) else {
        return;
    };
    // Pick the lineage to inherit from: prefer the longest existing chain
    // among the marked args (so transitively-marked composition keeps the
    // richest history).
    let Ok(mut g) = ledger().lock() else {
        return;
    };
    // Reap any stale entries among the arg ptrs while scanning. `chosen`
    // is the longest-chain LIVE parent.
    let mut chosen: Option<ProvNode> = None;
    let mut stale_keys: Vec<usize> = Vec::new();
    for a in args {
        if let Some(p) = value_arc_ptr(a) {
            if let Some(entry) = g.get(&p) {
                let live = entry
                    .weak
                    .upgrade()
                    .map(|arc| Arc::as_ptr(&arc) as usize == p)
                    .unwrap_or(false);
                if !live {
                    stale_keys.push(p);
                    continue;
                }
                if chosen
                    .as_ref()
                    .map_or(true, |c| entry.node.ops.len() > c.ops.len())
                {
                    chosen = Some(entry.node.clone());
                }
            }
        }
    }
    for k in stale_keys {
        g.remove(&k);
    }
    let (origin, origin_line, mut ops) = match chosen {
        Some(c) => (c.origin, c.origin_line, c.ops),
        None => return, // no LIVE marked args
    };
    ops.push(ProvOp {
        op: op.to_string(),
        args: args.iter().map(short_summary).collect(),
        line,
    });
    let Some(result_weak) = value_arc_weak(result) else {
        return;
    };
    g.insert(
        result_ptr,
        LedgerEntry {
            weak: result_weak,
            node: ProvNode {
                origin,
                origin_line,
                ops,
            },
        },
    );
}

/// Look up a value's lineage. Returns a node clone for serialization to
/// a stryke-side hashref. Returns `None` if the entry is stale (the
/// original Arc was dropped and this `ptr` belongs to a new allocation
/// at the recycled address) — stale entries are reaped during lookup to
/// bound ledger growth without an explicit sweep.
pub fn lookup(v: &StrykeValue) -> Option<ProvNode> {
    let ptr = value_arc_ptr(v)?;
    let mut g = ledger().lock().ok()?;
    let entry = g.get(&ptr)?;
    match entry.weak.upgrade() {
        Some(arc) if Arc::as_ptr(&arc) as usize == ptr => Some(entry.node.clone()),
        _ => {
            // Stale: heap object gone OR address reused by a fresh
            // allocation. Reap and report missing.
            g.remove(&ptr);
            None
        }
    }
}

/// Drop a value's ledger entry. Idempotent.
pub fn unmark(v: &StrykeValue) -> bool {
    let Some(ptr) = value_arc_ptr(v) else {
        return false;
    };
    if let Ok(mut g) = ledger().lock() {
        g.remove(&ptr).is_some()
    } else {
        false
    }
}

/// Serialize a `ProvNode` to a stryke hashref:
///
///   {
///     origin       => "HASH entries=3",
///     origin_line  => 7,
///     ops          => [
///       { op => "sha256", args => ["BYTES len=8"], line => 12 },
///       ...
///     ],
///   }
pub fn node_to_value(node: &ProvNode) -> StrykeValue {
    let mut top: IndexMap<String, StrykeValue> = IndexMap::new();
    top.insert("origin".into(), StrykeValue::string(node.origin.clone()));
    top.insert(
        "origin_line".into(),
        StrykeValue::integer(node.origin_line as i64),
    );
    let mut ops_list: Vec<StrykeValue> = Vec::with_capacity(node.ops.len());
    for op in &node.ops {
        let mut entry: IndexMap<String, StrykeValue> = IndexMap::new();
        entry.insert("op".into(), StrykeValue::string(op.op.clone()));
        let args_arr: Vec<StrykeValue> = op
            .args
            .iter()
            .map(|s| StrykeValue::string(s.clone()))
            .collect();
        entry.insert(
            "args".into(),
            StrykeValue::array_ref(Arc::new(RwLock::new(args_arr))),
        );
        entry.insert("line".into(), StrykeValue::integer(op.line as i64));
        ops_list.push(StrykeValue::hash_ref(Arc::new(RwLock::new(entry))));
    }
    top.insert(
        "ops".into(),
        StrykeValue::array_ref(Arc::new(RwLock::new(ops_list))),
    );
    StrykeValue::hash_ref(Arc::new(RwLock::new(top)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_and_lookup_round_trip_on_hash() {
        let mut m: IndexMap<String, StrykeValue> = IndexMap::new();
        m.insert("k".into(), StrykeValue::integer(1));
        let h = StrykeValue::hash_ref(Arc::new(RwLock::new(m)));
        assert!(mark(&h, 42));
        let node = lookup(&h).expect("marked value must be in ledger");
        assert_eq!(node.origin_line, 42);
        assert!(node.origin.starts_with("HASH"), "origin = {}", node.origin);
        assert!(node.ops.is_empty(), "no ops yet at origin");
    }

    #[test]
    fn lookup_returns_none_for_immediate_integer() {
        let v = StrykeValue::integer(7);
        assert!(lookup(&v).is_none());
    }

    #[test]
    fn unmark_clears_the_entry() {
        let h = StrykeValue::hash_ref(Arc::new(RwLock::new(IndexMap::new())));
        assert!(mark(&h, 1));
        assert!(is_marked(&h));
        assert!(unmark(&h));
        assert!(!is_marked(&h));
        assert!(!unmark(&h), "second unmark is a no-op");
    }

    #[test]
    fn record_op_propagates_origin_through_chain() {
        let origin = StrykeValue::hash_ref(Arc::new(RwLock::new(IndexMap::new())));
        assert!(mark(&origin, 10));

        let intermediate = StrykeValue::array_ref(Arc::new(RwLock::new(Vec::new())));
        record_op(&intermediate, "to_array", &[origin.clone()], 11);
        let n1 = lookup(&intermediate).expect("intermediate inherited lineage");
        assert_eq!(n1.origin_line, 10, "origin_line must come from `origin`");
        assert_eq!(n1.ops.len(), 1);
        assert_eq!(n1.ops[0].op, "to_array");
        assert_eq!(n1.ops[0].line, 11);

        let final_v = StrykeValue::bytes(Arc::new(vec![1u8, 2, 3]));
        record_op(&final_v, "pack", &[intermediate.clone()], 12);
        let n2 = lookup(&final_v).expect("final inherited extended lineage");
        assert_eq!(
            n2.origin_line, 10,
            "origin still points at original mark site"
        );
        assert_eq!(n2.ops.len(), 2, "chain length is op count, not just last");
        assert_eq!(n2.ops[1].op, "pack");
    }

    /// 8 threads concurrently mark / lookup / record_op on a shared
    /// hash. No data races, no deadlocks, all threads observe the
    /// final lineage as a coherent state. Pins the `Mutex<HashMap>`
    /// ledger's thread-safety claim at the Rust level — stryke's
    /// own threading (spawn / pchannel) has its own quirks that
    /// would muddy a stryke-source version of this test.
    #[test]
    fn concurrent_mark_lookup_threads_observe_consistent_state() {
        use std::thread;

        let h = StrykeValue::hash_ref(Arc::new(RwLock::new(IndexMap::new())));
        assert!(mark(&h, 1));

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let h = h.clone();
                thread::spawn(move || {
                    // Mix of operations: lookup, record_op, is_marked.
                    // Loop a few times so we have actual contention on
                    // the ledger Mutex, not just one-shot calls.
                    for _ in 0..50 {
                        let node = lookup(&h).expect("origin must be live");
                        assert!(node.origin.starts_with("HASH"));
                        assert!(
                            is_marked(&h),
                            "thread {i}: is_marked must return true for the origin"
                        );
                        // Synthetic result value that we use to record an op —
                        // a fresh array per call (different Arc each loop).
                        let result = StrykeValue::array_ref(Arc::new(RwLock::new(Vec::new())));
                        record_op(&result, &format!("thread{i}_op"), &[h.clone()], i as usize);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("thread");
        }

        // Origin is still there + still keyed by the same Arc ptr.
        let final_node = lookup(&h).expect("origin still live after concurrency");
        assert_eq!(final_node.origin_line, 1);
    }

    #[test]
    fn record_op_no_op_when_no_args_marked() {
        let a = StrykeValue::hash_ref(Arc::new(RwLock::new(IndexMap::new())));
        let b = StrykeValue::hash_ref(Arc::new(RwLock::new(IndexMap::new())));
        // Neither marked.
        record_op(&b, "noop_demo", &[a.clone()], 1);
        assert!(
            lookup(&b).is_none(),
            "result must not gain a lineage from unmarked args"
        );
    }
}
