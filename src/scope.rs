use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};

use crate::ast::PerlTypeName;
use crate::error::PerlError;
use crate::value::PerlValue;

/// Thread-safe shared array for `mysync @a`.
#[derive(Debug, Clone)]
pub struct AtomicArray(pub Arc<Mutex<Vec<PerlValue>>>);

/// Thread-safe shared hash for `mysync %h`.
#[derive(Debug, Clone)]
pub struct AtomicHash(pub Arc<Mutex<IndexMap<String, PerlValue>>>);

type ScopeCaptureWithAtomics = (
    Vec<(String, PerlValue)>,
    Vec<(String, AtomicArray)>,
    Vec<(String, AtomicHash)>,
);

/// Arrays installed by [`crate::interpreter::Interpreter::new`] on the outer frame. They must not be
/// copied into [`Scope::capture`] / [`Scope::restore_capture`] for closures, or the restored copy
/// would shadow the live handles (stale `@INC`, `%ENV`, topic `@_`, etc.).
#[inline]
fn capture_skip_bootstrap_array(name: &str) -> bool {
    matches!(
        name,
        "INC" | "ARGV" | "_" | "-" | "+" | "^CAPTURE" | "^CAPTURE_ALL"
    )
}

/// Hashes installed at interpreter bootstrap (same rationale as [`capture_skip_bootstrap_array`]).
#[inline]
fn capture_skip_bootstrap_hash(name: &str) -> bool {
    matches!(name, "INC" | "ENV" | "SIG" | "^HOOK")
}

/// Saved bindings for `local $x` / `local @a` / `local %h` — restored on [`Scope::pop_frame`].
#[derive(Clone, Debug)]
enum LocalRestore {
    Scalar(String, PerlValue),
    Array(String, Vec<PerlValue>),
    Hash(String, IndexMap<String, PerlValue>),
    /// `local $h{k}` — third is `None` if the key was absent before `local` (restore deletes the key).
    HashElement(String, String, Option<PerlValue>),
    /// `local $a[i]` — restore previous slot value (see [`Scope::local_set_array_element`]).
    ArrayElement(String, i64, PerlValue),
}

/// A single lexical scope frame.
/// Uses Vec instead of HashMap — for typical Perl code with < 10 variables per
/// scope, linear scan is faster than hashing due to cache locality and zero
/// hash overhead.
#[derive(Debug, Clone)]
struct Frame {
    scalars: Vec<(String, PerlValue)>,
    arrays: Vec<(String, Vec<PerlValue>)>,
    /// Subroutine (or bootstrap) `@_` — stored separately so call paths can move the arg
    /// [`Vec`] into the frame without an extra copy via [`Frame::arrays`].
    sub_underscore: Option<Vec<PerlValue>>,
    hashes: Vec<(String, IndexMap<String, PerlValue>)>,
    /// Slot-indexed scalars for O(1) access from compiled subroutines.
    /// Compiler assigns `my $x` declarations a u8 slot index; the VM accesses
    /// `scalar_slots[idx]` directly without name lookup or frame walking.
    scalar_slots: Vec<PerlValue>,
    /// Bare scalar name for each slot (same index as `scalar_slots`) — for [`Scope::capture`]
    /// / closures when the binding exists only in `scalar_slots`.
    scalar_slot_names: Vec<Option<String>>,
    /// Dynamic `local` saves — applied in reverse when this frame is popped.
    local_restores: Vec<LocalRestore>,
    /// Lexical names from `frozen my $x` / `@a` / `%h` (bare name, same as storage key).
    frozen_scalars: HashSet<String>,
    frozen_arrays: HashSet<String>,
    frozen_hashes: HashSet<String>,
    /// `typed my $x : Int` — runtime type checks on assignment.
    typed_scalars: HashMap<String, PerlTypeName>,
    /// Thread-safe arrays from `mysync @a`
    atomic_arrays: Vec<(String, AtomicArray)>,
    /// Thread-safe hashes from `mysync %h`
    atomic_hashes: Vec<(String, AtomicHash)>,
    /// `defer { BLOCK }` closures to run when this frame is popped (LIFO order).
    defers: Vec<PerlValue>,
}

impl Frame {
    /// Drop all lexical bindings so blessed objects run `DESTROY` when frames are recycled
    /// ([`Scope::pop_frame`]) or reused ([`Scope::push_frame`]).
    #[inline]
    fn clear_all_bindings(&mut self) {
        self.scalars.clear();
        self.arrays.clear();
        self.sub_underscore = None;
        self.hashes.clear();
        self.scalar_slots.clear();
        self.scalar_slot_names.clear();
        self.local_restores.clear();
        self.frozen_scalars.clear();
        self.frozen_arrays.clear();
        self.frozen_hashes.clear();
        self.typed_scalars.clear();
        self.atomic_arrays.clear();
        self.defers.clear();
        self.atomic_hashes.clear();
    }

    /// True if this slot index is a real binding (not vec padding before a higher-index declare).
    /// Anonymous temps use [`Option::Some`] with an empty string so slot ops do not fall through
    /// to an outer frame's same slot index.
    #[inline]
    fn owns_scalar_slot_index(&self, idx: usize) -> bool {
        self.scalar_slot_names.get(idx).is_some_and(|n| n.is_some())
    }

    #[inline]
    fn new() -> Self {
        Self {
            scalars: Vec::new(),
            arrays: Vec::new(),
            sub_underscore: None,
            hashes: Vec::new(),
            scalar_slots: Vec::new(),
            scalar_slot_names: Vec::new(),
            frozen_scalars: HashSet::new(),
            frozen_arrays: HashSet::new(),
            frozen_hashes: HashSet::new(),
            typed_scalars: HashMap::new(),
            atomic_arrays: Vec::new(),
            atomic_hashes: Vec::new(),
            local_restores: Vec::new(),
            defers: Vec::new(),
        }
    }

