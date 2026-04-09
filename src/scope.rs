use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::Mutex;

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

/// A single lexical scope frame.
/// Uses Vec instead of HashMap — for typical Perl code with < 10 variables per
/// scope, linear scan is faster than hashing due to cache locality and zero
/// hash overhead.
#[derive(Debug, Clone)]
struct Frame {
    scalars: Vec<(String, PerlValue)>,
    arrays: Vec<(String, Vec<PerlValue>)>,
    hashes: Vec<(String, IndexMap<String, PerlValue>)>,
    /// Thread-safe arrays from `mysync @a`
    atomic_arrays: Vec<(String, AtomicArray)>,
    /// Thread-safe hashes from `mysync %h`
    atomic_hashes: Vec<(String, AtomicHash)>,
}

impl Frame {
    #[inline]
    fn new() -> Self {
        Self {
            scalars: Vec::new(),
            arrays: Vec::new(),
            hashes: Vec::new(),
            atomic_arrays: Vec::new(),
            atomic_hashes: Vec::new(),
        }
    }

    #[inline]
    fn get_scalar(&self, name: &str) -> Option<&PerlValue> {
        // Linear scan — faster than HashMap for N < ~15
        self.scalars.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    #[inline]
    fn has_scalar(&self, name: &str) -> bool {
        self.scalars.iter().any(|(k, _)| k == name)
    }

    #[inline]
    fn set_scalar(&mut self, name: &str, val: PerlValue) {
        if let Some(entry) = self.scalars.iter_mut().find(|(k, _)| k == name) {
            entry.1 = val;
        } else {
            self.scalars.push((name.to_string(), val));
        }
    }

    #[inline]
    fn get_array(&self, name: &str) -> Option<&Vec<PerlValue>> {
        self.arrays.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    #[inline]
    fn has_array(&self, name: &str) -> bool {
        self.arrays.iter().any(|(k, _)| k == name)
    }

    #[inline]
    fn get_array_mut(&mut self, name: &str) -> Option<&mut Vec<PerlValue>> {
        self.arrays
            .iter_mut()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }

    #[inline]
    fn set_array(&mut self, name: &str, val: Vec<PerlValue>) {
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
        };
        s.frames.push(Frame::new());
        s
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
            self.frames.pop();
        }
    }

    #[inline]
    pub fn push_frame(&mut self) {
        self.frames.push(Frame::new());
    }

    #[inline]
    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    // ── Scalars ──

    #[inline]
    pub fn declare_scalar(&mut self, name: &str, val: PerlValue) {
        if let Some(frame) = self.frames.last_mut() {
            frame.set_scalar(name, val);
        }
    }

