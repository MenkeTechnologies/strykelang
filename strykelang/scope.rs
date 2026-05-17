use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};

use crate::ast::PerlTypeName;
use crate::error::StrykeError;
use crate::value::StrykeValue;

/// Thread-safe shared array for `mysync @a`.
#[derive(Debug, Clone)]
pub struct AtomicArray(pub Arc<Mutex<Vec<StrykeValue>>>);

/// Thread-safe shared hash for `mysync %h`.
#[derive(Debug, Clone)]
pub struct AtomicHash(pub Arc<Mutex<IndexMap<String, StrykeValue>>>);

type ScopeCaptureWithAtomics = (
    Vec<(String, StrykeValue)>,
    Vec<(String, AtomicArray)>,
    Vec<(String, AtomicHash)>,
);

/// Storage for hashes promoted to shared Arc-backed RwLocks (see [`Frame::shared_hashes`]).
/// Aliased to keep the field declaration readable (clippy::type_complexity).
type SharedHashEntry = (
    String,
    Arc<parking_lot::RwLock<IndexMap<String, StrykeValue>>>,
);

/// `main` is the default package — `$main::X` ≡ `$X`, `@main::INC` ≡
/// `@INC`, `%main::ENV` ≡ `%ENV`. Storage uses the bare key, so every
/// scope getter has to short-circuit `main::name` (with no further
/// `::`) through the unqualified lookup. Returns the bare suffix when
/// the name has the exact `main::ident` shape; `None` otherwise (incl.
/// `main::Pkg::name`, where `Pkg` is a real subpackage that must keep
/// its qualified storage key).
#[inline]
pub(crate) fn strip_main_prefix(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("main::")?;
    if rest.contains("::") {
        return None;
    }
    Some(rest)
}

/// Canonicalize a `main::name` query into the bare `name` form.
/// Shadow-binds `$name` so the rest of the function body operates on
/// the canonical key. No allocation — the borrow is reused.
macro_rules! canon_main {
    ($name:ident) => {
        let $name: &str = $crate::scope::strip_main_prefix($name).unwrap_or($name);
    };
}

/// Arrays installed by [`crate::vm_helper::VMHelper::new`] on the outer frame. They must not be
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

/// Parse a positional topic-slot scalar name (no leading sigil) and return the
/// slot index N. Recognizes `_N` and the outer-chain forms `_N<`, `_N<<`,
/// `_N<<<`, `_N<<<<`. Returns `None` for `_`, `_<`, etc. (slot 0, which never
/// needs to bump `max_active_slot`).
#[inline]
/// Return the alias name for a topic-variant if `_` ↔ `_0` apply at any
/// chain level. `_` ↔ `_0`, `_<` ↔ `_0<`, `_<<<` ↔ `_0<<<`, etc. Anything
/// else (`_1`, `_2<<`, …) has no alias (those are positional-only slots).
fn topic_alias(name: &str) -> Option<String> {
    if name == "_" { return Some("_0".to_string()); }
    if name == "_0" { return Some("_".to_string()); }
    // _<+ → _0<+, _0<+ → _<+
    if let Some(rest) = name.strip_prefix("_0") {
        if rest.chars().all(|c| c == '<') && !rest.is_empty() {
            return Some(format!("_{rest}"));
        }
    }
    if let Some(rest) = name.strip_prefix('_') {
        if rest.chars().all(|c| c == '<') && !rest.is_empty() {
            return Some(format!("_0{rest}"));
        }
    }
    None
}

fn parse_positional_topic_slot(name: &str) -> Option<usize> {
    let bytes = name.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'_' || !bytes[1].is_ascii_digit() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let digits = &name[1..i];
    while i < bytes.len() && bytes[i] == b'<' {
        i += 1;
    }
    if i != bytes.len() {
        return None;
    }
    digits.parse().ok().filter(|&n: &usize| n >= 1)
}

/// Saved bindings for `local $x` / `local @a` / `local %h` — restored on [`Scope::pop_frame`].
#[derive(Clone, Debug)]
enum LocalRestore {
    Scalar(String, StrykeValue),
    Array(String, Vec<StrykeValue>),
    Hash(String, IndexMap<String, StrykeValue>),
    /// `local $h{k}` — third is `None` if the key was absent before `local` (restore deletes the key).
    HashElement(String, String, Option<StrykeValue>),
    /// `local $a[i]` — restore previous slot value (see [`Scope::local_set_array_element`]).
    ArrayElement(String, i64, StrykeValue),
}