    #[inline]
    fn get_scalar(&self, name: &str) -> Option<&PerlValue> {
        if let Some(v) = self.get_scalar_from_slot(name) {
            return Some(v);
        }
        self.scalars.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    /// O(N) scan over slot names — only used by `get_scalar` fallback (name-based lookup);
    /// hot compiled paths use `get_scalar_slot(idx)` directly.
    #[inline]
    fn get_scalar_from_slot(&self, name: &str) -> Option<&PerlValue> {
        for (i, sn) in self.scalar_slot_names.iter().enumerate() {
            if let Some(ref n) = sn {
                if n == name {
                    return self.scalar_slots.get(i);
                }
            }
        }
        None
    }

    #[inline]
    fn has_scalar(&self, name: &str) -> bool {
        if self
            .scalar_slot_names
            .iter()
            .any(|sn| sn.as_deref() == Some(name))
        {
            return true;
        }
        self.scalars.iter().any(|(k, _)| k == name)
    }

    #[inline]
    fn set_scalar(&mut self, name: &str, val: PerlValue) {
        for (i, sn) in self.scalar_slot_names.iter().enumerate() {
            if let Some(ref n) = sn {
                if n == name {
                    if i < self.scalar_slots.len() {
                        self.scalar_slots[i] = val;
                    }
                    return;
                }
            }
        }
        if let Some(entry) = self.scalars.iter_mut().find(|(k, _)| k == name) {
            entry.1 = val;
        } else {
            self.scalars.push((name.to_string(), val));
        }
    }

    #[inline]
    fn get_array(&self, name: &str) -> Option<&Vec<PerlValue>> {
        if name == "_" {
            if let Some(ref v) = self.sub_underscore {
                return Some(v);
            }
        }
        self.arrays.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    #[inline]
    fn has_array(&self, name: &str) -> bool {
        if name == "_" && self.sub_underscore.is_some() {
            return true;
        }
        self.arrays.iter().any(|(k, _)| k == name)
    }

    #[inline]
    fn get_array_mut(&mut self, name: &str) -> Option<&mut Vec<PerlValue>> {
        if name == "_" {
            return self.sub_underscore.as_mut();
        }
        self.arrays
            .iter_mut()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }

    #[inline]
    fn set_array(&mut self, name: &str, val: Vec<PerlValue>) {
        if name == "_" {
            if let Some(pos) = self.arrays.iter().position(|(k, _)| k == name) {
                self.arrays.swap_remove(pos);
            }
            self.sub_underscore = Some(val);
            return;
        }
        if let Some(entry) = self.arrays.iter_mut().find(|(k, _)| k == name) {
            entry.1 = val;
        } else {
            self.arrays.push((name.to_string(), val));
        }
    }

    #[inline]
    fn get_hash(&self, name: &str) -> Option<&IndexMap<String, PerlValue>> {
        self.hashes.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    #[inline]
    fn has_hash(&self, name: &str) -> bool {
        self.hashes.iter().any(|(k, _)| k == name)
    }

    #[inline]
    fn get_hash_mut(&mut self, name: &str) -> Option<&mut IndexMap<String, PerlValue>> {
        self.hashes
            .iter_mut()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }

    #[inline]
    fn set_hash(&mut self, name: &str, val: IndexMap<String, PerlValue>) {
        if let Some(entry) = self.hashes.iter_mut().find(|(k, _)| k == name) {
            entry.1 = val;
        } else {
            self.hashes.push((name.to_string(), val));
        }
    }
}

/// Manages lexical scoping with a stack of frames.
/// Innermost frame is last in the vector.
#[derive(Debug, Clone)]
pub struct Scope {
    frames: Vec<Frame>,
    /// Recycled frames to avoid allocation on every push_frame/pop_frame cycle.
    frame_pool: Vec<Frame>,
    /// When true (rayon worker / parallel block), reject writes to outer captured lexicals unless
    /// the binding is `mysync` (atomic) or a loop topic (`$_`, `$a`, `$b`). Package names with `::`
    /// are exempt. Requires at least two frames (captured + block locals); use [`Self::push_frame`]
    /// before running a block body on a worker.
    parallel_guard: bool,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

impl Scope {
    pub fn new() -> Self {
        let mut s = Self {
            frames: Vec::with_capacity(32),
            frame_pool: Vec::with_capacity(32),
            parallel_guard: false,
        };
        s.frames.push(Frame::new());
        s
    }

    /// Enable [`Self::parallel_guard`] for parallel worker interpreters (pmap, fan, …).
    #[inline]
    pub fn set_parallel_guard(&mut self, enabled: bool) {
        self.parallel_guard = enabled;
    }

    #[inline]
    pub fn parallel_guard(&self) -> bool {
        self.parallel_guard
    }

    #[inline]
    fn parallel_skip_special_name(name: &str) -> bool {
        name.contains("::")
    }

    /// Loop/sort topic scalars that parallel ops assign before each iteration.
    #[inline]
    fn parallel_allowed_topic_scalar(name: &str) -> bool {
        matches!(name, "_" | "a" | "b")
    }

    /// Regex / runtime scratch arrays live on an outer frame; parallel match still mutates them.
    #[inline]
    fn parallel_allowed_internal_array(name: &str) -> bool {
        matches!(name, "-" | "+" | "^CAPTURE" | "^CAPTURE_ALL")
    }

    /// `%ENV`, `%INC`, and regex named-capture hashes `"+"` / `"-"` — same outer-frame issue as internal arrays.
    #[inline]
    fn parallel_allowed_internal_hash(name: &str) -> bool {
        matches!(name, "+" | "-" | "ENV" | "INC")
    }

    fn check_parallel_scalar_write(&self, name: &str) -> Result<(), PerlError> {
        if !self.parallel_guard || Self::parallel_skip_special_name(name) {
            return Ok(());
        }
        if Self::parallel_allowed_topic_scalar(name) {
            return Ok(());
        }
        if crate::special_vars::is_regex_match_scalar_name(name) {
            return Ok(());
        }
        let inner = self.frames.len().saturating_sub(1);
        for (i, frame) in self.frames.iter().enumerate().rev() {
            if frame.has_scalar(name) {
                if let Some(v) = frame.get_scalar(name) {
                    if v.as_atomic_arc().is_some() {
                        return Ok(());
                    }
                }
                if i != inner {
                    return Err(PerlError::runtime(
                        format!(
                            "cannot assign to captured non-mysync variable `${}` in a parallel block",
                            name
                        ),
                        0,
                    ));
                }
                return Ok(());
            }
        }
        Err(PerlError::runtime(
            format!(
                "cannot assign to undeclared variable `${}` in a parallel block",
                name
            ),
            0,
        ))
    }

    #[inline]
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Pop frames until we're at `target_depth`. Used by VM ReturnValue
    /// to cleanly unwind through if/while/for blocks on return.
    #[inline]
    pub fn pop_to_depth(&mut self, target_depth: usize) {
        while self.frames.len() > target_depth && self.frames.len() > 1 {
            self.pop_frame();
        }
    }

    #[inline]
    pub fn push_frame(&mut self) {
        if let Some(mut frame) = self.frame_pool.pop() {
            frame.clear_all_bindings();
            self.frames.push(frame);
        } else {
            self.frames.push(Frame::new());
        }
    }

    // ── Frame-local scalar slots (O(1) access for compiled subs) ──

    /// Read scalar from slot — innermost binding for `slot` wins (same index can exist on nested
    /// frames; padding entries without [`Frame::owns_scalar_slot_index`] do not shadow outers).
    #[inline]
    pub fn get_scalar_slot(&self, slot: u8) -> PerlValue {
        let idx = slot as usize;
        for frame in self.frames.iter().rev() {
            if idx < frame.scalar_slots.len() && frame.owns_scalar_slot_index(idx) {
                return frame.scalar_slots[idx].clone();
            }
        }
        PerlValue::UNDEF
    }

    /// Write scalar to slot — innermost binding for `slot` wins (see [`Self::get_scalar_slot`]).
    #[inline]
    pub fn set_scalar_slot(&mut self, slot: u8, val: PerlValue) {
        let idx = slot as usize;
        let len = self.frames.len();
        for i in (0..len).rev() {
            if idx < self.frames[i].scalar_slots.len() && self.frames[i].owns_scalar_slot_index(idx)
            {
                self.frames[i].scalar_slots[idx] = val;
                return;
            }
        }
        let top = self.frames.last_mut().unwrap();
        top.scalar_slots.resize(idx + 1, PerlValue::UNDEF);
        if idx >= top.scalar_slot_names.len() {
            top.scalar_slot_names.resize(idx + 1, None);
        }
        top.scalar_slot_names[idx] = Some(String::new());
        top.scalar_slots[idx] = val;
    }

    /// Like [`set_scalar_slot`] but respects the parallel guard — returns `Err` when assigning
    /// to a slot that belongs to an outer frame inside a parallel block.  `slot_name` is resolved
    /// from the bytecode's name table by the caller when available.
    #[inline]
    pub fn set_scalar_slot_checked(
        &mut self,
        slot: u8,
        val: PerlValue,
        slot_name: Option<&str>,
    ) -> Result<(), PerlError> {
        if self.parallel_guard {
            let idx = slot as usize;
            let len = self.frames.len();
            let top_has = idx < self.frames[len - 1].scalar_slots.len()
                && self.frames[len - 1].owns_scalar_slot_index(idx);
            if !top_has {
                let name_owned: String = {
                    let mut found = String::new();
                    for i in (0..len).rev() {
                        if let Some(Some(n)) = self.frames[i].scalar_slot_names.get(idx) {
                            found = n.clone();
                            break;
                        }
                    }
                    if found.is_empty() {
                        if let Some(sn) = slot_name {
                            found = sn.to_string();
                        }
                    }
                    found
                };
                let name = name_owned.as_str();
                if !name.is_empty() && !Self::parallel_allowed_topic_scalar(name) {
                    let inner = len.saturating_sub(1);
                    for (fi, frame) in self.frames.iter().enumerate().rev() {
                        if frame.has_scalar(name)
                            || (idx < frame.scalar_slots.len() && frame.owns_scalar_slot_index(idx))
                        {
                            if fi != inner {
                                return Err(PerlError::runtime(
                                    format!(
                                        "cannot assign to captured outer lexical `${}` inside a parallel block (use `mysync`)",
                                        name
                                    ),
                                    0,
                                ));
                            }
                            break;
                        }
                    }
                }
            }
        }
        self.set_scalar_slot(slot, val);
        Ok(())
    }

    /// Declare + initialize scalar in the current frame's slot array.
    /// `name` (bare identifier, e.g. `x` for `$x`) is stored for [`Scope::capture`] when the
    /// binding is slot-only (no duplicate `frame.scalars` row).
    #[inline]
    pub fn declare_scalar_slot(&mut self, slot: u8, val: PerlValue, name: Option<&str>) {
        let idx = slot as usize;
        let frame = self.frames.last_mut().unwrap();
        if idx >= frame.scalar_slots.len() {
            frame.scalar_slots.resize(idx + 1, PerlValue::UNDEF);
        }
        frame.scalar_slots[idx] = val;
        if idx >= frame.scalar_slot_names.len() {
            frame.scalar_slot_names.resize(idx + 1, None);
        }
        match name {
            Some(n) => frame.scalar_slot_names[idx] = Some(n.to_string()),
            // Anonymous slot: mark occupied so padding holes don't shadow parent frame slots.
            None => frame.scalar_slot_names[idx] = Some(String::new()),
        }
    }

    /// Slot-indexed `.=` — avoids frame walking and string comparison on every iteration.
    ///
    /// Returns a [`PerlValue::shallow_clone`] (Arc::clone) of the stored value
    /// rather than a full [`Clone`], which would deep-copy the entire `String`
    /// payload and turn a `$s .= "x"` loop into O(N²) memcpy.
    /// Repeated `$slot .= rhs` fused-loop fast path: locates the slot's frame once,
    /// tries `try_concat_repeat_inplace` (unique heap-String → single `reserve`+`push_str`
    /// burst), and returns `true` on success. Returns `false` when the slot is not a
    /// uniquely-held `String` so the caller can fall back to the per-iteration slow
    /// path. Called from `Op::ConcatConstSlotLoop`.
    #[inline]
    pub fn scalar_slot_concat_repeat_inplace(&mut self, slot: u8, rhs: &str, n: usize) -> bool {
        let idx = slot as usize;
        let len = self.frames.len();
        let fi = {
            let mut found = len - 1;
            if idx >= self.frames[found].scalar_slots.len()
                || !self.frames[found].owns_scalar_slot_index(idx)
            {
                for i in (0..len - 1).rev() {
                    if idx < self.frames[i].scalar_slots.len()
                        && self.frames[i].owns_scalar_slot_index(idx)
                    {
                        found = i;
                        break;
                    }
                }
            }
            found
        };
        let frame = &mut self.frames[fi];
        if idx >= frame.scalar_slots.len() {
            frame.scalar_slots.resize(idx + 1, PerlValue::UNDEF);
        }
        frame.scalar_slots[idx].try_concat_repeat_inplace(rhs, n)
    }

    /// Slow fallback for the fused string-append loop: clones the RHS into a new
    /// `PerlValue::string` once and runs the existing `scalar_slot_concat_inplace`
    /// path `n` times. Used by `Op::ConcatConstSlotLoop` when the slot is aliased
    /// and the in-place fast path rejected the mutation.
    #[inline]
    pub fn scalar_slot_concat_repeat_slow(&mut self, slot: u8, rhs: &str, n: usize) {
        let pv = PerlValue::string(rhs.to_owned());
        for _ in 0..n {
            let _ = self.scalar_slot_concat_inplace(slot, &pv);
        }
    }

    #[inline]
    pub fn scalar_slot_concat_inplace(&mut self, slot: u8, rhs: &PerlValue) -> PerlValue {
        let idx = slot as usize;
        let len = self.frames.len();
        let fi = {
            let mut found = len - 1;
            if idx >= self.frames[found].scalar_slots.len()
                || !self.frames[found].owns_scalar_slot_index(idx)
            {
                for i in (0..len - 1).rev() {
                    if idx < self.frames[i].scalar_slots.len()
                        && self.frames[i].owns_scalar_slot_index(idx)
                    {
                        found = i;
                        break;
                    }
                }
            }
            found
        };
        let frame = &mut self.frames[fi];
        if idx >= frame.scalar_slots.len() {
            frame.scalar_slots.resize(idx + 1, PerlValue::UNDEF);
        }
        // Fast path: when the slot holds the only `Arc<HeapObject::String>` handle,
        // extend the underlying `String` buffer in place — no Arc alloc, no full
        // unwrap/rewrap. This turns a `$s .= "x"` loop into `String::push_str` only.
        // The shallow_clone handle that goes back onto the VM stack briefly bumps
        // the refcount to 2, so the NEXT iteration's fast path would fail — except
        // the VM immediately `Pop`s that handle (or `ConcatAppendSlotVoid` never
        // pushes it), restoring unique ownership before the next `.=`.
        if frame.scalar_slots[idx].try_concat_append_inplace(rhs) {
            return frame.scalar_slots[idx].shallow_clone();
        }
        let new_val = std::mem::replace(&mut frame.scalar_slots[idx], PerlValue::UNDEF)
            .concat_append_owned(rhs);
        let handle = new_val.shallow_clone();
        frame.scalar_slots[idx] = new_val;
        handle
    }

    #[inline]
    pub(crate) fn can_pop_frame(&self) -> bool {
        self.frames.len() > 1
    }

    #[inline]
    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            let mut frame = self.frames.pop().expect("pop_frame");
            // Local restore must write outer bindings even when parallel_guard is on
            // (user code cannot mutate captured vars; unwind is not user mutation).
            let saved_guard = self.parallel_guard;
            self.parallel_guard = false;
            for entry in frame.local_restores.drain(..).rev() {
                match entry {
                    LocalRestore::Scalar(name, old) => {
                        let _ = self.set_scalar(&name, old);
                    }
                    LocalRestore::Array(name, old) => {
                        let _ = self.set_array(&name, old);
                    }
                    LocalRestore::Hash(name, old) => {
                        let _ = self.set_hash(&name, old);
                    }
                    LocalRestore::HashElement(name, key, old) => match old {
                        Some(v) => {
                            let _ = self.set_hash_element(&name, &key, v);
                        }
                        None => {
                            let _ = self.delete_hash_element(&name, &key);
                        }
                    },
                    LocalRestore::ArrayElement(name, index, old) => {
                        let _ = self.set_array_element(&name, index, old);
                    }
                }
            }
            self.parallel_guard = saved_guard;
            frame.clear_all_bindings();
            // Return frame to pool for reuse (avoids allocation on next push_frame).
            if self.frame_pool.len() < 64 {
                self.frame_pool.push(frame);
            }
        }
    }

    /// `local $name` — save current value, assign `val`; restore on `pop_frame`.
    pub fn local_set_scalar(&mut self, name: &str, val: PerlValue) -> Result<(), PerlError> {
        let old = self.get_scalar(name);
        if let Some(frame) = self.frames.last_mut() {
            frame
                .local_restores
                .push(LocalRestore::Scalar(name.to_string(), old));
        }
        self.set_scalar(name, val)
    }

    /// `local @name` — not valid for `mysync` arrays.
    pub fn local_set_array(&mut self, name: &str, val: Vec<PerlValue>) -> Result<(), PerlError> {
        if self.find_atomic_array(name).is_some() {
            return Err(PerlError::runtime(
                "local cannot be used on mysync arrays",
                0,
            ));
        }
        let old = self.get_array(name);
        if let Some(frame) = self.frames.last_mut() {
            frame
                .local_restores
                .push(LocalRestore::Array(name.to_string(), old));
        }
        self.set_array(name, val)?;
        Ok(())
    }

    /// `local %name`
    pub fn local_set_hash(
        &mut self,
        name: &str,
        val: IndexMap<String, PerlValue>,
    ) -> Result<(), PerlError> {
        if self.find_atomic_hash(name).is_some() {
            return Err(PerlError::runtime(
                "local cannot be used on mysync hashes",
                0,
            ));
        }
        let old = self.get_hash(name);
        if let Some(frame) = self.frames.last_mut() {
            frame
                .local_restores
                .push(LocalRestore::Hash(name.to_string(), old));
        }
        self.set_hash(name, val)?;
        Ok(())
    }

    /// `local $h{key} = val` — save key state; restore one slot on `pop_frame`.
    pub fn local_set_hash_element(
        &mut self,
        name: &str,
        key: &str,
        val: PerlValue,
    ) -> Result<(), PerlError> {
        if self.find_atomic_hash(name).is_some() {
            return Err(PerlError::runtime(
                "local cannot be used on mysync hash elements",
                0,
            ));
        }
        let old = if self.exists_hash_element(name, key) {
            Some(self.get_hash_element(name, key))
        } else {
            None
        };
        if let Some(frame) = self.frames.last_mut() {
            frame.local_restores.push(LocalRestore::HashElement(
                name.to_string(),
                key.to_string(),
                old,
            ));
        }
        self.set_hash_element(name, key, val)?;
        Ok(())
    }

    /// `local $a[i] = val` — save element (as returned by [`Self::get_array_element`]), assign;
    /// restore on [`Self::pop_frame`].
    pub fn local_set_array_element(
        &mut self,
        name: &str,
        index: i64,
        val: PerlValue,
    ) -> Result<(), PerlError> {
        if self.find_atomic_array(name).is_some() {
            return Err(PerlError::runtime(
                "local cannot be used on mysync array elements",
                0,
            ));
        }
        let old = self.get_array_element(name, index);
        if let Some(frame) = self.frames.last_mut() {
            frame
                .local_restores
                .push(LocalRestore::ArrayElement(name.to_string(), index, old));
        }
        self.set_array_element(name, index, val)?;
        Ok(())
    }

    // ── Scalars ──

    #[inline]
    pub fn declare_scalar(&mut self, name: &str, val: PerlValue) {
        let _ = self.declare_scalar_frozen(name, val, false, None);
    }

    /// Declare a lexical scalar; `frozen` means no further assignment to this binding.
    /// `ty` is from `typed my $x : Int` — enforced on every assignment.
    pub fn declare_scalar_frozen(
        &mut self,
        name: &str,
        val: PerlValue,
        frozen: bool,
        ty: Option<PerlTypeName>,
    ) -> Result<(), PerlError> {
        if let Some(ref t) = ty {
            t.check_value(&val)
                .map_err(|msg| PerlError::type_error(format!("`${}`: {}", name, msg), 0))?;
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.set_scalar(name, val);
            if frozen {
                frame.frozen_scalars.insert(name.to_string());
            }
            if let Some(t) = ty {
                frame.typed_scalars.insert(name.to_string(), t);
            }
        }
        Ok(())
    }

    /// True if the innermost lexical scalar binding for `name` is `frozen`.
    pub fn is_scalar_frozen(&self, name: &str) -> bool {
        for frame in self.frames.iter().rev() {
            if frame.has_scalar(name) {
                return frame.frozen_scalars.contains(name);
            }
        }
        false
    }

    /// True if the innermost lexical array binding for `name` is `frozen`.
    pub fn is_array_frozen(&self, name: &str) -> bool {
        for frame in self.frames.iter().rev() {
            if frame.has_array(name) {
                return frame.frozen_arrays.contains(name);
            }
        }
        false
    }

    /// True if the innermost lexical hash binding for `name` is `frozen`.
    pub fn is_hash_frozen(&self, name: &str) -> bool {
        for frame in self.frames.iter().rev() {
            if frame.has_hash(name) {
                return frame.frozen_hashes.contains(name);
            }
        }
        false
    }

    /// Returns Some(sigil) if the named variable is frozen, None if mutable.
    pub fn check_frozen(&self, sigil: &str, name: &str) -> Option<&'static str> {
        match sigil {
            "$" => {
                if self.is_scalar_frozen(name) {
                    Some("scalar")
                } else {
                    None
                }
            }
            "@" => {
                if self.is_array_frozen(name) {
                    Some("array")
                } else {
                    None
                }
            }
            "%" => {
                if self.is_hash_frozen(name) {
                    Some("hash")
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    #[inline]
    pub fn get_scalar(&self, name: &str) -> PerlValue {
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.get_scalar(name) {
                // Transparently unwrap Atomic — read through the lock
                if let Some(arc) = val.as_atomic_arc() {
                    return arc.lock().clone();
                }
                // Transparently unwrap ScalarRef (captured closure variable) — read through the lock
                if let Some(arc) = val.as_scalar_ref() {
                    return arc.read().clone();
                }
                return val.clone();
            }
        }
        PerlValue::UNDEF
    }

    /// True if any frame has a lexical scalar binding for `name` (`my` / `our` / assignment).
    #[inline]
    pub fn scalar_binding_exists(&self, name: &str) -> bool {
        for frame in self.frames.iter().rev() {
            if frame.has_scalar(name) {
                return true;
            }
        }
        false
    }

    /// Collect all scalar variable names across all frames (for debugger).
    pub fn all_scalar_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for frame in &self.frames {
            for (name, _) in &frame.scalars {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            for name in frame.scalar_slot_names.iter().flatten() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }

    /// True if any frame or atomic slot holds an array named `name`.
    #[inline]
    pub fn array_binding_exists(&self, name: &str) -> bool {
        if self.find_atomic_array(name).is_some() {
            return true;
        }
        for frame in self.frames.iter().rev() {
            if frame.has_array(name) {
                return true;
            }
        }
        false
    }

    /// True if any frame or atomic slot holds a hash named `name`.
    #[inline]
    pub fn hash_binding_exists(&self, name: &str) -> bool {
        if self.find_atomic_hash(name).is_some() {
            return true;
        }
        for frame in self.frames.iter().rev() {
            if frame.has_hash(name) {
                return true;
            }
        }
        false
    }

    /// Get the raw scalar value WITHOUT unwrapping Atomic.
    /// Used by scope.capture() to preserve the Arc for sharing across threads.
    #[inline]
    pub fn get_scalar_raw(&self, name: &str) -> PerlValue {
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.get_scalar(name) {
                return val.clone();
            }
        }
        PerlValue::UNDEF
    }

    /// Atomically read-modify-write a scalar. Holds the Mutex lock for
    /// the entire cycle so `mysync` variables are race-free under `fan`/`pfor`.
    /// Returns the NEW value.
    pub fn atomic_mutate(
        &mut self,
        name: &str,
        f: impl FnOnce(&PerlValue) -> PerlValue,
    ) -> PerlValue {
        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.get_scalar(name) {
                if let Some(arc) = v.as_atomic_arc() {
                    let mut guard = arc.lock();
                    let old = guard.clone();
                    let new_val = f(&guard);
                    *guard = new_val.clone();
                    crate::parallel_trace::emit_scalar_mutation(name, &old, &new_val);
                    return new_val;
                }
            }
        }
        // Non-atomic fallback
        let old = self.get_scalar(name);
        let new_val = f(&old);
        let _ = self.set_scalar(name, new_val.clone());
        new_val
    }

    /// Like atomic_mutate but returns the OLD value (for postfix `$x++`).
    pub fn atomic_mutate_post(
        &mut self,
        name: &str,
        f: impl FnOnce(&PerlValue) -> PerlValue,
    ) -> PerlValue {
        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.get_scalar(name) {
                if let Some(arc) = v.as_atomic_arc() {
                    let mut guard = arc.lock();
                    let old = guard.clone();
                    let new_val = f(&old);
                    *guard = new_val.clone();
                    crate::parallel_trace::emit_scalar_mutation(name, &old, &new_val);
                    return old;
                }
            }
        }
        // Non-atomic fallback
        let old = self.get_scalar(name);
        let _ = self.set_scalar(name, f(&old));
        old
    }

    /// Append `rhs` to a scalar string in-place (no clone of the existing string).
    /// If the scalar is not yet a String, it is converted first.
    ///
    /// The binding and the returned [`PerlValue`] share the same heap [`Arc`] via
    /// [`PerlValue::shallow_clone`] on the store — a full [`Clone`] would deep-copy the
    /// entire `String` each time and make repeated `.=` O(N²) in the total length.
    #[inline]
    pub fn scalar_concat_inplace(
        &mut self,
        name: &str,
        rhs: &PerlValue,
    ) -> Result<PerlValue, PerlError> {
        self.check_parallel_scalar_write(name)?;
        for frame in self.frames.iter_mut().rev() {
            if let Some(entry) = frame.scalars.iter_mut().find(|(k, _)| k == name) {
                // `mysync $x` stores `HeapObject::Atomic` — must mutate under the mutex, not
                // `into_string()` the wrapper (that would stringify the cell, not the payload).
                if let Some(atomic_arc) = entry.1.as_atomic_arc() {
                    let mut guard = atomic_arc.lock();
                    let inner = std::mem::replace(&mut *guard, PerlValue::UNDEF);
                    let new_val = inner.concat_append_owned(rhs);
                    *guard = new_val.shallow_clone();
                    return Ok(new_val);
                }
                // Fast path: same `Arc::get_mut` trick as the slot variant — mutate the
                // underlying `String` directly when the scalar is the lone handle.
                if entry.1.try_concat_append_inplace(rhs) {
                    return Ok(entry.1.shallow_clone());
                }
                // Use `into_string` + `append_to` so heap strings take the `Arc::try_unwrap`
                // fast path instead of `Display` / heap formatting on every `.=`.
                let new_val =
                    std::mem::replace(&mut entry.1, PerlValue::UNDEF).concat_append_owned(rhs);
                entry.1 = new_val.shallow_clone();
                return Ok(new_val);
            }
        }
        // Variable not found — create as new string
        let val = PerlValue::UNDEF.concat_append_owned(rhs);
        self.frames[0].set_scalar(name, val.shallow_clone());
        Ok(val)
    }

    #[inline]
    pub fn set_scalar(&mut self, name: &str, val: PerlValue) -> Result<(), PerlError> {
        self.check_parallel_scalar_write(name)?;
        for frame in self.frames.iter_mut().rev() {
            // If the existing value is Atomic, write through the lock
            if let Some(v) = frame.get_scalar(name) {
                if let Some(arc) = v.as_atomic_arc() {
                    let mut guard = arc.lock();
                    let old = guard.clone();
                    *guard = val.clone();
                    crate::parallel_trace::emit_scalar_mutation(name, &old, &val);
                    return Ok(());
                }
                // If the existing value is ScalarRef (captured closure variable), write through it
                if let Some(arc) = v.as_scalar_ref() {
                    *arc.write() = val;
                    return Ok(());
                }
            }
            if frame.has_scalar(name) {
                if let Some(ty) = frame.typed_scalars.get(name) {
                    ty.check_value(&val)
                        .map_err(|msg| PerlError::type_error(format!("`${}`: {}", name, msg), 0))?;
                }
                frame.set_scalar(name, val);
                return Ok(());
            }
        }
        self.frames[0].set_scalar(name, val);
        Ok(())
    }

    /// Set the topic variable `$_` and its numeric alias `$_0` together.
    /// Use this for single-arg closures (map, grep, etc.) so both `$_` and `$_0` work.
    /// This declares them in the current scope (not global), suitable for sub calls.
    ///
    /// Also sets outer topic aliases: `$_<` = previous `$_`, `$_<<` = previous `$_<`, etc.
    /// This allows nested blocks (e.g. `fan` inside `>{}`) to access enclosing topic values.
    #[inline]
    pub fn set_topic(&mut self, val: PerlValue) {
        // Shift existing outer topics down one level before setting new topic.
        // We support up to 4 levels: $_<, $_<<, $_<<<, $_<<<<
        // First, read current values (in reverse order to avoid overwriting what we read).
        let old_3lt = self.get_scalar("_<<<");
        let old_2lt = self.get_scalar("_<<");
        let old_1lt = self.get_scalar("_<");
        let old_topic = self.get_scalar("_");

        // Now set the new values
        self.declare_scalar("_", val.clone());
        self.declare_scalar("_0", val);
        // Set outer topics only if there was a previous topic
        if !old_topic.is_undef() {
            self.declare_scalar("_<", old_topic);
        }
        if !old_1lt.is_undef() {
            self.declare_scalar("_<<", old_1lt);
        }
        if !old_2lt.is_undef() {
            self.declare_scalar("_<<<", old_2lt);
        }
        if !old_3lt.is_undef() {
            self.declare_scalar("_<<<<", old_3lt);
        }
    }

    /// Set numeric closure argument aliases `$_0`, `$_1`, `$_2`, ... for all args.
    /// Also sets `$_` to the first argument (if any), shifting outer topics like [`set_topic`].
    #[inline]
    pub fn set_closure_args(&mut self, args: &[PerlValue]) {
        if let Some(first) = args.first() {
            // Use set_topic to properly shift the topic stack
            self.set_topic(first.clone());
        }
        for (i, val) in args.iter().enumerate() {
            self.declare_scalar(&format!("_{}", i), val.clone());
        }
    }

    /// Register a `defer { BLOCK }` closure to run when this scope exits.
    #[inline]
    pub fn push_defer(&mut self, coderef: PerlValue) {
        if let Some(frame) = self.frames.last_mut() {
            frame.defers.push(coderef);
        }
    }

    /// Take all deferred blocks from the current frame (for execution on scope exit).
    /// Returns them in reverse order (LIFO - last defer runs first).
    #[inline]
    pub fn take_defers(&mut self) -> Vec<PerlValue> {
        if let Some(frame) = self.frames.last_mut() {
            let mut defers = std::mem::take(&mut frame.defers);
            defers.reverse();
            defers
        } else {
            Vec::new()
        }
    }

    // ── Atomic array/hash declarations ──

    pub fn declare_atomic_array(&mut self, name: &str, val: Vec<PerlValue>) {
        if let Some(frame) = self.frames.last_mut() {
            frame
                .atomic_arrays
                .push((name.to_string(), AtomicArray(Arc::new(Mutex::new(val)))));
        }
    }

    pub fn declare_atomic_hash(&mut self, name: &str, val: IndexMap<String, PerlValue>) {
        if let Some(frame) = self.frames.last_mut() {
            frame
                .atomic_hashes
                .push((name.to_string(), AtomicHash(Arc::new(Mutex::new(val)))));
        }
    }

    /// Find an atomic array by name (returns the Arc for sharing).
    fn find_atomic_array(&self, name: &str) -> Option<&AtomicArray> {
        for frame in self.frames.iter().rev() {
            if let Some(aa) = frame.atomic_arrays.iter().find(|(k, _)| k == name) {
                return Some(&aa.1);
            }
        }
        None
    }

    /// Find an atomic hash by name.
    fn find_atomic_hash(&self, name: &str) -> Option<&AtomicHash> {
        for frame in self.frames.iter().rev() {
            if let Some(ah) = frame.atomic_hashes.iter().find(|(k, _)| k == name) {
                return Some(&ah.1);
            }
        }
        None
    }

    // ── Arrays ──

    /// Remove `@_` from the innermost frame without cloning (move out of the frame `sub_underscore` field).
    /// Call sites restore with [`Self::declare_array`] before running a body that uses `shift` / `@_`.
    #[inline]
    pub fn take_sub_underscore(&mut self) -> Option<Vec<PerlValue>> {
        self.frames.last_mut()?.sub_underscore.take()
    }

    pub fn declare_array(&mut self, name: &str, val: Vec<PerlValue>) {
        self.declare_array_frozen(name, val, false);
    }

    pub fn declare_array_frozen(&mut self, name: &str, val: Vec<PerlValue>, frozen: bool) {
        // Package stash names (`Foo::BAR`) live in the outermost frame so nested blocks/subs
        // cannot shadow `@C::ISA` with an empty array (breaks inheritance / SUPER).
        let idx = if name.contains("::") {
            0
        } else {
            self.frames.len().saturating_sub(1)
        };
        if let Some(frame) = self.frames.get_mut(idx) {
            frame.set_array(name, val);
            if frozen {
                frame.frozen_arrays.insert(name.to_string());
            }
        }
    }

    pub fn get_array(&self, name: &str) -> Vec<PerlValue> {
        // Check atomic arrays first
        if let Some(aa) = self.find_atomic_array(name) {
            return aa.0.lock().clone();
        }
        if name.contains("::") {
            if let Some(f) = self.frames.first() {
                if let Some(val) = f.get_array(name) {
                    return val.clone();
                }
            }
            return Vec::new();
        }
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.get_array(name) {
                return val.clone();
            }
        }
        Vec::new()
    }

    /// Borrow the innermost binding for `name` when it is a plain [`Vec`] (not `mysync`).
    /// Used to pass `@_` to [`crate::list_util::native_dispatch`] without cloning the vector.
    #[inline]
    pub fn get_array_borrow(&self, name: &str) -> Option<&[PerlValue]> {
        if self.find_atomic_array(name).is_some() {
            return None;
        }
        if name.contains("::") {
            return self
                .frames
                .first()
                .and_then(|f| f.get_array(name))
                .map(|v| v.as_slice());
        }
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.get_array(name) {
                return Some(val.as_slice());
            }
        }
        None
    }