    #[inline]
    pub fn get_scalar(&self, name: &str) -> PerlValue {
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.get_scalar(name) {
                // Transparently unwrap Atomic — read through the lock
                if let PerlValue::Atomic(ref arc) = val {
                    return arc.lock().clone();
                }
                return val.clone();
            }
        }
        PerlValue::Undef
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
        PerlValue::Undef
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
            if let Some(PerlValue::Atomic(ref arc)) = frame.get_scalar(name) {
                let mut guard = arc.lock();
                let new_val = f(&guard);
                *guard = new_val.clone();
                return new_val;
            }
        }
        // Non-atomic fallback
        let old = self.get_scalar(name);
        let new_val = f(&old);
        self.set_scalar(name, new_val.clone());
        new_val
    }

    /// Like atomic_mutate but returns the OLD value (for postfix `$x++`).
    pub fn atomic_mutate_post(
        &mut self,
        name: &str,
        f: impl FnOnce(&PerlValue) -> PerlValue,
    ) -> PerlValue {
        for frame in self.frames.iter().rev() {
            if let Some(PerlValue::Atomic(ref arc)) = frame.get_scalar(name) {
                let mut guard = arc.lock();
                let old = guard.clone();
                *guard = f(&old);
                return old;
            }
        }
        // Non-atomic fallback
        let old = self.get_scalar(name);
        self.set_scalar(name, f(&old));
        old
    }

    #[inline]
    pub fn set_scalar(&mut self, name: &str, val: PerlValue) {
        for frame in self.frames.iter_mut().rev() {
            // If the existing value is Atomic, write through the lock
            if let Some(PerlValue::Atomic(ref arc)) = frame.get_scalar(name) {
                *arc.lock() = val;
                return;
            }
            if frame.has_scalar(name) {
                frame.set_scalar(name, val);
                return;
            }
        }
        self.frames[0].set_scalar(name, val);
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

    #[inline]
    pub fn declare_array(&mut self, name: &str, val: Vec<PerlValue>) {
        if let Some(frame) = self.frames.last_mut() {
            frame.set_array(name, val);
        }
    }

    pub fn get_array(&self, name: &str) -> Vec<PerlValue> {
        // Check atomic arrays first
        if let Some(aa) = self.find_atomic_array(name) {
            return aa.0.lock().clone();
        }
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.get_array(name) {
                return val.clone();
            }
        }
        Vec::new()
    }

    pub fn get_array_mut(&mut self, name: &str) -> &mut Vec<PerlValue> {
        // Note: can't return &mut into a Mutex. Callers needing atomic array
        // mutation should use atomic_array_mutate instead. For non-atomic arrays:
        let mut target_idx = None;
        for i in (0..self.frames.len()).rev() {
            if self.frames[i].has_array(name) {
                target_idx = Some(i);
                break;
            }
        }
        let idx = target_idx.unwrap_or(0);
        let frame = &mut self.frames[idx];
        if frame.get_array_mut(name).is_none() {
            frame.arrays.push((name.to_string(), Vec::new()));
        }
        frame.get_array_mut(name).unwrap()
    }

    /// Push to array — works for both regular and atomic arrays.
    pub fn push_to_array(&mut self, name: &str, val: PerlValue) {
        if let Some(aa) = self.find_atomic_array(name) {
            aa.0.lock().push(val);
            return;
        }
        self.get_array_mut(name).push(val);
    }

    /// Pop from array — works for both regular and atomic arrays.
    pub fn pop_from_array(&mut self, name: &str) -> PerlValue {
        if let Some(aa) = self.find_atomic_array(name) {
            return aa.0.lock().pop().unwrap_or(PerlValue::Undef);
        }
        self.get_array_mut(name).pop().unwrap_or(PerlValue::Undef)
    }

    /// Shift from array — works for both regular and atomic arrays.
    pub fn shift_from_array(&mut self, name: &str) -> PerlValue {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut guard = aa.0.lock();
            return if guard.is_empty() {
                PerlValue::Undef
            } else {
                guard.remove(0)
            };
        }
        let arr = self.get_array_mut(name);
        if arr.is_empty() {
            PerlValue::Undef
        } else {
            arr.remove(0)
        }
    }

    /// Get array length — works for both regular and atomic arrays.
    pub fn array_len(&self, name: &str) -> usize {
        if let Some(aa) = self.find_atomic_array(name) {
            return aa.0.lock().len();
        }
        for frame in self.frames.iter().rev() {
            if let Some(arr) = frame.get_array(name) {
                return arr.len();
            }
        }
        0
    }

    pub fn set_array(&mut self, name: &str, val: Vec<PerlValue>) {
        if let Some(aa) = self.find_atomic_array(name) {
            *aa.0.lock() = val;
            return;
        }
        for frame in self.frames.iter_mut().rev() {
            if frame.has_array(name) {
                frame.set_array(name, val);
                return;
            }
        }
        self.frames[0].set_array(name, val);
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
            return arr.get(idx).cloned().unwrap_or(PerlValue::Undef);
        }
        for frame in self.frames.iter().rev() {
            if let Some(arr) = frame.get_array(name) {
                let idx = if index < 0 {
                    (arr.len() as i64 + index) as usize
                } else {
                    index as usize
                };
                return arr.get(idx).cloned().unwrap_or(PerlValue::Undef);
            }
        }
        PerlValue::Undef
    }

    pub fn set_array_element(&mut self, name: &str, index: i64, val: PerlValue) {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut arr = aa.0.lock();
            let idx = if index < 0 {
                (arr.len() as i64 + index).max(0) as usize
            } else {
                index as usize
            };
            if idx >= arr.len() {
                arr.resize(idx + 1, PerlValue::Undef);
            }
            arr[idx] = val;
            return;
        }
        let arr = self.get_array_mut(name);
        let idx = if index < 0 {
            let len = arr.len() as i64;
            (len + index).max(0) as usize
        } else {
            index as usize
        };
        if idx >= arr.len() {
            arr.resize(idx + 1, PerlValue::Undef);
        }
        arr[idx] = val;
    }

    // ── Hashes ──

    #[inline]
    pub fn declare_hash(&mut self, name: &str, val: IndexMap<String, PerlValue>) {
        if let Some(frame) = self.frames.last_mut() {
            frame.set_hash(name, val);
        }
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

    pub fn get_hash_mut(&mut self, name: &str) -> &mut IndexMap<String, PerlValue> {
        let mut target_idx = None;
        for i in (0..self.frames.len()).rev() {
            if self.frames[i].has_hash(name) {
                target_idx = Some(i);
                break;
            }
        }
        let idx = target_idx.unwrap_or(0);
        let frame = &mut self.frames[idx];
        if frame.get_hash_mut(name).is_none() {
            frame.hashes.push((name.to_string(), IndexMap::new()));
        }
        frame.get_hash_mut(name).unwrap()
    }

    pub fn set_hash(&mut self, name: &str, val: IndexMap<String, PerlValue>) {
        if let Some(ah) = self.find_atomic_hash(name) {
            *ah.0.lock() = val;
            return;
        }
        for frame in self.frames.iter_mut().rev() {
            if frame.has_hash(name) {
                frame.set_hash(name, val);
                return;
            }
        }
        self.frames[0].set_hash(name, val);
    }

    #[inline]
    pub fn get_hash_element(&self, name: &str, key: &str) -> PerlValue {
        if let Some(ah) = self.find_atomic_hash(name) {
            return ah.0.lock().get(key).cloned().unwrap_or(PerlValue::Undef);
        }
        for frame in self.frames.iter().rev() {
            if let Some(hash) = frame.get_hash(name) {
                return hash.get(key).cloned().unwrap_or(PerlValue::Undef);
            }
        }
        PerlValue::Undef
    }

    /// Atomically read-modify-write a hash element. For atomic hashes, holds
    /// the Mutex for the full cycle. Returns the new value.
    pub fn atomic_hash_mutate(
        &mut self,
        name: &str,
        key: &str,
        f: impl FnOnce(&PerlValue) -> PerlValue,
    ) -> PerlValue {
        if let Some(ah) = self.find_atomic_hash(name) {
            let mut guard = ah.0.lock();
            let old = guard.get(key).cloned().unwrap_or(PerlValue::Undef);
            let new_val = f(&old);
            guard.insert(key.to_string(), new_val.clone());
            return new_val;
        }
        // Non-atomic fallback
        let old = self.get_hash_element(name, key);
        let new_val = f(&old);
        self.set_hash_element(name, key, new_val.clone());
        new_val
    }

    /// Atomically read-modify-write an array element. Returns the new value.
    pub fn atomic_array_mutate(
        &mut self,
        name: &str,
        index: i64,
        f: impl FnOnce(&PerlValue) -> PerlValue,
    ) -> PerlValue {
        if let Some(aa) = self.find_atomic_array(name) {
            let mut guard = aa.0.lock();
            let idx = if index < 0 { (guard.len() as i64 + index).max(0) as usize } else { index as usize };
            if idx >= guard.len() {
                guard.resize(idx + 1, PerlValue::Undef);
            }
            let old = guard[idx].clone();
            let new_val = f(&old);
            guard[idx] = new_val.clone();
            return new_val;
        }
        // Non-atomic fallback
        let old = self.get_array_element(name, index);
        let new_val = f(&old);
        self.set_array_element(name, index, new_val.clone());
        new_val
    }

    pub fn set_hash_element(&mut self, name: &str, key: &str, val: PerlValue) {
        if let Some(ah) = self.find_atomic_hash(name) {
            ah.0.lock().insert(key.to_string(), val);
            return;
        }
        let hash = self.get_hash_mut(name);
        hash.insert(key.to_string(), val);
    }

    pub fn delete_hash_element(&mut self, name: &str, key: &str) -> PerlValue {
        if let Some(ah) = self.find_atomic_hash(name) {
            return ah.0.lock().shift_remove(key).unwrap_or(PerlValue::Undef);
        }
        let hash = self.get_hash_mut(name);
        hash.shift_remove(key).unwrap_or(PerlValue::Undef)
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

    pub fn capture(&self) -> Vec<(String, PerlValue)> {
        let mut captured = Vec::new();
        for frame in &self.frames {
            for (k, v) in &frame.scalars {
                captured.push((format!("${}", k), v.clone()));
            }
            // Capture atomic arrays as special markers with the Arc
            for (k, _aa) in &frame.atomic_arrays {
                // Store as "$__atomic_array__NAME" with the Arc wrapped in a PerlValue
                // We use a special prefix that restore_capture will recognize
                captured.push((
                    format!("@sync_{}", k),
                    PerlValue::Atomic(Arc::new(Mutex::new(PerlValue::String(String::new())))),
                ));
                // Actually store the real Arc — we need a way to pass it.
                // Use a side channel: store a PerlValue that wraps the AtomicArray's Arc.
                // Hack: we can't put Arc<Mutex<Vec>> in PerlValue. Instead,
                // we'll store the array name and let restore_capture re-find it.
            }
            for (k, _ah) in &frame.atomic_hashes {
                captured.push((
                    format!("%sync_{}", k),
                    PerlValue::Atomic(Arc::new(Mutex::new(PerlValue::String(String::new())))),
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
            if let Some(stripped) = name.strip_prefix('$') {
                self.declare_scalar(stripped, val.clone());
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
        assert!(matches!(s.get_scalar("not_declared"), PerlValue::Undef));
    }

    #[test]
    fn inner_frame_shadows_outer_scalar() {
        let mut s = Scope::new();
        s.declare_scalar("a", PerlValue::Integer(1));
        s.push_frame();
        s.declare_scalar("a", PerlValue::Integer(2));
        assert_eq!(s.get_scalar("a").to_int(), 2);
        s.pop_frame();
        assert_eq!(s.get_scalar("a").to_int(), 1);
    }

    #[test]
    fn set_scalar_updates_innermost_binding() {
        let mut s = Scope::new();
        s.declare_scalar("a", PerlValue::Integer(1));
        s.push_frame();
        s.declare_scalar("a", PerlValue::Integer(2));
        s.set_scalar("a", PerlValue::Integer(99));
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
                PerlValue::Integer(10),
                PerlValue::Integer(20),
                PerlValue::Integer(30),
            ],
        );
        assert_eq!(s.get_array_element("a", -1).to_int(), 30);
    }

    #[test]
    fn set_array_element_extends_array_with_undef_gaps() {
        let mut s = Scope::new();
        s.declare_array("a", vec![]);
        s.set_array_element("a", 2, PerlValue::Integer(7));
        assert_eq!(s.get_array_element("a", 2).to_int(), 7);
        assert!(matches!(s.get_array_element("a", 0), PerlValue::Undef));
    }

    #[test]
    fn capture_restore_roundtrip_scalar() {
        let mut s = Scope::new();
        s.declare_scalar("n", PerlValue::Integer(42));
        let cap = s.capture();
        let mut t = Scope::new();
        t.restore_capture(&cap);
        assert_eq!(t.get_scalar("n").to_int(), 42);
    }

    #[test]
    fn hash_get_set_delete_exists() {
        let mut s = Scope::new();
        let mut m = IndexMap::new();
        m.insert("k".to_string(), PerlValue::Integer(1));
        s.declare_hash("h", m);
        assert_eq!(s.get_hash_element("h", "k").to_int(), 1);
        assert!(s.exists_hash_element("h", "k"));
        s.set_hash_element("h", "k", PerlValue::Integer(99));
        assert_eq!(s.get_hash_element("h", "k").to_int(), 99);
        let del = s.delete_hash_element("h", "k");
        assert_eq!(del.to_int(), 99);
        assert!(!s.exists_hash_element("h", "k"));
    }

    #[test]
    fn inner_frame_shadows_outer_hash_name() {
        let mut s = Scope::new();
        let mut outer = IndexMap::new();
        outer.insert("k".to_string(), PerlValue::Integer(1));
        s.declare_hash("h", outer);
        s.push_frame();
        let mut inner = IndexMap::new();
        inner.insert("k".to_string(), PerlValue::Integer(2));
        s.declare_hash("h", inner);
        assert_eq!(s.get_hash_element("h", "k").to_int(), 2);
        s.pop_frame();
        assert_eq!(s.get_hash_element("h", "k").to_int(), 1);
    }

    #[test]
    fn inner_frame_shadows_outer_array_name() {
        let mut s = Scope::new();
        s.declare_array("a", vec![PerlValue::Integer(1)]);
        s.push_frame();
        s.declare_array("a", vec![PerlValue::Integer(2), PerlValue::Integer(3)]);
        assert_eq!(s.get_array_element("a", 1).to_int(), 3);
        s.pop_frame();
        assert_eq!(s.get_array_element("a", 0).to_int(), 1);
    }

    #[test]
    fn pop_frame_never_removes_global_frame() {
        let mut s = Scope::new();
        s.declare_scalar("x", PerlValue::Integer(1));
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
}