/// A single lexical scope frame.
/// Uses Vec instead of HashMap — for typical Perl code with < 10 variables per
/// scope, linear scan is faster than hashing due to cache locality and zero
/// hash overhead.
#[derive(Debug, Clone)]
struct Frame {
    scalars: Vec<(String, StrykeValue)>,
    arrays: Vec<(String, Vec<StrykeValue>)>,
    /// Subroutine (or bootstrap) `@_` — stored separately so call paths can move the arg
    /// [`Vec`] into the frame without an extra copy via [`Frame::arrays`].
    sub_underscore: Option<Vec<StrykeValue>>,
    hashes: Vec<(String, IndexMap<String, StrykeValue>)>,
    /// Slot-indexed scalars for O(1) access from compiled subroutines.
    /// Compiler assigns `my $x` declarations a u8 slot index; the VM accesses
    /// `scalar_slots[idx]` directly without name lookup or frame walking.
    scalar_slots: Vec<StrykeValue>,
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
    /// Arrays promoted to shared Arc-backed storage by `\@arr`.
    /// When a ref is taken, both the scope and the ref share the same Arc,
    /// so mutations through either path are visible. Re-declaration removes the entry.
    shared_arrays: Vec<(String, Arc<parking_lot::RwLock<Vec<StrykeValue>>>)>,
    /// Hashes promoted to shared Arc-backed storage by `\%hash`.
    shared_hashes: Vec<SharedHashEntry>,
    /// Thread-safe arrays from `mysync @a`
    atomic_arrays: Vec<(String, AtomicArray)>,
    /// Thread-safe hashes from `mysync %h`
    atomic_hashes: Vec<(String, AtomicHash)>,
    /// `defer { BLOCK }` closures to run when this frame is popped (LIFO order).
    defers: Vec<StrykeValue>,
    /// True after the first [`Scope::set_topic`] call in this frame. Subsequent
    /// calls (the next iter of the SAME `map`/`grep`/etc.) skip the chain shift
    /// so `_<` keeps pointing at the enclosing scope's topic instead of rolling
    /// to the previous iter's value. Reset by [`Self::clear_all_bindings`] when
    /// the frame is recycled. `set_closure_args` does NOT set this flag — sub
    /// entry shifts are real outer-topic captures, not iter re-entries.
    set_topic_called: bool,
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
        self.shared_arrays.clear();
        self.shared_hashes.clear();
        self.atomic_arrays.clear();
        self.defers.clear();
        self.atomic_hashes.clear();
        self.set_topic_called = false;
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
            shared_arrays: Vec::new(),
            shared_hashes: Vec::new(),
            typed_scalars: HashMap::new(),
            atomic_arrays: Vec::new(),
            atomic_hashes: Vec::new(),
            local_restores: Vec::new(),
            defers: Vec::new(),
            set_topic_called: false,
        }
    }

    #[inline]
    fn get_scalar(&self, name: &str) -> Option<&StrykeValue> {
        let name = strip_main_prefix(name).unwrap_or(name);
        if let Some(v) = self.get_scalar_from_slot(name) {
            return Some(v);
        }
        self.scalars.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    /// O(N) scan over slot names — only used by `get_scalar` fallback (name-based lookup);
    /// hot compiled paths use `get_scalar_slot(idx)` directly.
    #[inline]
    fn get_scalar_from_slot(&self, name: &str) -> Option<&StrykeValue> {
        let name = strip_main_prefix(name).unwrap_or(name);
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
        let name = strip_main_prefix(name).unwrap_or(name);
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
    fn set_scalar(&mut self, name: &str, val: StrykeValue) {
        let name = strip_main_prefix(name).unwrap_or(name);
        for (i, sn) in self.scalar_slot_names.iter().enumerate() {
            if let Some(ref n) = sn {
                if n == name {
                    if i < self.scalar_slots.len() {
                        // Write through CaptureCell so closures sharing this cell see the update
                        if let Some(r) = self.scalar_slots[i].as_capture_cell() {
                            *r.write() = val;
                        } else {
                            self.scalar_slots[i] = val;
                        }
                    }
                    return;
                }
            }
        }
        if let Some(entry) = self.scalars.iter_mut().find(|(k, _)| k == name) {
            // Write through CaptureCell so closures sharing this cell see the update
            if let Some(r) = entry.1.as_capture_cell() {
                *r.write() = val;
            } else {
                entry.1 = val;
            }
        } else {
            self.scalars.push((name.to_string(), val));
        }
    }

    /// Topic-slot variant: REPLACE the slot's value without writing
    /// through any existing CaptureCell. Used by `shift_slot_chain` /
    /// `declare_topic_slot` so binding the per-call arg to `$_`/`$_0`
    /// doesn't mutate a closure-captured cell shared with the outer
    /// scope. Without this, every call of a closure would clobber the
    /// caller's `$_` with the call's first arg, and `$_<` inside HOF
    /// blocks would alias the iter value rather than the surrounding
    /// scope's topic.
    #[inline]
    fn set_scalar_raw(&mut self, name: &str, val: StrykeValue) {
        let name = strip_main_prefix(name).unwrap_or(name);
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
    fn get_array(&self, name: &str) -> Option<&Vec<StrykeValue>> {
        let name = strip_main_prefix(name).unwrap_or(name);
        if name == "_" {
            if let Some(ref v) = self.sub_underscore {
                return Some(v);
            }
        }
        self.arrays.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    #[inline]
    fn has_array(&self, name: &str) -> bool {
        let name = strip_main_prefix(name).unwrap_or(name);
        if name == "_" && self.sub_underscore.is_some() {
            return true;
        }
        self.arrays.iter().any(|(k, _)| k == name)
            || self.shared_arrays.iter().any(|(k, _)| k == name)
    }

    #[inline]
    fn get_array_mut(&mut self, name: &str) -> Option<&mut Vec<StrykeValue>> {
        let name = strip_main_prefix(name).unwrap_or(name);
        if name == "_" {
            return self.sub_underscore.as_mut();
        }
        self.arrays
            .iter_mut()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }

    #[inline]
    fn set_array(&mut self, name: &str, val: Vec<StrykeValue>) {
        let name = strip_main_prefix(name).unwrap_or(name);
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
    fn get_hash(&self, name: &str) -> Option<&IndexMap<String, StrykeValue>> {
        let name = strip_main_prefix(name).unwrap_or(name);
        self.hashes.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    #[inline]
    fn has_hash(&self, name: &str) -> bool {
        let name = strip_main_prefix(name).unwrap_or(name);
        self.hashes.iter().any(|(k, _)| k == name)
            || self.shared_hashes.iter().any(|(k, _)| k == name)
    }

    #[inline]
    fn get_hash_mut(&mut self, name: &str) -> Option<&mut IndexMap<String, StrykeValue>> {
        let name = strip_main_prefix(name).unwrap_or(name);
        self.hashes
            .iter_mut()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }

    #[inline]
    fn set_hash(&mut self, name: &str, val: IndexMap<String, StrykeValue>) {
        let name = strip_main_prefix(name).unwrap_or(name);
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
    /// Frame depth at the moment `parallel_guard` was enabled. Frames at depth
    /// `>= parallel_guard_baseline` were pushed AFTER the guard turned on, so
    /// they are worker-local and writable; frames at depth `< baseline` are the
    /// captured outer scope and writes to those need `mysync`. Without this,
    /// any nested block (e.g. `for my $y (@x) { ... }` inside `pmap`) would
    /// push a new frame that makes `@x` look "captured" relative to the
    /// innermost frame, even though `@x` was declared INSIDE the worker's own
    /// block. Reset to 0 when the guard is disabled.
    parallel_guard_baseline: usize,
    /// Highest positional slot index ever activated by [`Self::set_closure_args`].
    /// Once a slot is touched, every subsequent frame shifts that slot's outer
    /// chain (`_N<`, `_N<<`, ...) even if the new frame has fewer args. This
    /// is what makes `_N<<<<` reach 4 frames up consistently — intermediate
    /// frames with no slot N still propagate the chain (with `undef` if they
    /// didn't bind that slot themselves).
    max_active_slot: usize,
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
            parallel_guard_baseline: 0,
            max_active_slot: 0,
        };
        s.frames.push(Frame::new());
        s
    }

    /// Enable [`Self::parallel_guard`] for parallel worker interpreters (pmap, fan, …).
    /// Snapshots the current frame depth as the baseline — any frames pushed
    /// after this call are worker-local and writable; frames already present
    /// are the captured outer scope.
    #[inline]
    pub fn set_parallel_guard(&mut self, enabled: bool) {
        self.parallel_guard = enabled;
        self.parallel_guard_baseline = if enabled { self.frames.len() } else { 0 };
    }

    #[inline]
    pub fn parallel_guard(&self) -> bool {
        self.parallel_guard
    }

    /// Names allowed to mutate freely inside a parallel block. Excludes plain
    /// package-qualified names (`Pkg::x`) — those used to be unconditionally skipped,
    /// but that was the source of plain `our $x` silently accumulating per-worker
    /// copies under `fan`/`pmap`/`pfor`. The atomicity check in
    /// [`Self::check_parallel_scalar_write`] now decides: `oursync` (Atomic-backed) is
    /// allowed, plain `our` (non-atomic) errors with a directive to declare `oursync`.
    #[inline]
    fn parallel_skip_special_name(_name: &str) -> bool {
        false
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

    fn check_parallel_scalar_write(&self, name: &str) -> Result<(), StrykeError> {
        if !self.parallel_guard || Self::parallel_skip_special_name(name) {
            return Ok(());
        }
        if Self::parallel_allowed_topic_scalar(name) {
            return Ok(());
        }
        if crate::special_vars::is_regex_match_scalar_name(name) {
            return Ok(());
        }
        // Worker-local frames are at depth >= baseline; any frame at that
        // depth or deeper is fine to write (it was created by THIS worker's
        // block, even if a nested for/if/sub pushed an inner frame after it).
        let baseline = self.parallel_guard_baseline;
        for (i, frame) in self.frames.iter().enumerate().rev() {
            if frame.has_scalar(name) {
                if let Some(v) = frame.get_scalar(name) {
                    if v.as_atomic_arc().is_some() {
                        return Ok(());
                    }
                }
                if i < baseline {
                    // Direct the user to the right shared-state primitive based on
                    // whether the captured variable is package-global (`our` →
                    // `oursync`) or lexical (`my` → `mysync`).
                    let directive = if name.contains("::") {
                        "declare `oursync` for shared package-global state"
                    } else {
                        "declare `mysync` for shared lexical state"
                    };
                    return Err(StrykeError::runtime(
                        format!(
                            "cannot assign to captured non-atomic variable `${}` in a parallel block — {}",
                            name, directive
                        ),
                        0,
                    ));
                }
                return Ok(());
            }
        }
        Err(StrykeError::runtime(
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
    pub fn get_scalar_slot(&self, slot: u8) -> StrykeValue {
        let idx = slot as usize;
        for frame in self.frames.iter().rev() {
            if idx < frame.scalar_slots.len() && frame.owns_scalar_slot_index(idx) {
                let val = &frame.scalar_slots[idx];
                // Transparently unwrap CaptureCell (closure-captured variable) — read through
                // the shared lock. User-created ScalarRef from `\expr` is NOT unwrapped.
                if let Some(arc) = val.as_capture_cell() {
                    return arc.read().clone();
                }
                return val.clone();
            }
        }
        StrykeValue::UNDEF
    }

    /// Write scalar to slot — innermost binding for `slot` wins (see [`Self::get_scalar_slot`]).
    #[inline]
    pub fn set_scalar_slot(&mut self, slot: u8, val: StrykeValue) {
        let idx = slot as usize;
        let len = self.frames.len();
        for i in (0..len).rev() {
            if idx < self.frames[i].scalar_slots.len() && self.frames[i].owns_scalar_slot_index(idx)
            {
                // Write through CaptureCell so closures sharing this cell see the update
                if let Some(r) = self.frames[i].scalar_slots[idx].as_capture_cell() {
                    *r.write() = val;
                } else {
                    self.frames[i].scalar_slots[idx] = val;
                }
                return;
            }
        }
        let top = self.frames.last_mut().unwrap();
        top.scalar_slots.resize(idx + 1, StrykeValue::UNDEF);
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
        val: StrykeValue,
        slot_name: Option<&str>,
    ) -> Result<(), StrykeError> {
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
                    let baseline = self.parallel_guard_baseline;
                    for (fi, frame) in self.frames.iter().enumerate().rev() {
                        if frame.has_scalar(name)
                            || (idx < frame.scalar_slots.len() && frame.owns_scalar_slot_index(idx))
                        {
                            if fi < baseline {
                                return Err(StrykeError::runtime(
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
    pub fn declare_scalar_slot(&mut self, slot: u8, val: StrykeValue, name: Option<&str>) {
        let idx = slot as usize;
        let frame = self.frames.last_mut().unwrap();
        if idx >= frame.scalar_slots.len() {
            frame.scalar_slots.resize(idx + 1, StrykeValue::UNDEF);
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
    /// Returns a [`StrykeValue::shallow_clone`] (Arc::clone) of the stored value
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
            frame.scalar_slots.resize(idx + 1, StrykeValue::UNDEF);
        }
        frame.scalar_slots[idx].try_concat_repeat_inplace(rhs, n)
    }

    /// Slow fallback for the fused string-append loop: clones the RHS into a new
    /// `StrykeValue::string` once and runs the existing `scalar_slot_concat_inplace`
    /// path `n` times. Used by `Op::ConcatConstSlotLoop` when the slot is aliased
    /// and the in-place fast path rejected the mutation.
    #[inline]
    pub fn scalar_slot_concat_repeat_slow(&mut self, slot: u8, rhs: &str, n: usize) {
        let pv = StrykeValue::string(rhs.to_owned());
        for _ in 0..n {
            let _ = self.scalar_slot_concat_inplace(slot, &pv);
        }
    }

    #[inline]
    pub fn scalar_slot_concat_inplace(&mut self, slot: u8, rhs: &StrykeValue) -> StrykeValue {
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
            frame.scalar_slots.resize(idx + 1, StrykeValue::UNDEF);
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
        let new_val = std::mem::replace(&mut frame.scalar_slots[idx], StrykeValue::UNDEF)
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
    pub fn local_set_scalar(&mut self, name: &str, val: StrykeValue) -> Result<(), StrykeError> {
        let old = self.get_scalar(name);
        if let Some(frame) = self.frames.last_mut() {
            frame
                .local_restores
                .push(LocalRestore::Scalar(name.to_string(), old));
        }
        self.set_scalar(name, val)
    }

    /// `local @name` — not valid for `mysync` arrays.
    pub fn local_set_array(&mut self, name: &str, val: Vec<StrykeValue>) -> Result<(), StrykeError> {
        if self.find_atomic_array(name).is_some() {
            return Err(StrykeError::runtime(
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
        val: IndexMap<String, StrykeValue>,
    ) -> Result<(), StrykeError> {
        if self.find_atomic_hash(name).is_some() {
            return Err(StrykeError::runtime(
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
        val: StrykeValue,
    ) -> Result<(), StrykeError> {
        if self.find_atomic_hash(name).is_some() {
            return Err(StrykeError::runtime(
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
        val: StrykeValue,
    ) -> Result<(), StrykeError> {
        if self.find_atomic_array(name).is_some() {
            return Err(StrykeError::runtime(
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
    pub fn declare_scalar(&mut self, name: &str, val: StrykeValue) {
        let _ = self.declare_scalar_frozen(name, val, false, None);
    }

    /// Declare a lexical scalar; `frozen` means no further assignment to this binding.
    /// `ty` is from `typed my $x : Int` — enforced on every assignment.
    pub fn declare_scalar_frozen(
        &mut self,
        name: &str,
        val: StrykeValue,
        frozen: bool,
        ty: Option<PerlTypeName>,
    ) -> Result<(), StrykeError> {
        canon_main!(name);
        if let Some(ref t) = ty {
            t.check_value(&val)
                .map_err(|msg| StrykeError::type_error(format!("`${}`: {}", name, msg), 0))?;
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
    pub fn get_scalar(&self, name: &str) -> StrykeValue {
        // `$main::X` aliases the bare `$X` (default-package equivalence).
        if let Some(rest) = strip_main_prefix(name) {
            return self.get_scalar(rest);
        }
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.get_scalar(name) {
                // Transparently unwrap Atomic — read through the lock
                if let Some(arc) = val.as_atomic_arc() {
                    return arc.lock().clone();
                }
                // Transparently unwrap CaptureCell (closure-captured variable) — read through the lock.
                // User-created ScalarRef from `\expr` is NOT unwrapped.
                if let Some(arc) = val.as_capture_cell() {
                    return arc.read().clone();
                }
                // Topic-slot chain (`_<`, `_<<`, `_N<+`, `_0<+`, …) reports
                // the value at the requested ascent level verbatim. No
                // fallback to current `_`: if no enclosing topic frame
                // populated that level, the chain entry is undef and
                // `_<` returns undef. This matches the documented
                // semantics of "walk N frames up the topic chain".
                return val.clone();
            }
        }
        StrykeValue::UNDEF
    }

    /// True for ANY topic-variant name: `_`, `_<+`, `_N`, `_N<+`. Matches
    /// the regex `^_[0-9]*<*$`. User assignments to these names are
    /// routed through the raw-write path so they stay frame-local rather
    /// than propagating up via closure-shared CaptureCells. Topic
    /// variants are framework-managed positional aliases — mutating them
    /// inside a block must NOT leak to the surrounding scope, otherwise
    /// per-iter HOF body mutations would chaotically mutate the caller's
    /// `$_`/`$_N` and break the lexical-outer chain invariant.
    #[inline]
    pub(crate) fn is_topic_variant_name(name: &str) -> bool {
        let bytes = name.as_bytes();
        if bytes.is_empty() || bytes[0] != b'_' {
            return false;
        }
        let mut i = 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        while i < bytes.len() && bytes[i] == b'<' {
            i += 1;
        }
        i == bytes.len()
    }

    /// True if any frame has a lexical scalar binding for `name` (`my` / `our` / assignment).
    #[inline]
    pub fn scalar_binding_exists(&self, name: &str) -> bool {
        canon_main!(name);
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

    /// Names of every array binding visible across frames (deduplicated).
    pub fn all_array_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for frame in &self.frames {
            for (name, _) in &frame.arrays {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            for (name, _) in &frame.shared_arrays {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            for (name, _) in &frame.atomic_arrays {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }

    /// Names of every hash binding visible across frames (deduplicated).
    pub fn all_hash_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for frame in &self.frames {
            for (name, _) in &frame.hashes {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            for (name, _) in &frame.shared_hashes {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            for (name, _) in &frame.atomic_hashes {
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
        canon_main!(name);
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
        if let Some(rest) = strip_main_prefix(name) {
            return self.hash_binding_exists(rest);
        }
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
    pub fn get_scalar_raw(&self, name: &str) -> StrykeValue {
        if let Some(rest) = strip_main_prefix(name) {
            return self.get_scalar_raw(rest);
        }
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.get_scalar(name) {
                return val.clone();
            }
        }
        StrykeValue::UNDEF
    }

    /// Atomically read-modify-write a scalar. Holds the Mutex lock for
    /// the entire cycle so `mysync` variables are race-free under `fan`/`pfor`.
    /// Returns the NEW value. Returns `Err` when the parallel guard rejects the
    /// write — `++`/`+=`/`-=` on a captured non-atomic outer-scope variable now
    /// fails fast just like plain `=` does, instead of silently dropping writes.
    pub fn atomic_mutate(
        &mut self,
        name: &str,
        f: impl FnOnce(&StrykeValue) -> StrykeValue,
    ) -> Result<StrykeValue, StrykeError> {
        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.get_scalar(name) {
                if let Some(arc) = v.as_atomic_arc() {
                    let mut guard = arc.lock();
                    let old = guard.clone();
                    let new_val = f(&guard);
                    *guard = new_val.clone();
                    crate::parallel_trace::emit_scalar_mutation(name, &old, &new_val);
                    return Ok(new_val);
                }
            }
        }
        // Non-atomic fallback. Route through `set_scalar` so the parallel guard
        // fires on `our` / `my` writes from inside `fan` / `pmap` / `pfor`.
        let old = self.get_scalar(name);
        let new_val = f(&old);
        self.set_scalar(name, new_val.clone())?;
        Ok(new_val)
    }

    /// Like [`Self::atomic_mutate`] but returns the OLD value (for postfix `$x++`).
    /// Returns `Err` for non-atomic captured-outer writes inside parallel blocks
    /// (same DESIGN-001 strict-error path as `atomic_mutate`).
    pub fn atomic_mutate_post(
        &mut self,
        name: &str,
        f: impl FnOnce(&StrykeValue) -> StrykeValue,
    ) -> Result<StrykeValue, StrykeError> {
        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.get_scalar(name) {
                if let Some(arc) = v.as_atomic_arc() {
                    let mut guard = arc.lock();
                    let old = guard.clone();
                    let new_val = f(&old);
                    *guard = new_val.clone();
                    crate::parallel_trace::emit_scalar_mutation(name, &old, &new_val);
                    return Ok(old);
                }
            }
        }
        // Non-atomic fallback — same parallel-guard semantics as `atomic_mutate`.
        let old = self.get_scalar(name);
        self.set_scalar(name, f(&old))?;
        Ok(old)
    }

    /// Append `rhs` to a scalar string in-place (no clone of the existing string).
    /// If the scalar is not yet a String, it is converted first.
    ///
    /// The binding and the returned [`StrykeValue`] share the same heap [`Arc`] via
    /// [`StrykeValue::shallow_clone`] on the store — a full [`Clone`] would deep-copy the
    /// entire `String` each time and make repeated `.=` O(N²) in the total length.
    #[inline]
    pub fn scalar_concat_inplace(
        &mut self,
        name: &str,
        rhs: &StrykeValue,
    ) -> Result<StrykeValue, StrykeError> {
        canon_main!(name);
        self.check_parallel_scalar_write(name)?;
        for frame in self.frames.iter_mut().rev() {
            if let Some(entry) = frame.scalars.iter_mut().find(|(k, _)| k == name) {
                // `mysync $x` stores `HeapObject::Atomic` — must mutate under the mutex, not
                // `into_string()` the wrapper (that would stringify the cell, not the payload).
                if let Some(atomic_arc) = entry.1.as_atomic_arc() {
                    let mut guard = atomic_arc.lock();
                    let inner = std::mem::replace(&mut *guard, StrykeValue::UNDEF);
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
                    std::mem::replace(&mut entry.1, StrykeValue::UNDEF).concat_append_owned(rhs);
                entry.1 = new_val.shallow_clone();
                return Ok(new_val);
            }
        }
        // Variable not found — create as new string
        let val = StrykeValue::UNDEF.concat_append_owned(rhs);
        self.frames[0].set_scalar(name, val.shallow_clone());
        Ok(val)
    }

    #[inline]
    pub fn set_scalar(&mut self, name: &str, val: StrykeValue) -> Result<(), StrykeError> {
        if let Some(rest) = strip_main_prefix(name) {
            return self.set_scalar(rest, val);
        }
        self.check_parallel_scalar_write(name)?;
        // Topic variants (`_`, `_<+`, `_N`, `_N<+`) are framework-managed
        // positional aliases. User assignments to them stay LOCAL to the
        // current frame — never propagate up through closure-shared
        // CaptureCells. Without this guard, a per-iter mutation inside a
        // HOF block would clobber the surrounding scope's `$_`/`$_N` and
        // break the "topic chain is lexical, not iterative" invariant.
        if Self::is_topic_variant_name(name) {
            if let Some(frame) = self.frames.last_mut() {
                frame.set_scalar_raw(name, val.clone());
                // Documented language invariant: `$_` and `$_0` are the same
                // variable (the topic ≡ positional slot 0). For deeper
                // chain levels they're also aliased: `$_<` ≡ `$_0<`, etc.
                // Mirror the write so reads of either name return the same
                // value. This fixes for-loop bindings where the foreach
                // compile path emits `Op::SetScalarPlain("_")` — without the
                // mirror, `$_0` stays undef.
                if let Some(alias) = topic_alias(name) {
                    frame.set_scalar_raw(&alias, val);
                }
            }
            return Ok(());
        }
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
                // If the existing value is CaptureCell (closure-captured variable), write through it
                if let Some(arc) = v.as_capture_cell() {
                    *arc.write() = val;
                    return Ok(());
                }
            }
            if frame.has_scalar(name) {
                if let Some(ty) = frame.typed_scalars.get(name) {
                    ty.check_value(&val)
                        .map_err(|msg| StrykeError::type_error(format!("`${}`: {}", name, msg), 0))?;
                }
                frame.set_scalar(name, val);
                return Ok(());
            }
        }
        self.frames[0].set_scalar(name, val);
        Ok(())
    }

    /// Topic-slot key for slot N at chain level L (0 = current, 1..5 = outer
    /// frames). Slot 0's canonical form is bare `_` / `_<` / `_<<` / ... so
    /// direct `$_ = …` assignments and existing `$_<` consumers see the
    /// expected key. The `_0<+` form is the alias, written in lockstep by
    /// `declare_topic_slot`. For slot N ≥ 1 the canonical key is `_N<+`.
    #[inline]
    fn topic_slot_key(slot: usize, level: usize) -> String {
        debug_assert!(level <= 5);
        if slot == 0 {
            if level == 0 {
                "_".to_string()
            } else {
                format!("_{}", "<".repeat(level))
            }
        } else if level == 0 {
            format!("_{}", slot)
        } else {
            format!("_{}{}", slot, "<".repeat(level))
        }
    }

    /// Mirror key for slot 0 (`_0` / `_0<` / `_0<<` / ...) — the explicit-zero
    /// alias of the bare form. Returns `None` for slot N ≥ 1 (no alias).
    #[inline]
    fn topic_slot_alias_key(slot: usize, level: usize) -> Option<String> {
        if slot != 0 {
            return None;
        }
        Some(if level == 0 {
            "_0".to_string()
        } else {
            format!("_0{}", "<".repeat(level))
        })
    }

    /// Write a value at slot N, level L. For slot 0 also writes the bare-`_`
    /// mirror at the same level so `_<<<<` ≡ `_0<<<<` resolve to the same
    /// scalar. This is what makes the world-first multi-level implicit-param
    /// matrix work — see `lexer.rs` for the lexing side and the user-visible
    /// rule "_< ≡ $_< ≡ _0< ≡ $_0<".
    #[inline]
    fn declare_topic_slot(&mut self, slot: usize, level: usize, val: StrykeValue) {
        // Use `set_scalar_raw` (frame method) so binding the topic does
        // NOT write through a closure-captured CaptureCell. Without this,
        // every per-iter HOF block call would clobber the surrounding
        // scope's `$_` with the iter value via the shared cell — making
        // `$_<` alias the iter value rather than the lexical outer's
        // topic. The chain semantics require frame-isolated writes.
        if let Some(frame) = self.frames.last_mut() {
            let key = Self::topic_slot_key(slot, level);
            frame.set_scalar_raw(&key, val.clone());
            if let Some(alias) = Self::topic_slot_alias_key(slot, level) {
                frame.set_scalar_raw(&alias, val);
            }
        }
    }

    /// Set the topic variable `$_` and its numeric alias `$_0` together.
    /// Use this for **block-form** closures (`map { ... }`, `grep { ... }`,
    /// `sort { ... }`, threaded `~> @arr map { ... }`, `fi { ... }`,
    /// etc.) so `$_`, `$_0`, and the outer-topic chain (`$_<`, `$_<<`, …)
    /// all behave correctly. EXPR-form HOFs (`grep EXPR, LIST`,
    /// `map EXPR, LIST`, `reject EXPR, LIST`, `grepv EXPR, LIST`, etc. —
    /// anything with no `{}`) MUST use [`Self::set_topic_local`] instead;
    /// EXPR position is in the same lexical scope as the surrounding code,
    /// so there is no scope/frame boundary and the chain MUST NOT shift.
    /// User-facing rule: **`{}` triggers the shift; no `{}` means no shift**.
    ///
    /// This declares `$_`/`$_0` in the current scope (not global), suitable
    /// for sub calls.
    ///
    /// Shifts the outer-topic chain (`$_<`, `$_<<`, `$_<<<`, `$_<<<<`,
    /// `$_<<<<<`) on the FIRST call in a given frame so nested blocks can
    /// peek up to 5 frames out. Subsequent calls in the same frame (the next iteration of the
    /// SAME `map`/`grep`/etc.) only refresh `_` and `_0` so `_<` keeps
    /// pointing at the **enclosing scope's** topic, not the previous
    /// iteration's value. This is the "frame-based" reading: from inside a
    /// nested closure, `_<` means "the topic of the closure that contains
    /// me" (which is constant across my iterations), not "the topic the
    /// previous iter set" (which would roll). All previously-activated
    /// positional slots shift in lockstep on the first call.
    #[inline]
    pub fn set_topic(&mut self, val: StrykeValue) {
        // Iteration re-entry detection: the per-frame `set_topic_called` flag
        // is true if a previous `set_topic` already shifted in this frame
        // (i.e. we're in the next iter of the SAME loop). Refresh `_` / `_0`
        // only — preserve the chain so the enclosing scope's topic stays
        // visible. `set_closure_args` does NOT set this flag, so a sub call
        // followed by `map { ... }` still gets a real shift on the FIRST
        // map iter (the chain becomes "the sub's args at level 1").
        let already_shifted = self
            .frames
            .last()
            .map(|f| f.set_topic_called)
            .unwrap_or(false);
        if already_shifted {
            self.declare_topic_slot(0, 0, val);
            for slot in 1..=self.max_active_slot {
                self.declare_topic_slot(slot, 0, StrykeValue::UNDEF);
            }
            return;
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.set_topic_called = true;
        }
        self.shift_slot_chain(0, val);
        for slot in 1..=self.max_active_slot {
            self.shift_slot_chain(slot, StrykeValue::UNDEF);
        }
    }

    /// EXPR-form variant: rebinds `$_` / `$_0` to `val` for the current
    /// iteration WITHOUT shifting any chain or zeroing slot 1+ aliases.
    /// Used by `grep EXPR, LIST` / `map EXPR, LIST` and the streaming
    /// equivalents — the EXPR is evaluated in the lexical scope of the
    /// surrounding code, with no block boundary, so the topic chain
    /// shouldn't roll. Crucially this preserves `_1`, `_2`, ..., `_N`
    /// from the caller fn so patterns like `grep _1, @$_` work without
    /// chain-ascent.
    #[inline]
    pub fn set_topic_local(&mut self, val: StrykeValue) {
        self.declare_topic_slot(0, 0, val);
    }

    /// Set numeric closure argument aliases `$_0`, `$_1`, `$_2`, ... for all
    /// args. Also sets `$_` to the first argument (if any) and shifts the
    /// outer-topic chain on EVERY positional slot ever activated, so a 5-deep
    /// nested block can read `_2<<<<<` to reach the third positional argument
    /// from 5 frames up. (Stryke-only — no other language has nested implicit
    /// positionals.)
    ///
    /// The shift fires on slots `0..=max(args.len()-1, max_active_slot)`. A
    /// frame that binds fewer args than the high-water mark still rotates the
    /// older slots (the new "current" for an unbound slot is `undef`, so old
    /// values march through `_N<<<<` and eventually fall off the end).
    #[inline]
    pub fn set_closure_args(&mut self, args: &[StrykeValue]) {
        let n = args.len();
        if n == 0 {
            return;
        }
        let high = n.saturating_sub(1).max(self.max_active_slot);
        for slot in 0..=high {
            let val = args.get(slot).cloned().unwrap_or(StrykeValue::UNDEF);
            self.shift_slot_chain(slot, val);
        }
        if n > 0 && n - 1 > self.max_active_slot {
            self.max_active_slot = n - 1;
        }
    }

    /// Shift slot N's outer-topic chain by one level and install `val` as the
    /// new current value. Internal helper for [`set_topic`] / [`set_closure_args`].
    ///
    /// Writes ALL 6 levels unconditionally — even when the previous values
    /// are `undef` — so chain semantics stay intact across frames that don't
    /// bind every active slot. Without the unconditional write, a stale value
    /// at `_N<` would persist across multiple "no slot N here" frames and
    /// `_N<<<<<` would never reach 5 frames back.
    #[inline]
    fn shift_slot_chain(&mut self, slot: usize, val: StrykeValue) {
        let l4 = self.get_scalar(&Self::topic_slot_key(slot, 4));
        let l3 = self.get_scalar(&Self::topic_slot_key(slot, 3));
        let l2 = self.get_scalar(&Self::topic_slot_key(slot, 2));
        let l1 = self.get_scalar(&Self::topic_slot_key(slot, 1));
        let cur = self.get_scalar(&Self::topic_slot_key(slot, 0));

        self.declare_topic_slot(slot, 0, val);
        self.declare_topic_slot(slot, 1, cur);
        self.declare_topic_slot(slot, 2, l1);
        self.declare_topic_slot(slot, 3, l2);
        self.declare_topic_slot(slot, 4, l3);
        self.declare_topic_slot(slot, 5, l4);
    }

    /// Set the canonical sort/reduce binding pair: `$a` / `$b` (Perl-isms) AND
    /// `$_0` / `$_1` (the stryke positional aliases — preferred under
    /// `--no-interop` because the `$a`/`$b` pair is inconsistent — there is
    /// no `$c`). The bareword forms `_0` / `_1` resolve to `$_0` / `$_1` via
    /// the parser, so blocks like `sort { _0 <=> _1 }` and `reduce { _0 + _1 }`
    /// just work. Use this helper anywhere the legacy code wrote two adjacent
    /// `set_scalar("a", …); set_scalar("b", …)` lines.
    #[inline]
    pub fn set_sort_pair(&mut self, a: StrykeValue, b: StrykeValue) {
        let _ = self.set_scalar("a", a.clone());
        let _ = self.set_scalar("b", b.clone());
        let _ = self.set_scalar("_0", a.clone());
        let _ = self.set_scalar("_1", b);
        // Bind `$_` to slot 0 too — per the four-way aliasing rule
        // (`_`, `$_`, `_0`, `$_0` are all equivalent for slot 0),
        // bare `_` inside `sort { _ <=> _1 }` must resolve to slot 0.
        // Without this, `_` falls through to whatever the outer scope's
        // topic was and the sort silently produces garbage order.
        let _ = self.set_scalar("_", a);
    }

    /// Save the entire topic slot 0 chain (`$_`, `$_<`, `$_<<`, ...) so it can
    /// be restored after a block that corrupts it (like `sort { ... }`).
    /// Returns a 6-element array [level0..level5].
    #[inline]
    pub fn save_topic_chain(&self) -> [StrykeValue; 6] {
        [
            self.get_scalar(&Self::topic_slot_key(0, 0)),
            self.get_scalar(&Self::topic_slot_key(0, 1)),
            self.get_scalar(&Self::topic_slot_key(0, 2)),
            self.get_scalar(&Self::topic_slot_key(0, 3)),
            self.get_scalar(&Self::topic_slot_key(0, 4)),
            self.get_scalar(&Self::topic_slot_key(0, 5)),
        ]
    }

    /// Restore the topic slot 0 chain from a previous [`save_topic_chain`] call.
    #[inline]
    pub fn restore_topic_chain(&mut self, saved: [StrykeValue; 6]) {
        for (level, val) in saved.into_iter().enumerate() {
            self.declare_topic_slot(0, level, val);
        }
    }

    /// Register a `defer { BLOCK }` closure to run when this scope exits.
    #[inline]
    pub fn push_defer(&mut self, coderef: StrykeValue) {
        if let Some(frame) = self.frames.last_mut() {
            frame.defers.push(coderef);
        }
    }

    /// Take all deferred blocks from the current frame (for execution on scope exit).
    /// Returns them in reverse order (LIFO - last defer runs first).
    #[inline]
    pub fn take_defers(&mut self) -> Vec<StrykeValue> {
        if let Some(frame) = self.frames.last_mut() {
            let mut defers = std::mem::take(&mut frame.defers);
            defers.reverse();
            defers
        } else {
            Vec::new()
        }
    }

    // ── Atomic array/hash declarations ──

    pub fn declare_atomic_array(&mut self, name: &str, val: Vec<StrykeValue>) {
        canon_main!(name);
        if let Some(frame) = self.frames.last_mut() {
            frame
                .atomic_arrays
                .push((name.to_string(), AtomicArray(Arc::new(Mutex::new(val)))));
        }
    }

    pub fn declare_atomic_hash(&mut self, name: &str, val: IndexMap<String, StrykeValue>) {
        canon_main!(name);
        if let Some(frame) = self.frames.last_mut() {
            frame
                .atomic_hashes
                .push((name.to_string(), AtomicHash(Arc::new(Mutex::new(val)))));
        }
    }

    /// Find an atomic array by name (returns the Arc for sharing).
    fn find_atomic_array(&self, name: &str) -> Option<&AtomicArray> {
        let name = strip_main_prefix(name).unwrap_or(name);
        for frame in self.frames.iter().rev() {
            if let Some(aa) = frame.atomic_arrays.iter().find(|(k, _)| k == name) {
                return Some(&aa.1);
            }
        }
        None
    }

    /// Find an atomic hash by name.
    fn find_atomic_hash(&self, name: &str) -> Option<&AtomicHash> {
        let name = strip_main_prefix(name).unwrap_or(name);
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
    pub fn take_sub_underscore(&mut self) -> Option<Vec<StrykeValue>> {
        self.frames.last_mut()?.sub_underscore.take()
    }

    pub fn declare_array(&mut self, name: &str, val: Vec<StrykeValue>) {
        self.declare_array_frozen(name, val, false);
    }

    pub fn declare_array_frozen(&mut self, name: &str, val: Vec<StrykeValue>, frozen: bool) {
        canon_main!(name);
        // Package stash names (`Foo::BAR`) live in the outermost frame so nested blocks/subs
        // cannot shadow `@C::ISA` with an empty array (breaks inheritance / SUPER).
        let idx = if name.contains("::") {
            0
        } else {
            self.frames.len().saturating_sub(1)
        };
        if let Some(frame) = self.frames.get_mut(idx) {
            // Remove any existing shared Arc — re-declaration disconnects old refs.
            frame.shared_arrays.retain(|(k, _)| k != name);
            frame.set_array(name, val);
            if frozen {
                frame.frozen_arrays.insert(name.to_string());
            } else {
                // Redeclaring as non-frozen should unfreeze if previously frozen
                frame.frozen_arrays.remove(name);
            }
        }
    }

    pub fn get_array(&self, name: &str) -> Vec<StrykeValue> {
        // `@main::X` aliases the bare `@X` because `main` is the default
        // package — `@main::INC` ≡ `@INC`, `@main::ARGV` ≡ `@ARGV`,
        // `@main::fpath` ≡ `@fpath`. The bare form is what's actually
        // stored, so the qualified form has to short-circuit through
        // the unqualified lookup.
        if let Some(rest) = strip_main_prefix(name) {
            return self.get_array(rest);
        }
        // Check atomic arrays first
        if let Some(aa) = self.find_atomic_array(name) {
            return aa.0.lock().clone();
        }
        // Check shared (Arc-backed) arrays
        if let Some(arc) = self.find_shared_array(name) {
            return arc.read().clone();
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
    /// Used to pass `@_` to [`crate::list_builtins::native_dispatch`] without cloning the vector.
    #[inline]
    pub fn get_array_borrow(&self, name: &str) -> Option<&[StrykeValue]> {
        if let Some(rest) = strip_main_prefix(name) {
            return self.get_array_borrow(rest);
        }
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

    fn check_parallel_array_write(&self, name: &str) -> Result<(), StrykeError> {
        if !self.parallel_guard
            || Self::parallel_skip_special_name(name)
            || Self::parallel_allowed_internal_array(name)
        {
            return Ok(());
        }
        // Worker-local frames are at depth >= baseline.
        let baseline = self.parallel_guard_baseline;
        match self.resolve_array_frame_idx(name) {
            None => Err(StrykeError::runtime(
                format!(
                    "cannot modify undeclared array `@{}` in a parallel block",
                    name
                ),
                0,
            )),
            Some(idx) if idx < baseline => Err(StrykeError::runtime(
                format!(
                    "cannot modify captured non-mysync array `@{}` in a parallel block",
                    name
                ),
                0,
            )),
            Some(_) => Ok(()),
        }
    }

    /// Resolve an [`ArrayBindingRef`] or [`HashBindingRef`] to an Arc-backed
    /// snapshot so the value survives scope pop. Called when a value is stored
    /// as an *element* inside a container (array/hash) — NOT for scalar assignment,
    /// where binding refs must stay live for aliasing.
    #[inline]
    pub fn resolve_container_binding_ref(&self, val: StrykeValue) -> StrykeValue {
        if let Some(name) = val.as_array_binding_name() {
            let data = self.get_array(&name);
            return StrykeValue::array_ref(Arc::new(parking_lot::RwLock::new(data)));
        }
        if let Some(name) = val.as_hash_binding_name() {
            let data = self.get_hash(&name);
            return StrykeValue::hash_ref(Arc::new(parking_lot::RwLock::new(data)));
        }
        val
    }

    /// Promote `@name` to shared Arc-backed storage and return an [`ArrayRef`] that
    /// shares the same `Arc`. Both the scope binding and the returned ref point to
    /// the same data, so mutations through either path are visible.
    pub fn promote_array_to_shared(
        &mut self,
        name: &str,
    ) -> Arc<parking_lot::RwLock<Vec<StrykeValue>>> {
        // Atomic (mysync) arrays: snapshot current data into a separate Arc.
        // Can't share the Mutex-backed storage directly.
        if let Some(aa) = self.find_atomic_array(name) {
            let data = aa.0.lock().clone();
            return Arc::new(parking_lot::RwLock::new(data));
        }
        // Already promoted? Return the existing Arc.
        let idx = self.resolve_array_frame_idx(name).unwrap_or_default();
        let frame = &mut self.frames[idx];
        if let Some(entry) = frame.shared_arrays.iter().find(|(k, _)| k == name) {
            return Arc::clone(&entry.1);
        }
        // Take data from frame.arrays, create Arc, store in shared_arrays.
        let data = if let Some(pos) = frame.arrays.iter().position(|(k, _)| k == name) {
            frame.arrays.swap_remove(pos).1
        } else if name == "_" {
            frame.sub_underscore.take().unwrap_or_default()
        } else {
            Vec::new()
        };
        let arc = Arc::new(parking_lot::RwLock::new(data));
        frame
            .shared_arrays
            .push((name.to_string(), Arc::clone(&arc)));
        arc
    }

    /// Promote `%name` to shared Arc-backed storage and return a [`HashRef`] that
    /// shares the same `Arc`.
    pub fn promote_hash_to_shared(
        &mut self,
        name: &str,
    ) -> Arc<parking_lot::RwLock<IndexMap<String, StrykeValue>>> {
        let idx = self.resolve_hash_frame_idx(name).unwrap_or_default();
        let frame = &mut self.frames[idx];
        if let Some(entry) = frame.shared_hashes.iter().find(|(k, _)| k == name) {
            return Arc::clone(&entry.1);
        }
        let data = if let Some(pos) = frame.hashes.iter().position(|(k, _)| k == name) {
            frame.hashes.swap_remove(pos).1
        } else {
            IndexMap::new()
        };
        let arc = Arc::new(parking_lot::RwLock::new(data));
        frame
            .shared_hashes
            .push((name.to_string(), Arc::clone(&arc)));
        arc
    }

    /// Find the shared Arc for `@name`, if any.
    fn find_shared_array(&self, name: &str) -> Option<Arc<parking_lot::RwLock<Vec<StrykeValue>>>> {
        let name = strip_main_prefix(name).unwrap_or(name);
        for frame in self.frames.iter().rev() {
            if let Some(entry) = frame.shared_arrays.iter().find(|(k, _)| k == name) {
                return Some(Arc::clone(&entry.1));
            }
            // If this frame has the plain array, stop — it shadows outer shared ones.
            if frame.arrays.iter().any(|(k, _)| k == name) {
                return None;
            }
        }
        None
    }

    /// Find the shared Arc for `%name`, if any.
    fn find_shared_hash(
        &self,
        name: &str,
    ) -> Option<Arc<parking_lot::RwLock<IndexMap<String, StrykeValue>>>> {
        let name = strip_main_prefix(name).unwrap_or(name);
        for frame in self.frames.iter().rev() {
            if let Some(entry) = frame.shared_hashes.iter().find(|(k, _)| k == name) {
                return Some(Arc::clone(&entry.1));
            }
            if frame.hashes.iter().any(|(k, _)| k == name) {
                return None;
            }
        }
        None
    }

    pub fn get_array_mut(&mut self, name: &str) -> Result<&mut Vec<StrykeValue>, StrykeError> {
        // Note: can't return &mut into a Mutex. Callers needing atomic array
        // mutation should use atomic_array_mutate instead. For non-atomic arrays:
        if self.find_atomic_array(name).is_some() {
            return Err(StrykeError::runtime(
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
    pub fn push_to_array(&mut self, name: &str, val: StrykeValue) -> Result<(), StrykeError> {
        let val = self.resolve_container_binding_ref(val);
        if let Some(aa) = self.find_atomic_array(name) {
            aa.0.lock().push(val);
            return Ok(());
        }
        if let Some(arc) = self.find_shared_array(name) {
            arc.write().push(val);
            return Ok(());
        }
        self.get_array_mut(name)?.push(val);
        Ok(())
    }

    /// Bulk `push @name, start..end-1` for the fused counted-loop superinstruction:
    /// reserves the `Vec` once, then pushes `StrykeValue::integer(i)` for `i in start..end`
    /// in a tight Rust loop. Atomic arrays take a single `lock().push()` burst.
    pub fn push_int_range_to_array(
        &mut self,
        name: &str,
        start: i64,
        end: i64,
    ) -> Result<(), StrykeError> {
        if end <= start {
            return Ok(());
        }
        let count = (end - start) as usize;
        if let Some(aa) = self.find_atomic_array(name) {
            let mut g = aa.0.lock();
            g.reserve(count);
            for i in start..end {
                g.push(StrykeValue::integer(i));
            }
            return Ok(());
        }
        let arr = self.get_array_mut(name)?;
        arr.reserve(count);
        for i in start..end {
            arr.push(StrykeValue::integer(i));
        }
        Ok(())
    }

    /// Pop from array — works for regular, shared, and atomic arrays.
    pub fn pop_from_array(&mut self, name: &str) -> Result<StrykeValue, StrykeError> {
        if let Some(aa) = self.find_atomic_array(name) {
            return Ok(aa.0.lock().pop().unwrap_or(StrykeValue::UNDEF));
        }
        if let Some(arc) = self.find_shared_array(name) {
            return Ok(arc.write().pop().unwrap_or(StrykeValue::UNDEF));
        }
        Ok(self
            .get_array_mut(name)?
            .pop()
            .unwrap_or(StrykeValue::UNDEF))
    }

    /// Shift from array — works for regular, shared, and atomic arrays.
    pub fn shift_from_array(&mut self, name: &str) -> Result<StrykeValue, StrykeError> {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut guard = aa.0.lock();
            return Ok(if guard.is_empty() {
                StrykeValue::UNDEF
            } else {
                guard.remove(0)
            });
        }
        if let Some(arc) = self.find_shared_array(name) {
            let mut arr = arc.write();
            return Ok(if arr.is_empty() {
                StrykeValue::UNDEF
            } else {
                arr.remove(0)
            });
        }
        let arr = self.get_array_mut(name)?;
        Ok(if arr.is_empty() {
            StrykeValue::UNDEF
        } else {
            arr.remove(0)
        })
    }

    /// Splice in place — works for regular, shared, and atomic arrays.
    /// `off..end` must already be clamped (use `splice_compute_range` to compute).
    /// Returns the removed elements.
    pub fn splice_in_place(
        &mut self,
        name: &str,
        off: usize,
        end: usize,
        rep_vals: Vec<StrykeValue>,
    ) -> Result<Vec<StrykeValue>, StrykeError> {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut g = aa.0.lock();
            let removed: Vec<StrykeValue> = g.drain(off..end).collect();
            for (i, v) in rep_vals.into_iter().enumerate() {
                g.insert(off + i, v);
            }
            return Ok(removed);
        }
        if let Some(arc) = self.find_shared_array(name) {
            let mut g = arc.write();
            let removed: Vec<StrykeValue> = g.drain(off..end).collect();
            for (i, v) in rep_vals.into_iter().enumerate() {
                g.insert(off + i, v);
            }
            return Ok(removed);
        }
        let arr = self.get_array_mut(name)?;
        let removed: Vec<StrykeValue> = arr.drain(off..end).collect();
        for (i, v) in rep_vals.into_iter().enumerate() {
            arr.insert(off + i, v);
        }
        Ok(removed)
    }

    /// Get array length — works for both regular and atomic arrays.
    pub fn array_len(&self, name: &str) -> usize {
        canon_main!(name);
        if let Some(aa) = self.find_atomic_array(name) {
            return aa.0.lock().len();
        }
        if let Some(arc) = self.find_shared_array(name) {
            return arc.read().len();
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

    pub fn set_array(&mut self, name: &str, val: Vec<StrykeValue>) -> Result<(), StrykeError> {
        if let Some(aa) = self.find_atomic_array(name) {
            *aa.0.lock() = val;
            return Ok(());
        }
        if let Some(arc) = self.find_shared_array(name) {
            *arc.write() = val;
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
    pub fn get_array_element(&self, name: &str, index: i64) -> StrykeValue {
        canon_main!(name);
        if let Some(aa) = self.find_atomic_array(name) {
            let arr = aa.0.lock();
            let idx = if index < 0 {
                (arr.len() as i64 + index) as usize
            } else {
                index as usize
            };
            return arr.get(idx).cloned().unwrap_or(StrykeValue::UNDEF);
        }
        if let Some(arc) = self.find_shared_array(name) {
            let arr = arc.read();
            let idx = if index < 0 {
                (arr.len() as i64 + index) as usize
            } else {
                index as usize
            };
            return arr.get(idx).cloned().unwrap_or(StrykeValue::UNDEF);
        }
        for frame in self.frames.iter().rev() {
            if let Some(arr) = frame.get_array(name) {
                let idx = if index < 0 {
                    (arr.len() as i64 + index) as usize
                } else {
                    index as usize
                };
                return arr.get(idx).cloned().unwrap_or(StrykeValue::UNDEF);
            }
        }
        StrykeValue::UNDEF
    }

    pub fn set_array_element(
        &mut self,
        name: &str,
        index: i64,
        val: StrykeValue,
    ) -> Result<(), StrykeError> {
        let val = self.resolve_container_binding_ref(val);
        if let Some(aa) = self.find_atomic_array(name) {
            let mut arr = aa.0.lock();
            let idx = if index < 0 {
                (arr.len() as i64 + index).max(0) as usize
            } else {
                index as usize
            };
            if idx >= arr.len() {
                arr.resize(idx + 1, StrykeValue::UNDEF);
            }
            arr[idx] = val;
            return Ok(());
        }
        if let Some(arc) = self.find_shared_array(name) {
            let mut arr = arc.write();
            let idx = if index < 0 {
                (arr.len() as i64 + index).max(0) as usize
            } else {
                index as usize
            };
            if idx >= arr.len() {
                arr.resize(idx + 1, StrykeValue::UNDEF);
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
            arr.resize(idx + 1, StrykeValue::UNDEF);
        }
        arr[idx] = val;
        Ok(())
    }

    /// Perl `exists $a[$i]` — true when the slot index is within the current array length.
    pub fn exists_array_element(&self, name: &str, index: i64) -> bool {
        canon_main!(name);
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
    pub fn delete_array_element(
        &mut self,
        name: &str,
        index: i64,
    ) -> Result<StrykeValue, StrykeError> {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut arr = aa.0.lock();
            let idx = if index < 0 {
                (arr.len() as i64 + index) as usize
            } else {
                index as usize
            };
            if idx >= arr.len() {
                return Ok(StrykeValue::UNDEF);
            }
            let old = arr.get(idx).cloned().unwrap_or(StrykeValue::UNDEF);
            arr[idx] = StrykeValue::UNDEF;
            return Ok(old);
        }
        let arr = self.get_array_mut(name)?;
        let idx = if index < 0 {
            (arr.len() as i64 + index) as usize
        } else {
            index as usize
        };
        if idx >= arr.len() {
            return Ok(StrykeValue::UNDEF);
        }
        let old = arr.get(idx).cloned().unwrap_or(StrykeValue::UNDEF);
        arr[idx] = StrykeValue::UNDEF;
        Ok(old)
    }

    // ── Hashes ──

    #[inline]
    pub fn declare_hash(&mut self, name: &str, val: IndexMap<String, StrykeValue>) {
        self.declare_hash_frozen(name, val, false);
    }

    pub fn declare_hash_frozen(
        &mut self,
        name: &str,
        val: IndexMap<String, StrykeValue>,
        frozen: bool,
    ) {
        canon_main!(name);
        if let Some(frame) = self.frames.last_mut() {
            // Remove any existing shared Arc — re-declaration disconnects old refs.
            frame.shared_hashes.retain(|(k, _)| k != name);
            frame.set_hash(name, val);
            if frozen {
                frame.frozen_hashes.insert(name.to_string());
            }
        }
    }

    /// Declare a hash in the bottom (global) frame, not the current lexical frame.
    pub fn declare_hash_global(&mut self, name: &str, val: IndexMap<String, StrykeValue>) {
        canon_main!(name);
        if let Some(frame) = self.frames.first_mut() {
            frame.set_hash(name, val);
        }
    }

    /// Declare a frozen hash in the bottom (global) frame — prevents user reassignment.
    pub fn declare_hash_global_frozen(&mut self, name: &str, val: IndexMap<String, StrykeValue>) {
        canon_main!(name);
        if let Some(frame) = self.frames.first_mut() {
            frame.set_hash(name, val);
            frame.frozen_hashes.insert(name.to_string());
        }
    }

    /// Returns `true` if a lexical (non-bottom) frame declares `%name`.
    pub fn has_lexical_hash(&self, name: &str) -> bool {
        canon_main!(name);
        self.frames.iter().skip(1).any(|f| f.has_hash(name))
    }

    /// Returns `true` if ANY frame (including global) declares `%name`.
    pub fn any_frame_has_hash(&self, name: &str) -> bool {
        canon_main!(name);
        self.frames.iter().any(|f| f.has_hash(name))
    }

    pub fn get_hash(&self, name: &str) -> IndexMap<String, StrykeValue> {
        // `%main::X` aliases the bare `%X` (default-package equivalence).
        if let Some(rest) = strip_main_prefix(name) {
            return self.get_hash(rest);
        }
        if let Some(ah) = self.find_atomic_hash(name) {
            return ah.0.lock().clone();
        }
        if let Some(arc) = self.find_shared_hash(name) {
            return arc.read().clone();
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

    fn check_parallel_hash_write(&self, name: &str) -> Result<(), StrykeError> {
        if !self.parallel_guard
            || Self::parallel_skip_special_name(name)
            || Self::parallel_allowed_internal_hash(name)
        {
            return Ok(());
        }
        // Worker-local frames are at depth >= baseline.
        let baseline = self.parallel_guard_baseline;
        match self.resolve_hash_frame_idx(name) {
            None => Err(StrykeError::runtime(
                format!(
                    "cannot modify undeclared hash `%{}` in a parallel block",
                    name
                ),
                0,
            )),
            Some(idx) if idx < baseline => Err(StrykeError::runtime(
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
    ) -> Result<&mut IndexMap<String, StrykeValue>, StrykeError> {
        if self.find_atomic_hash(name).is_some() {
            return Err(StrykeError::runtime(
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
        val: IndexMap<String, StrykeValue>,
    ) -> Result<(), StrykeError> {
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
    pub fn get_hash_element(&self, name: &str, key: &str) -> StrykeValue {
        canon_main!(name);
        if let Some(ah) = self.find_atomic_hash(name) {
            return ah.0.lock().get(key).cloned().unwrap_or(StrykeValue::UNDEF);
        }
        if let Some(arc) = self.find_shared_hash(name) {
            return arc.read().get(key).cloned().unwrap_or(StrykeValue::UNDEF);
        }
        for frame in self.frames.iter().rev() {
            if let Some(hash) = frame.get_hash(name) {
                return hash.get(key).cloned().unwrap_or(StrykeValue::UNDEF);
            }
        }
        StrykeValue::UNDEF
    }

    /// Atomically read-modify-write a hash element. For atomic hashes, holds
    /// the Mutex for the full cycle. Returns the new value.
    pub fn atomic_hash_mutate(
        &mut self,
        name: &str,
        key: &str,
        f: impl FnOnce(&StrykeValue) -> StrykeValue,
    ) -> Result<StrykeValue, StrykeError> {
        if let Some(ah) = self.find_atomic_hash(name) {
            let mut guard = ah.0.lock();
            let old = guard.get(key).cloned().unwrap_or(StrykeValue::UNDEF);
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
        f: impl FnOnce(&StrykeValue) -> StrykeValue,
    ) -> Result<StrykeValue, StrykeError> {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut guard = aa.0.lock();
            let idx = if index < 0 {
                (guard.len() as i64 + index).max(0) as usize
            } else {
                index as usize
            };
            if idx >= guard.len() {
                guard.resize(idx + 1, StrykeValue::UNDEF);
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
        val: StrykeValue,
    ) -> Result<(), StrykeError> {
        let val = self.resolve_container_binding_ref(val);
        // `$SIG{INT} = \&h` — lazily install the matching signal hook. Until Perl code touches
        // `%SIG`, the POSIX default stays in place so Ctrl-C terminates immediately.
        if name == "SIG" {
            crate::perl_signal::install(key);
        }
        if let Some(ah) = self.find_atomic_hash(name) {
            ah.0.lock().insert(key.to_string(), val);
            return Ok(());
        }
        if let Some(arc) = self.find_shared_hash(name) {
            arc.write().insert(key.to_string(), val);
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
    ) -> Result<(), StrykeError> {
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
                g.insert(key, StrykeValue::integer(i.wrapping_mul(k)));
            }
            return Ok(());
        }
        let hash = self.get_hash_mut(name)?;
        hash.reserve(count);
        let mut buf = itoa::Buffer::new();
        for i in start..end {
            let key = buf.format(i).to_owned();
            hash.insert(key, StrykeValue::integer(i.wrapping_mul(k)));
        }
        Ok(())
    }

    pub fn delete_hash_element(&mut self, name: &str, key: &str) -> Result<StrykeValue, StrykeError> {
        canon_main!(name);
        if let Some(ah) = self.find_atomic_hash(name) {
            return Ok(ah.0.lock().shift_remove(key).unwrap_or(StrykeValue::UNDEF));
        }
        let hash = self.get_hash_mut(name)?;
        Ok(hash.shift_remove(key).unwrap_or(StrykeValue::UNDEF))
    }

    #[inline]
    pub fn exists_hash_element(&self, name: &str, key: &str) -> bool {
        canon_main!(name);
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
    /// allocates one `StrykeValue::string` per key).
    #[inline]
    pub fn for_each_hash_value(&self, name: &str, mut visit: impl FnMut(&StrykeValue)) {
        canon_main!(name);
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

    /// Per-frame view of binding *names* (not values) for introspection
    /// pipelines that need to walk every name in every frame without
    /// reaching into private fields. Returns `(scalars, arrays, hashes)`.
    /// Atomic / shared variants are folded into the matching kind so the
    /// caller doesn't need to know the storage form.
    pub fn frames_for_introspection(&self) -> Vec<(Vec<&str>, Vec<&str>, Vec<&str>)> {
        self.frames
            .iter()
            .map(|f| {
                let mut scalars: Vec<&str> = f.scalars.iter().map(|(n, _)| n.as_str()).collect();
                // `my $x` ends up in scalar_slots; names live alongside.
                scalars.extend(f.scalar_slot_names.iter().filter_map(|opt| match opt {
                    Some(n) if !n.is_empty() => Some(n.as_str()),
                    _ => None,
                }));
                let mut arrays: Vec<&str> = f.arrays.iter().map(|(n, _)| n.as_str()).collect();
                arrays.extend(f.atomic_arrays.iter().map(|(n, _)| n.as_str()));
                arrays.extend(f.shared_arrays.iter().map(|(n, _)| n.as_str()));
                let mut hashes: Vec<&str> = f.hashes.iter().map(|(n, _)| n.as_str()).collect();
                hashes.extend(f.atomic_hashes.iter().map(|(n, _)| n.as_str()));
                hashes.extend(f.shared_hashes.iter().map(|(n, _)| n.as_str()));
                scalars.sort_unstable();
                arrays.sort_unstable();
                hashes.sort_unstable();
                (scalars, arrays, hashes)
            })
            .collect()
    }

    /// Sigil-prefixed name → variable-class string (`"scalar"`, `"array"`,
    /// `"hash"`, `"atomic_array"`, `"atomic_hash"`, `"shared_array"`,
    /// `"shared_hash"`) for every binding in every frame. Backs the
    /// `parameters()` builtin (zsh-`$parameters` analogue). Walks frames
    /// outermost → innermost so an inner shadow wins on duplicate names.
    pub fn parameters_pairs(&self) -> Vec<(String, &'static str)> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut out: Vec<(String, &'static str)> = Vec::new();
        // Iterate innermost first so the closest shadow registers first;
        // `seen` then suppresses outer duplicates.
        for frame in self.frames.iter().rev() {
            // Slot-allocated lexical scalars (`my $x` lands here). Names live
            // in `scalar_slot_names`; empty / None entries are anonymous
            // padding slots and skipped.
            for n in frame.scalar_slot_names.iter().flatten() {
                if !n.is_empty() {
                    let s = format!("${}", n);
                    if seen.insert(s.clone()) {
                        out.push((s, "scalar"));
                    }
                }
            }
            for (name, _) in &frame.scalars {
                let s = format!("${}", name);
                if seen.insert(s.clone()) {
                    out.push((s, "scalar"));
                }
            }
            for (name, _) in &frame.arrays {
                let s = format!("@{}", name);
                if seen.insert(s.clone()) {
                    out.push((s, "array"));
                }
            }
            for (name, _) in &frame.hashes {
                let s = format!("%{}", name);
                if seen.insert(s.clone()) {
                    out.push((s, "hash"));
                }
            }
            for (name, _) in &frame.atomic_arrays {
                let s = format!("@{}", name);
                if seen.insert(s.clone()) {
                    out.push((s, "atomic_array"));
                }
            }
            for (name, _) in &frame.atomic_hashes {
                let s = format!("%{}", name);
                if seen.insert(s.clone()) {
                    out.push((s, "atomic_hash"));
                }
            }
            for (name, _) in &frame.shared_arrays {
                let s = format!("@{}", name);
                if seen.insert(s.clone()) {
                    out.push((s, "shared_array"));
                }
            }
            for (name, _) in &frame.shared_hashes {
                let s = format!("%{}", name);
                if seen.insert(s.clone()) {
                    out.push((s, "shared_hash"));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
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

    pub fn capture(&mut self) -> Vec<(String, StrykeValue)> {
        // Capture wraps simple scalars in CaptureCell so repeat calls of the
        // SAME closure share state internally (factory pattern: `sub { ++$n }`
        // counts up). Whether the OUTER scope's storage is updated to share
        // the same cell — i.e. whether outer mutations are observable to the
        // closure (and vice versa) — depends on the mode:
        //
        //   - default stryke: cell is closure-local. Outer scope keeps its
        //     own storage. Outer mutations are NOT observable (DESIGN-001;
        //     race-free dispatch into pmap/pfor/async/spawn). Use `mysync`
        //     for explicitly-shared variables.
        //   - --compat: cell is shared by mutating outer storage to point at
        //     the same Arc. Perl 5 shared-storage closure semantics.
        let by_ref = crate::compat_mode();
        let mut captured = Vec::new();
        // Hash-stored scalar dedup: each name in `frame.scalars` has at most ONE binding
        // visible to the closure (innermost shadows outer). Without dedup, a name that
        // exists in multiple frames — e.g. `$_` restored into a callee frame by an earlier
        // `restore_capture`, while the top-level frame still holds the original — would be
        // pushed twice. `restore_capture` then declares them sequentially, and the second
        // `declare_scalar` write-throughs the first's CaptureCell with another CaptureCell,
        // nesting them. One `arc.read()` unwrap then surfaces the inner cell and renders
        // as `SCALAR(0x…)`. We walk hash-stored scalars innermost-first and skip names
        // already seen — only the innermost binding is captured.
        //
        // Slot-stored scalars, arrays, and hashes don't need dedup: they iterate
        // outer-first so that during `restore_capture` the innermost frame's value is
        // declared LAST, winning slot-index / hash-key collisions (factory-closure pattern
        // depends on this last-write-wins behavior).
        let mut seen_hash_scalars: HashSet<String> = HashSet::new();
        for frame in self.frames.iter_mut().rev() {
            for (k, v) in &mut frame.scalars {
                if !seen_hash_scalars.insert(k.clone()) {
                    continue;
                }
                if v.as_capture_cell().is_some() || v.as_scalar_ref().is_some() {
                    captured.push((format!("${}", k), v.clone()));
                } else if v.is_simple_scalar() {
                    let wrapped = StrykeValue::capture_cell(Arc::new(RwLock::new(v.clone())));
                    *v = wrapped.clone();
                    captured.push((format!("${}", k), wrapped));
                } else {
                    captured.push((format!("${}", k), v.clone()));
                }
            }
        }
        for frame in &mut self.frames {
            // Slot-stored scalars are lexical `my` declarations. Closure
            // capture rule (DESIGN-001):
            //   - default stryke: cell is closure-local. Repeat calls of the
            //     same closure share state (factory pattern), but outer
            //     mutations are NOT observable. Use `mysync` for shared
            //     state.
            //   - --compat: cell is shared with outer scope (Perl 5).
            for (i, v) in frame.scalar_slots.iter_mut().enumerate() {
                if let Some(Some(name)) = frame.scalar_slot_names.get(i) {
                    // Cross-storage shadow check: a hash-stored scalar with this
                    // name was already captured from an inner frame (e.g. a
                    // sub-parameter declared via `apply_sub_signature` in the
                    // callee's frame). Capturing the outer slot-stored entry too
                    // would put BOTH into the closure's call frame on restore,
                    // and `Frame::get_scalar` checks slots before scalars — so
                    // the slot-stored OUTER value would shadow the parameter on
                    // every closure body lookup. Skip the slot-stored entry to
                    // let the hash-stored param win at runtime.
                    if !name.is_empty() && seen_hash_scalars.contains(name) {
                        continue;
                    }
                    let cap_val = if v.as_capture_cell().is_some() || v.as_scalar_ref().is_some() {
                        v.clone()
                    } else {
                        let wrapped = StrykeValue::capture_cell(Arc::new(RwLock::new(v.clone())));
                        if by_ref {
                            *v = wrapped.clone();
                        }
                        wrapped
                    };
                    captured.push((format!("$slot:{}:{}", i, name), cap_val));
                }
            }
            for (k, v) in &frame.arrays {
                if capture_skip_bootstrap_array(k) {
                    continue;
                }
                if frame.frozen_arrays.contains(k) {
                    captured.push((format!("@frozen:{}", k), StrykeValue::array(v.clone())));
                } else {
                    captured.push((format!("@{}", k), StrykeValue::array(v.clone())));
                }
            }
            for (k, v) in &frame.hashes {
                if capture_skip_bootstrap_hash(k) {
                    continue;
                }
                if frame.frozen_hashes.contains(k) {
                    captured.push((format!("%frozen:{}", k), StrykeValue::hash(v.clone())));
                } else {
                    captured.push((format!("%{}", k), StrykeValue::hash(v.clone())));
                }
            }
            for (k, _aa) in &frame.atomic_arrays {
                captured.push((
                    format!("@sync_{}", k),
                    StrykeValue::atomic(Arc::new(Mutex::new(StrykeValue::string(String::new())))),
                ));
            }
            for (k, _ah) in &frame.atomic_hashes {
                captured.push((
                    format!("%sync_{}", k),
                    StrykeValue::atomic(Arc::new(Mutex::new(StrykeValue::string(String::new())))),
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
                    scalars.push((format!("@frozen:{}", k), StrykeValue::array(v.clone())));
                } else {
                    scalars.push((format!("@{}", k), StrykeValue::array(v.clone())));
                }
            }
            for (k, v) in &frame.hashes {
                if capture_skip_bootstrap_hash(k) {
                    continue;
                }
                if frame.frozen_hashes.contains(k) {
                    scalars.push((format!("%frozen:{}", k), StrykeValue::hash(v.clone())));
                } else {
                    scalars.push((format!("%{}", k), StrykeValue::hash(v.clone())));
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

    pub fn restore_capture(&mut self, captured: &[(String, StrykeValue)]) {
        for (name, val) in captured {
            if let Some(rest) = name.strip_prefix("$slot:") {
                // "$slot:INDEX:NAME" — restore into scalar_slots only.
                // `get_scalar` finds slots via `get_scalar_from_slot`, so a separate
                // `declare_scalar` is unnecessary and would double-wrap: `set_scalar`
                // sees the slot's ScalarRef and writes *through* it, nesting
                // `ScalarRef(ScalarRef(inner))`.
                if let Some(colon) = rest.find(':') {
                    let idx: usize = rest[..colon].parse().unwrap_or(0);
                    let sname = &rest[colon + 1..];
                    self.declare_scalar_slot(idx as u8, val.clone(), Some(sname));
                }
            } else if let Some(stripped) = name.strip_prefix('$') {
                self.declare_scalar(stripped, val.clone());
                // Topic positional slot like `_1`, `_2<`, `_12<<<<` — bump
                // `max_active_slot` so the next `set_topic` shifts that slot's
                // outer-topic chain. Without this, lazy iterators built from a
                // fresh `Interpreter` (FilterStreamIterator etc.) lose `_1<`
                // because `set_topic`'s shift loop runs `1..=max_active_slot`
                // and that high-water mark resets to 0 in a fresh scope.
                if let Some(slot) = parse_positional_topic_slot(stripped) {
                    if slot > self.max_active_slot {
                        self.max_active_slot = slot;
                    }
                }
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
    use crate::value::StrykeValue;

    #[test]
    fn missing_scalar_is_undef() {
        let s = Scope::new();
        assert!(s.get_scalar("not_declared").is_undef());
    }

    #[test]
    fn inner_frame_shadows_outer_scalar() {
        let mut s = Scope::new();
        s.declare_scalar("a", StrykeValue::integer(1));
        s.push_frame();
        s.declare_scalar("a", StrykeValue::integer(2));
        assert_eq!(s.get_scalar("a").to_int(), 2);
        s.pop_frame();
        assert_eq!(s.get_scalar("a").to_int(), 1);
    }

    #[test]
    fn set_scalar_updates_innermost_binding() {
        let mut s = Scope::new();
        s.declare_scalar("a", StrykeValue::integer(1));
        s.push_frame();
        s.declare_scalar("a", StrykeValue::integer(2));
        let _ = s.set_scalar("a", StrykeValue::integer(99));
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
                StrykeValue::integer(10),
                StrykeValue::integer(20),
                StrykeValue::integer(30),
            ],
        );
        assert_eq!(s.get_array_element("a", -1).to_int(), 30);
    }

    #[test]
    fn set_array_element_extends_array_with_undef_gaps() {
        let mut s = Scope::new();
        s.declare_array("a", vec![]);
        s.set_array_element("a", 2, StrykeValue::integer(7))
            .unwrap();
        assert_eq!(s.get_array_element("a", 2).to_int(), 7);
        assert!(s.get_array_element("a", 0).is_undef());
    }

    #[test]
    fn capture_restore_roundtrip_scalar() {
        let mut s = Scope::new();
        s.declare_scalar("n", StrykeValue::integer(42));
        let cap = s.capture();
        let mut t = Scope::new();
        t.restore_capture(&cap);
        assert_eq!(t.get_scalar("n").to_int(), 42);
    }

    #[test]
    fn capture_restore_roundtrip_lexical_array_and_hash() {
        let mut s = Scope::new();
        s.declare_array("a", vec![StrykeValue::integer(1), StrykeValue::integer(2)]);
        let mut m = IndexMap::new();
        m.insert("k".to_string(), StrykeValue::integer(99));
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
        m.insert("k".to_string(), StrykeValue::integer(1));
        s.declare_hash("h", m);
        assert_eq!(s.get_hash_element("h", "k").to_int(), 1);
        assert!(s.exists_hash_element("h", "k"));
        s.set_hash_element("h", "k", StrykeValue::integer(99))
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
        outer.insert("k".to_string(), StrykeValue::integer(1));
        s.declare_hash("h", outer);
        s.push_frame();
        let mut inner = IndexMap::new();
        inner.insert("k".to_string(), StrykeValue::integer(2));
        s.declare_hash("h", inner);
        assert_eq!(s.get_hash_element("h", "k").to_int(), 2);
        s.pop_frame();
        assert_eq!(s.get_hash_element("h", "k").to_int(), 1);
    }

    #[test]
    fn inner_frame_shadows_outer_array_name() {
        let mut s = Scope::new();
        s.declare_array("a", vec![StrykeValue::integer(1)]);
        s.push_frame();
        s.declare_array("a", vec![StrykeValue::integer(2), StrykeValue::integer(3)]);
        assert_eq!(s.get_array_element("a", 1).to_int(), 3);
        s.pop_frame();
        assert_eq!(s.get_array_element("a", 0).to_int(), 1);
    }

    #[test]
    fn pop_frame_never_removes_global_frame() {
        let mut s = Scope::new();
        s.declare_scalar("x", StrykeValue::integer(1));
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
        s.push_to_array("a", StrykeValue::integer(1)).unwrap();
        s.push_to_array("a", StrykeValue::integer(2)).unwrap();
        assert_eq!(s.array_len("a"), 2);
        assert_eq!(s.pop_from_array("a").unwrap().to_int(), 2);
        assert_eq!(s.pop_from_array("a").unwrap().to_int(), 1);
        assert!(s.pop_from_array("a").unwrap().is_undef());
    }

    #[test]
    fn shift_from_array_drops_front() {
        let mut s = Scope::new();
        s.declare_array("a", vec![StrykeValue::integer(1), StrykeValue::integer(2)]);
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
            StrykeValue::atomic(Arc::new(Mutex::new(StrykeValue::integer(10)))),
        );
        let v = s
            .atomic_mutate("n", |old| StrykeValue::integer(old.to_int() + 5))
            .expect("atomic_mutate on atomic-backed scalar must not fail");
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
            StrykeValue::atomic(Arc::new(Mutex::new(StrykeValue::integer(7)))),
        );
        let old = s
            .atomic_mutate_post("n", |v| StrykeValue::integer(v.to_int() + 1))
            .expect("atomic_mutate_post on atomic-backed scalar must not fail");
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
            StrykeValue::atomic(Arc::new(Mutex::new(StrykeValue::integer(3)))),
        );
        assert!(s.get_scalar_raw("n").is_atomic());
        assert!(!s.get_scalar("n").is_atomic());
    }

    #[test]
    fn missing_array_element_is_undef() {
        let mut s = Scope::new();
        s.declare_array("a", vec![StrykeValue::integer(1)]);
        assert!(s.get_array_element("a", 99).is_undef());
    }

    #[test]
    fn restore_atomics_puts_atomic_containers_in_frame() {
        use indexmap::IndexMap;
        use parking_lot::Mutex;
        use std::sync::Arc;
        let mut s = Scope::new();
        let aa = AtomicArray(Arc::new(Mutex::new(vec![StrykeValue::integer(1)])));
        let ah = AtomicHash(Arc::new(Mutex::new(IndexMap::new())));
        s.restore_atomics(&[("ax".into(), aa.clone())], &[("hx".into(), ah.clone())]);
        assert_eq!(s.get_array_element("ax", 0).to_int(), 1);
        assert_eq!(s.array_len("ax"), 1);
        s.set_hash_element("hx", "k", StrykeValue::integer(2))
            .unwrap();
        assert_eq!(s.get_hash_element("hx", "k").to_int(), 2);
    }

    // ── topic_alias / outer-chain aliasing ──────────────────────────────
    //
    // The debugger and `for (@arr) { … }` loops depend on `$_` ↔ `$_0`
    // (and outer-chain analogues `_<` ↔ `_0<`) reading the same slot.
    // If `topic_alias` ever drops a mapping the user sees ghost values
    // in the Variables panel where `$_` and `$_0` disagree.

    #[test]
    fn topic_alias_pairs_underscore_with_zero() {
        assert_eq!(topic_alias("_").as_deref(), Some("_0"));
        assert_eq!(topic_alias("_0").as_deref(), Some("_"));
    }

    #[test]
    fn topic_alias_pairs_outer_chain_with_zero_form() {
        // `_<` ↔ `_0<`, `_<<<` ↔ `_0<<<`, etc.
        assert_eq!(topic_alias("_<").as_deref(), Some("_0<"));
        assert_eq!(topic_alias("_0<").as_deref(), Some("_<"));
        assert_eq!(topic_alias("_<<<").as_deref(), Some("_0<<<"));
        assert_eq!(topic_alias("_0<<<").as_deref(), Some("_<<<"));
    }

    #[test]
    fn topic_alias_has_no_pair_for_other_positionals() {
        // `_1`, `_2`, etc. are positional-only — no `$_` alias.
        assert!(topic_alias("_1").is_none());
        assert!(topic_alias("_2").is_none());
        assert!(topic_alias("_42").is_none());
        // `_<+digits` is mixed (slice index) — not a chevron-only chain.
        assert!(topic_alias("_<5").is_none());
        // Plain identifiers.
        assert!(topic_alias("foo").is_none());
        assert!(topic_alias("_foo").is_none());
    }

    // ── parse_positional_topic_slot ─────────────────────────────────────

    #[test]
    fn positional_topic_slot_parses_underscore_n() {
        // Only N >= 1 — `_0` is the topic alias for `_` (see
        // [`topic_alias`]), not a positional slot.
        assert_eq!(parse_positional_topic_slot("_1"), Some(1));
        assert_eq!(parse_positional_topic_slot("_2"), Some(2));
        assert_eq!(parse_positional_topic_slot("_42"), Some(42));
    }

    #[test]
    fn positional_topic_slot_rejects_non_positional_names() {
        assert!(parse_positional_topic_slot("_").is_none(), "bare _ has no slot");
        assert!(parse_positional_topic_slot("_0").is_none(), "_0 is the topic alias, not positional");
        assert!(parse_positional_topic_slot("_foo").is_none(), "named");
        assert!(parse_positional_topic_slot("foo").is_none());
        assert!(parse_positional_topic_slot("").is_none());
    }
}