    fn resolve_array_frame_idx(&self, name: &str) -> Option<usize> {
        if name.contains("::") {
            return Some(0);
        }
        (0..self.frames.len())
            .rev()
            .find(|&i| self.frames[i].has_array(name))
    }

    fn check_parallel_array_write(&self, name: &str) -> Result<(), PerlError> {
        if !self.parallel_guard
            || Self::parallel_skip_special_name(name)
            || Self::parallel_allowed_internal_array(name)
        {
            return Ok(());
        }
        let inner = self.frames.len().saturating_sub(1);
        match self.resolve_array_frame_idx(name) {
            None => Err(PerlError::runtime(
                format!(
                    "cannot modify undeclared array `@{}` in a parallel block",
                    name
                ),
                0,
            )),
            Some(idx) if idx != inner => Err(PerlError::runtime(
                format!(
                    "cannot modify captured non-mysync array `@{}` in a parallel block",
                    name
                ),
                0,
            )),
            Some(_) => Ok(()),
        }
    }

    pub fn get_array_mut(&mut self, name: &str) -> Result<&mut Vec<PerlValue>, PerlError> {
        // Note: can't return &mut into a Mutex. Callers needing atomic array
        // mutation should use atomic_array_mutate instead. For non-atomic arrays:
        if self.find_atomic_array(name).is_some() {
            return Err(PerlError::runtime(
                "get_array_mut: use atomic path for mysync arrays",
                0,
            ));
        }
        self.check_parallel_array_write(name)?;
        let idx = self.resolve_array_frame_idx(name).unwrap_or_default();
        let frame = &mut self.frames[idx];
        if frame.get_array_mut(name).is_none() {
            frame.arrays.push((name.to_string(), Vec::new()));
        }
        Ok(frame.get_array_mut(name).unwrap())
    }

    /// Push to array — works for both regular and atomic arrays.
    pub fn push_to_array(&mut self, name: &str, val: PerlValue) -> Result<(), PerlError> {
        if let Some(aa) = self.find_atomic_array(name) {
            aa.0.lock().push(val);
            return Ok(());
        }
        self.get_array_mut(name)?.push(val);
        Ok(())
    }

    /// Bulk `push @name, start..end-1` for the fused counted-loop superinstruction:
    /// reserves the `Vec` once, then pushes `PerlValue::integer(i)` for `i in start..end`
    /// in a tight Rust loop. Atomic arrays take a single `lock().push()` burst.
    pub fn push_int_range_to_array(
        &mut self,
        name: &str,
        start: i64,
        end: i64,
    ) -> Result<(), PerlError> {
        if end <= start {
            return Ok(());
        }
        let count = (end - start) as usize;
        if let Some(aa) = self.find_atomic_array(name) {
            let mut g = aa.0.lock();
            g.reserve(count);
            for i in start..end {
                g.push(PerlValue::integer(i));
            }
            return Ok(());
        }
        let arr = self.get_array_mut(name)?;
        arr.reserve(count);
        for i in start..end {
            arr.push(PerlValue::integer(i));
        }
        Ok(())
    }

    /// Pop from array — works for both regular and atomic arrays.
    pub fn pop_from_array(&mut self, name: &str) -> Result<PerlValue, PerlError> {
        if let Some(aa) = self.find_atomic_array(name) {
            return Ok(aa.0.lock().pop().unwrap_or(PerlValue::UNDEF));
        }
        Ok(self.get_array_mut(name)?.pop().unwrap_or(PerlValue::UNDEF))
    }

    /// Shift from array — works for both regular and atomic arrays.
    pub fn shift_from_array(&mut self, name: &str) -> Result<PerlValue, PerlError> {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut guard = aa.0.lock();
            return Ok(if guard.is_empty() {
                PerlValue::UNDEF
            } else {
                guard.remove(0)
            });
        }
        let arr = self.get_array_mut(name)?;
        Ok(if arr.is_empty() {
            PerlValue::UNDEF
        } else {
            arr.remove(0)
        })
    }

    /// Get array length — works for both regular and atomic arrays.
    pub fn array_len(&self, name: &str) -> usize {
        if let Some(aa) = self.find_atomic_array(name) {
            return aa.0.lock().len();
        }
        if name.contains("::") {
            return self
                .frames
                .first()
                .and_then(|f| f.get_array(name))
                .map(|a| a.len())
                .unwrap_or(0);
        }
        for frame in self.frames.iter().rev() {
            if let Some(arr) = frame.get_array(name) {
                return arr.len();
            }
        }
        0
    }

    pub fn set_array(&mut self, name: &str, val: Vec<PerlValue>) -> Result<(), PerlError> {
        if let Some(aa) = self.find_atomic_array(name) {
            *aa.0.lock() = val;
            return Ok(());
        }
        self.check_parallel_array_write(name)?;
        for frame in self.frames.iter_mut().rev() {
            if frame.has_array(name) {
                frame.set_array(name, val);
                return Ok(());
            }
        }
        self.frames[0].set_array(name, val);
        Ok(())
    }

    /// Direct element access — works for both regular and atomic arrays.
    #[inline]
    pub fn get_array_element(&self, name: &str, index: i64) -> PerlValue {
        if let Some(aa) = self.find_atomic_array(name) {
            let arr = aa.0.lock();
            let idx = if index < 0 {
                (arr.len() as i64 + index) as usize
            } else {
                index as usize
            };
            return arr.get(idx).cloned().unwrap_or(PerlValue::UNDEF);
        }
        for frame in self.frames.iter().rev() {
            if let Some(arr) = frame.get_array(name) {
                let idx = if index < 0 {
                    (arr.len() as i64 + index) as usize
                } else {
                    index as usize
                };
                return arr.get(idx).cloned().unwrap_or(PerlValue::UNDEF);
            }
        }
        PerlValue::UNDEF
    }

    pub fn set_array_element(
        &mut self,
        name: &str,
        index: i64,
        val: PerlValue,
    ) -> Result<(), PerlError> {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut arr = aa.0.lock();
            let idx = if index < 0 {
                (arr.len() as i64 + index).max(0) as usize
            } else {
                index as usize
            };
            if idx >= arr.len() {
                arr.resize(idx + 1, PerlValue::UNDEF);
            }
            arr[idx] = val;
            return Ok(());
        }
        let arr = self.get_array_mut(name)?;
        let idx = if index < 0 {
            let len = arr.len() as i64;
            (len + index).max(0) as usize
        } else {
            index as usize
        };
        if idx >= arr.len() {
            arr.resize(idx + 1, PerlValue::UNDEF);
        }
        arr[idx] = val;
        Ok(())
    }

    /// Perl `exists $a[$i]` — true when the slot index is within the current array length.
    pub fn exists_array_element(&self, name: &str, index: i64) -> bool {
        if let Some(aa) = self.find_atomic_array(name) {
            let arr = aa.0.lock();
            let idx = if index < 0 {
                (arr.len() as i64 + index) as usize
            } else {
                index as usize
            };
            return idx < arr.len();
        }
        for frame in self.frames.iter().rev() {
            if let Some(arr) = frame.get_array(name) {
                let idx = if index < 0 {
                    (arr.len() as i64 + index) as usize
                } else {
                    index as usize
                };
                return idx < arr.len();
            }
        }
        false
    }

    /// Perl `delete $a[$i]` — sets the element to `undef`, returns the previous value.
    pub fn delete_array_element(&mut self, name: &str, index: i64) -> Result<PerlValue, PerlError> {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut arr = aa.0.lock();
            let idx = if index < 0 {
                (arr.len() as i64 + index) as usize
            } else {
                index as usize
            };
            if idx >= arr.len() {
                return Ok(PerlValue::UNDEF);
            }
            let old = arr.get(idx).cloned().unwrap_or(PerlValue::UNDEF);
            arr[idx] = PerlValue::UNDEF;
            return Ok(old);
        }
        let arr = self.get_array_mut(name)?;
        let idx = if index < 0 {
            (arr.len() as i64 + index) as usize
        } else {
            index as usize
        };
        if idx >= arr.len() {
            return Ok(PerlValue::UNDEF);
        }
        let old = arr.get(idx).cloned().unwrap_or(PerlValue::UNDEF);
        arr[idx] = PerlValue::UNDEF;
        Ok(old)
    }

    // ── Hashes ──

    #[inline]
    pub fn declare_hash(&mut self, name: &str, val: IndexMap<String, PerlValue>) {
        self.declare_hash_frozen(name, val, false);
    }

    pub fn declare_hash_frozen(
        &mut self,
        name: &str,
        val: IndexMap<String, PerlValue>,
        frozen: bool,
    ) {
        if let Some(frame) = self.frames.last_mut() {
            frame.set_hash(name, val);
            if frozen {
                frame.frozen_hashes.insert(name.to_string());
            }
        }
    }

    /// Declare a hash in the bottom (global) frame, not the current lexical frame.
    pub fn declare_hash_global(&mut self, name: &str, val: IndexMap<String, PerlValue>) {
        if let Some(frame) = self.frames.first_mut() {
            frame.set_hash(name, val);
        }
    }

    /// Declare a frozen hash in the bottom (global) frame — prevents user reassignment.
    pub fn declare_hash_global_frozen(&mut self, name: &str, val: IndexMap<String, PerlValue>) {
        if let Some(frame) = self.frames.first_mut() {
            frame.set_hash(name, val);
            frame.frozen_hashes.insert(name.to_string());
        }
    }

    /// Returns `true` if a lexical (non-bottom) frame declares `%name`.
    pub fn has_lexical_hash(&self, name: &str) -> bool {
        self.frames.iter().skip(1).any(|f| f.has_hash(name))
    }

    /// Returns `true` if ANY frame (including global) declares `%name`.
    pub fn any_frame_has_hash(&self, name: &str) -> bool {
        self.frames.iter().any(|f| f.has_hash(name))
    }

    pub fn get_hash(&self, name: &str) -> IndexMap<String, PerlValue> {
        if let Some(ah) = self.find_atomic_hash(name) {
            return ah.0.lock().clone();
        }
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.get_hash(name) {
                return val.clone();
            }
        }
        IndexMap::new()
    }

    fn resolve_hash_frame_idx(&self, name: &str) -> Option<usize> {
        if name.contains("::") {
            return Some(0);
        }
        (0..self.frames.len())
            .rev()
            .find(|&i| self.frames[i].has_hash(name))
    }

    fn check_parallel_hash_write(&self, name: &str) -> Result<(), PerlError> {
        if !self.parallel_guard
            || Self::parallel_skip_special_name(name)
            || Self::parallel_allowed_internal_hash(name)
        {
            return Ok(());
        }
        let inner = self.frames.len().saturating_sub(1);
        match self.resolve_hash_frame_idx(name) {
            None => Err(PerlError::runtime(
                format!(
                    "cannot modify undeclared hash `%{}` in a parallel block",
                    name
                ),
                0,
            )),
            Some(idx) if idx != inner => Err(PerlError::runtime(
                format!(
                    "cannot modify captured non-mysync hash `%{}` in a parallel block",
                    name
                ),
                0,
            )),
            Some(_) => Ok(()),
        }
    }

    pub fn get_hash_mut(
        &mut self,
        name: &str,
    ) -> Result<&mut IndexMap<String, PerlValue>, PerlError> {
        if self.find_atomic_hash(name).is_some() {
            return Err(PerlError::runtime(
                "get_hash_mut: use atomic path for mysync hashes",
                0,
            ));
        }
        self.check_parallel_hash_write(name)?;
        let idx = self.resolve_hash_frame_idx(name).unwrap_or_default();
        let frame = &mut self.frames[idx];
        if frame.get_hash_mut(name).is_none() {
            frame.hashes.push((name.to_string(), IndexMap::new()));
        }
        Ok(frame.get_hash_mut(name).unwrap())
    }

    pub fn set_hash(
        &mut self,
        name: &str,
        val: IndexMap<String, PerlValue>,
    ) -> Result<(), PerlError> {
        if let Some(ah) = self.find_atomic_hash(name) {
            *ah.0.lock() = val;
            return Ok(());
        }
        self.check_parallel_hash_write(name)?;
        for frame in self.frames.iter_mut().rev() {
            if frame.has_hash(name) {
                frame.set_hash(name, val);
                return Ok(());
            }
        }
        self.frames[0].set_hash(name, val);
        Ok(())
    }

    #[inline]
    pub fn get_hash_element(&self, name: &str, key: &str) -> PerlValue {
        if let Some(ah) = self.find_atomic_hash(name) {
            return ah.0.lock().get(key).cloned().unwrap_or(PerlValue::UNDEF);
        }
        for frame in self.frames.iter().rev() {
            if let Some(hash) = frame.get_hash(name) {
                return hash.get(key).cloned().unwrap_or(PerlValue::UNDEF);
            }
        }
        PerlValue::UNDEF
    }

    /// Atomically read-modify-write a hash element. For atomic hashes, holds
    /// the Mutex for the full cycle. Returns the new value.
    pub fn atomic_hash_mutate(
        &mut self,
        name: &str,
        key: &str,
        f: impl FnOnce(&PerlValue) -> PerlValue,
    ) -> Result<PerlValue, PerlError> {
        if let Some(ah) = self.find_atomic_hash(name) {
            let mut guard = ah.0.lock();
            let old = guard.get(key).cloned().unwrap_or(PerlValue::UNDEF);
            let new_val = f(&old);
            guard.insert(key.to_string(), new_val.clone());
            return Ok(new_val);
        }
        // Non-atomic fallback
        let old = self.get_hash_element(name, key);
        let new_val = f(&old);
        self.set_hash_element(name, key, new_val.clone())?;
        Ok(new_val)
    }

    /// Atomically read-modify-write an array element. Returns the new value.
    pub fn atomic_array_mutate(
        &mut self,
        name: &str,
        index: i64,
        f: impl FnOnce(&PerlValue) -> PerlValue,
    ) -> Result<PerlValue, PerlError> {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut guard = aa.0.lock();
            let idx = if index < 0 {
                (guard.len() as i64 + index).max(0) as usize
            } else {
                index as usize
            };
            if idx >= guard.len() {
                guard.resize(idx + 1, PerlValue::UNDEF);
            }
            let old = guard[idx].clone();
            let new_val = f(&old);
            guard[idx] = new_val.clone();
            return Ok(new_val);
        }
        // Non-atomic fallback
        let old = self.get_array_element(name, index);
        let new_val = f(&old);
        self.set_array_element(name, index, new_val.clone())?;
        Ok(new_val)
    }

    pub fn set_hash_element(
        &mut self,
        name: &str,
        key: &str,
        val: PerlValue,
    ) -> Result<(), PerlError> {
        // `$SIG{INT} = \&h` — lazily install the matching signal hook. Until Perl code touches
        // `%SIG`, the POSIX default stays in place so Ctrl-C terminates immediately.
        if name == "SIG" {
            crate::perl_signal::install(key);
        }
        if let Some(ah) = self.find_atomic_hash(name) {
            ah.0.lock().insert(key.to_string(), val);
            return Ok(());
        }
        let hash = self.get_hash_mut(name)?;
        hash.insert(key.to_string(), val);
        Ok(())
    }

    /// Bulk `for i in start..end { $h{i} = i * k }` for the fused hash-insert loop.
    /// Reserves capacity once and runs the whole range in a tight Rust loop.
    /// `itoa` is used to stringify each key without a transient `format!` allocation.
    pub fn set_hash_int_times_range(
        &mut self,
        name: &str,
        start: i64,
        end: i64,
        k: i64,
    ) -> Result<(), PerlError> {
        if end <= start {
            return Ok(());
        }
        let count = (end - start) as usize;
        if let Some(ah) = self.find_atomic_hash(name) {
            let mut g = ah.0.lock();
            g.reserve(count);
            let mut buf = itoa::Buffer::new();
            for i in start..end {
                let key = buf.format(i).to_owned();
                g.insert(key, PerlValue::integer(i.wrapping_mul(k)));
            }
            return Ok(());
        }
        let hash = self.get_hash_mut(name)?;
        hash.reserve(count);
        let mut buf = itoa::Buffer::new();
        for i in start..end {
            let key = buf.format(i).to_owned();
            hash.insert(key, PerlValue::integer(i.wrapping_mul(k)));
        }
        Ok(())
    }

    pub fn delete_hash_element(&mut self, name: &str, key: &str) -> Result<PerlValue, PerlError> {
        if let Some(ah) = self.find_atomic_hash(name) {
            return Ok(ah.0.lock().shift_remove(key).unwrap_or(PerlValue::UNDEF));
        }
        let hash = self.get_hash_mut(name)?;
        Ok(hash.shift_remove(key).unwrap_or(PerlValue::UNDEF))
    }

    #[inline]
    pub fn exists_hash_element(&self, name: &str, key: &str) -> bool {
        if let Some(ah) = self.find_atomic_hash(name) {
            return ah.0.lock().contains_key(key);
        }
        for frame in self.frames.iter().rev() {
            if let Some(hash) = frame.get_hash(name) {
                return hash.contains_key(key);
            }
        }
        false
    }

    /// Walk all values of the named hash with a visitor. Used by the fused
    /// `for my $k (keys %h) { $sum += $h{$k} }` op so the hot loop runs without
    /// cloning the entire map into a keys array (vs the un-fused shape, which
    /// allocates one `PerlValue::string` per key).
    #[inline]
    pub fn for_each_hash_value(&self, name: &str, mut visit: impl FnMut(&PerlValue)) {
        if let Some(ah) = self.find_atomic_hash(name) {
            let g = ah.0.lock();
            for v in g.values() {
                visit(v);
            }
            return;
        }
        for frame in self.frames.iter().rev() {
            if let Some(hash) = frame.get_hash(name) {
                for v in hash.values() {
                    visit(v);
                }
                return;
            }
        }
    }

    /// Sigil-prefixed names (`$x`, `@a`, `%h`) from all frames, for REPL tab-completion.
    pub fn repl_binding_names(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for frame in &self.frames {
            for (name, _) in &frame.scalars {
                let s = format!("${}", name);
                if seen.insert(s.clone()) {
                    out.push(s);
                }
            }
            for (name, _) in &frame.arrays {
                let s = format!("@{}", name);
                if seen.insert(s.clone()) {
                    out.push(s);
                }
            }
            for (name, _) in &frame.hashes {
                let s = format!("%{}", name);
                if seen.insert(s.clone()) {
                    out.push(s);
                }
            }
            for (name, _) in &frame.atomic_arrays {
                let s = format!("@{}", name);
                if seen.insert(s.clone()) {
                    out.push(s);
                }
            }
            for (name, _) in &frame.atomic_hashes {
                let s = format!("%{}", name);
                if seen.insert(s.clone()) {
                    out.push(s);
                }
            }
        }
        out.sort();
        out
    }

    pub fn capture(&mut self) -> Vec<(String, PerlValue)> {
        let mut captured = Vec::new();
        for frame in &mut self.frames {
            for (k, v) in &mut frame.scalars {
                // Wrap scalar in ScalarRef so the closure shares the same memory cell.
                // If it's already a ScalarRef, just clone it (shares the same Arc).
                // Only wrap simple scalars (integers, floats, strings, undef); complex values
                // like refs, blessed objects, atomics, etc. already share via Arc and wrapping
                // them in ScalarRef breaks type detection (as_ppool, as_blessed_ref, etc.).
                if v.as_scalar_ref().is_some() {
                    captured.push((format!("${}", k), v.clone()));
                } else if v.is_simple_scalar() {
                    let wrapped = PerlValue::scalar_ref(Arc::new(RwLock::new(v.clone())));
                    // Update the original scope variable to point to the same ScalarRef
                    // so that subsequent closures share the same reference.
                    *v = wrapped.clone();
                    captured.push((format!("${}", k), wrapped));
                } else {
                    captured.push((format!("${}", k), v.clone()));
                }
            }
            for (i, v) in frame.scalar_slots.iter().enumerate() {
                if let Some(Some(name)) = frame.scalar_slot_names.get(i) {
                    // Scalar slots are used by the VM; don't modify them in-place.
                    // Wrap in ScalarRef for the captured closure environment only.
                    let wrapped = if v.as_scalar_ref().is_some() {
                        v.clone()
                    } else {
                        PerlValue::scalar_ref(Arc::new(RwLock::new(v.clone())))
                    };
                    captured.push((format!("$slot:{}:{}", i, name), wrapped));
                }
            }
            for (k, v) in &frame.arrays {
                if capture_skip_bootstrap_array(k) {
                    continue;
                }
                if frame.frozen_arrays.contains(k) {
                    captured.push((format!("@frozen:{}", k), PerlValue::array(v.clone())));
                } else {
                    captured.push((format!("@{}", k), PerlValue::array(v.clone())));
                }
            }
            for (k, v) in &frame.hashes {
                if capture_skip_bootstrap_hash(k) {
                    continue;
                }
                if frame.frozen_hashes.contains(k) {
                    captured.push((format!("%frozen:{}", k), PerlValue::hash(v.clone())));
                } else {
                    captured.push((format!("%{}", k), PerlValue::hash(v.clone())));
                }
            }
            for (k, _aa) in &frame.atomic_arrays {
                captured.push((
                    format!("@sync_{}", k),
                    PerlValue::atomic(Arc::new(Mutex::new(PerlValue::string(String::new())))),
                ));
            }
            for (k, _ah) in &frame.atomic_hashes {
                captured.push((
                    format!("%sync_{}", k),
                    PerlValue::atomic(Arc::new(Mutex::new(PerlValue::string(String::new())))),
                ));
            }
        }
        captured
    }

    /// Extended capture that returns atomic arrays/hashes separately.
    pub fn capture_with_atomics(&self) -> ScopeCaptureWithAtomics {
        let mut scalars = Vec::new();
        let mut arrays = Vec::new();
        let mut hashes = Vec::new();
        for frame in &self.frames {
            for (k, v) in &frame.scalars {
                scalars.push((format!("${}", k), v.clone()));
            }
            for (i, v) in frame.scalar_slots.iter().enumerate() {
                if let Some(Some(name)) = frame.scalar_slot_names.get(i) {
                    scalars.push((format!("$slot:{}:{}", i, name), v.clone()));
                }
            }
            for (k, v) in &frame.arrays {
                if capture_skip_bootstrap_array(k) {
                    continue;
                }
                if frame.frozen_arrays.contains(k) {
                    scalars.push((format!("@frozen:{}", k), PerlValue::array(v.clone())));
                } else {
                    scalars.push((format!("@{}", k), PerlValue::array(v.clone())));
                }
            }
            for (k, v) in &frame.hashes {
                if capture_skip_bootstrap_hash(k) {
                    continue;
                }
                if frame.frozen_hashes.contains(k) {
                    scalars.push((format!("%frozen:{}", k), PerlValue::hash(v.clone())));
                } else {
                    scalars.push((format!("%{}", k), PerlValue::hash(v.clone())));
                }
            }
            for (k, aa) in &frame.atomic_arrays {
                arrays.push((k.clone(), aa.clone()));
            }
            for (k, ah) in &frame.atomic_hashes {
                hashes.push((k.clone(), ah.clone()));
            }
        }
        (scalars, arrays, hashes)
    }

    pub fn restore_capture(&mut self, captured: &[(String, PerlValue)]) {
        for (name, val) in captured {
            if let Some(rest) = name.strip_prefix("$slot:") {
                // "$slot:INDEX:NAME" — restore into both scalar_slots and scalars.
                if let Some(colon) = rest.find(':') {
                    let idx: usize = rest[..colon].parse().unwrap_or(0);
                    let sname = &rest[colon + 1..];
                    self.declare_scalar_slot(idx as u8, val.clone(), Some(sname));
                    self.declare_scalar(sname, val.clone());
                }
            } else if let Some(stripped) = name.strip_prefix('$') {
                self.declare_scalar(stripped, val.clone());
            } else if let Some(rest) = name.strip_prefix("@frozen:") {
                let arr = val.as_array_vec().unwrap_or_else(|| val.to_list());
                self.declare_array_frozen(rest, arr, true);
            } else if let Some(rest) = name.strip_prefix("%frozen:") {
                if let Some(h) = val.as_hash_map() {
                    self.declare_hash_frozen(rest, h.clone(), true);
                }
            } else if let Some(rest) = name.strip_prefix('@') {
                if rest.starts_with("sync_") {
                    continue;
                }
                let arr = val.as_array_vec().unwrap_or_else(|| val.to_list());
                self.declare_array(rest, arr);
            } else if let Some(rest) = name.strip_prefix('%') {
                if rest.starts_with("sync_") {
                    continue;
                }
                if let Some(h) = val.as_hash_map() {
                    self.declare_hash(rest, h.clone());
                }
            }
        }
    }

    /// Restore atomic arrays/hashes from capture_with_atomics.
    pub fn restore_atomics(
        &mut self,
        arrays: &[(String, AtomicArray)],
        hashes: &[(String, AtomicHash)],
    ) {
        if let Some(frame) = self.frames.last_mut() {
            for (name, aa) in arrays {
                frame.atomic_arrays.push((name.clone(), aa.clone()));
            }
            for (name, ah) in hashes {
                frame.atomic_hashes.push((name.clone(), ah.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::PerlValue;

    #[test]
    fn missing_scalar_is_undef() {
        let s = Scope::new();
        assert!(s.get_scalar("not_declared").is_undef());
    }

    #[test]
    fn inner_frame_shadows_outer_scalar() {
        let mut s = Scope::new();
        s.declare_scalar("a", PerlValue::integer(1));
        s.push_frame();
        s.declare_scalar("a", PerlValue::integer(2));
        assert_eq!(s.get_scalar("a").to_int(), 2);
        s.pop_frame();
        assert_eq!(s.get_scalar("a").to_int(), 1);
    }

    #[test]
    fn set_scalar_updates_innermost_binding() {
        let mut s = Scope::new();
        s.declare_scalar("a", PerlValue::integer(1));
        s.push_frame();
        s.declare_scalar("a", PerlValue::integer(2));
        let _ = s.set_scalar("a", PerlValue::integer(99));
        assert_eq!(s.get_scalar("a").to_int(), 99);
        s.pop_frame();
        assert_eq!(s.get_scalar("a").to_int(), 1);
    }

    #[test]
    fn array_negative_index_reads_from_end() {
        let mut s = Scope::new();
        s.declare_array(
            "a",
            vec![
                PerlValue::integer(10),
                PerlValue::integer(20),
                PerlValue::integer(30),
            ],
        );
        assert_eq!(s.get_array_element("a", -1).to_int(), 30);
    }

    #[test]
    fn set_array_element_extends_array_with_undef_gaps() {
        let mut s = Scope::new();
        s.declare_array("a", vec![]);
        s.set_array_element("a", 2, PerlValue::integer(7)).unwrap();
        assert_eq!(s.get_array_element("a", 2).to_int(), 7);
        assert!(s.get_array_element("a", 0).is_undef());
    }

    #[test]
    fn capture_restore_roundtrip_scalar() {
        let mut s = Scope::new();
        s.declare_scalar("n", PerlValue::integer(42));
        let cap = s.capture();
        let mut t = Scope::new();
        t.restore_capture(&cap);
        assert_eq!(t.get_scalar("n").to_int(), 42);
    }

    #[test]
    fn capture_restore_roundtrip_lexical_array_and_hash() {
        let mut s = Scope::new();
        s.declare_array("a", vec![PerlValue::integer(1), PerlValue::integer(2)]);
        let mut m = IndexMap::new();
        m.insert("k".to_string(), PerlValue::integer(99));
        s.declare_hash("h", m);
        let cap = s.capture();
        let mut t = Scope::new();
        t.restore_capture(&cap);
        assert_eq!(t.get_array_element("a", 1).to_int(), 2);
        assert_eq!(t.get_hash_element("h", "k").to_int(), 99);
    }

    #[test]
    fn hash_get_set_delete_exists() {
        let mut s = Scope::new();
        let mut m = IndexMap::new();
        m.insert("k".to_string(), PerlValue::integer(1));
        s.declare_hash("h", m);
        assert_eq!(s.get_hash_element("h", "k").to_int(), 1);
        assert!(s.exists_hash_element("h", "k"));
        s.set_hash_element("h", "k", PerlValue::integer(99))
            .unwrap();
        assert_eq!(s.get_hash_element("h", "k").to_int(), 99);
        let del = s.delete_hash_element("h", "k").unwrap();
        assert_eq!(del.to_int(), 99);
        assert!(!s.exists_hash_element("h", "k"));
    }

    #[test]
    fn inner_frame_shadows_outer_hash_name() {
        let mut s = Scope::new();
        let mut outer = IndexMap::new();
        outer.insert("k".to_string(), PerlValue::integer(1));
        s.declare_hash("h", outer);
        s.push_frame();
        let mut inner = IndexMap::new();
        inner.insert("k".to_string(), PerlValue::integer(2));
        s.declare_hash("h", inner);
        assert_eq!(s.get_hash_element("h", "k").to_int(), 2);
        s.pop_frame();
        assert_eq!(s.get_hash_element("h", "k").to_int(), 1);
    }

    #[test]
    fn inner_frame_shadows_outer_array_name() {
        let mut s = Scope::new();
        s.declare_array("a", vec![PerlValue::integer(1)]);
        s.push_frame();
        s.declare_array("a", vec![PerlValue::integer(2), PerlValue::integer(3)]);
        assert_eq!(s.get_array_element("a", 1).to_int(), 3);
        s.pop_frame();
        assert_eq!(s.get_array_element("a", 0).to_int(), 1);
    }

    #[test]
    fn pop_frame_never_removes_global_frame() {
        let mut s = Scope::new();
        s.declare_scalar("x", PerlValue::integer(1));
        s.pop_frame();
        s.pop_frame();
        assert_eq!(s.get_scalar("x").to_int(), 1);
    }

    #[test]
    fn empty_array_declared_has_zero_length() {
        let mut s = Scope::new();
        s.declare_array("a", vec![]);
        assert_eq!(s.get_array("a").len(), 0);
    }

    #[test]
    fn depth_increments_with_push_frame() {
        let mut s = Scope::new();
        let d0 = s.depth();
        s.push_frame();
        assert_eq!(s.depth(), d0 + 1);
        s.pop_frame();
        assert_eq!(s.depth(), d0);
    }

    #[test]
    fn pop_to_depth_unwinds_to_target() {
        let mut s = Scope::new();
        s.push_frame();
        s.push_frame();
        let target = s.depth() - 1;
        s.pop_to_depth(target);
        assert_eq!(s.depth(), target);
    }

    #[test]
    fn array_len_and_push_pop_roundtrip() {
        let mut s = Scope::new();
        s.declare_array("a", vec![]);
        assert_eq!(s.array_len("a"), 0);
        s.push_to_array("a", PerlValue::integer(1)).unwrap();
        s.push_to_array("a", PerlValue::integer(2)).unwrap();
        assert_eq!(s.array_len("a"), 2);
        assert_eq!(s.pop_from_array("a").unwrap().to_int(), 2);
        assert_eq!(s.pop_from_array("a").unwrap().to_int(), 1);
        assert!(s.pop_from_array("a").unwrap().is_undef());
    }

    #[test]
    fn shift_from_array_drops_front() {
        let mut s = Scope::new();
        s.declare_array("a", vec![PerlValue::integer(1), PerlValue::integer(2)]);
        assert_eq!(s.shift_from_array("a").unwrap().to_int(), 1);
        assert_eq!(s.array_len("a"), 1);
    }

    #[test]
    fn atomic_mutate_increments_wrapped_scalar() {
        use parking_lot::Mutex;
        use std::sync::Arc;
        let mut s = Scope::new();
        s.declare_scalar(
            "n",
            PerlValue::atomic(Arc::new(Mutex::new(PerlValue::integer(10)))),
        );
        let v = s.atomic_mutate("n", |old| PerlValue::integer(old.to_int() + 5));
        assert_eq!(v.to_int(), 15);
        assert_eq!(s.get_scalar("n").to_int(), 15);
    }

    #[test]
    fn atomic_mutate_post_returns_old_value() {
        use parking_lot::Mutex;
        use std::sync::Arc;
        let mut s = Scope::new();
        s.declare_scalar(
            "n",
            PerlValue::atomic(Arc::new(Mutex::new(PerlValue::integer(7)))),
        );
        let old = s.atomic_mutate_post("n", |v| PerlValue::integer(v.to_int() + 1));
        assert_eq!(old.to_int(), 7);
        assert_eq!(s.get_scalar("n").to_int(), 8);
    }

    #[test]
    fn get_scalar_raw_keeps_atomic_wrapper() {
        use parking_lot::Mutex;
        use std::sync::Arc;
        let mut s = Scope::new();
        s.declare_scalar(
            "n",
            PerlValue::atomic(Arc::new(Mutex::new(PerlValue::integer(3)))),
        );
        assert!(s.get_scalar_raw("n").is_atomic());
        assert!(!s.get_scalar("n").is_atomic());
    }

    #[test]
    fn missing_array_element_is_undef() {
        let mut s = Scope::new();
        s.declare_array("a", vec![PerlValue::integer(1)]);
        assert!(s.get_array_element("a", 99).is_undef());
    }

    #[test]
    fn restore_atomics_puts_atomic_containers_in_frame() {
        use indexmap::IndexMap;
        use parking_lot::Mutex;
        use std::sync::Arc;
        let mut s = Scope::new();
        let aa = AtomicArray(Arc::new(Mutex::new(vec![PerlValue::integer(1)])));
        let ah = AtomicHash(Arc::new(Mutex::new(IndexMap::new())));
        s.restore_atomics(&[("ax".into(), aa.clone())], &[("hx".into(), ah.clone())]);
        assert_eq!(s.get_array_element("ax", 0).to_int(), 1);
        assert_eq!(s.array_len("ax"), 1);
        s.set_hash_element("hx", "k", PerlValue::integer(2))
            .unwrap();
        assert_eq!(s.get_hash_element("hx", "k").to_int(), 2);
    }
}
