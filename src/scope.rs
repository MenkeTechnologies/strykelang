use indexmap::IndexMap;
use std::collections::HashMap;

use crate::value::PerlValue;

/// A single lexical scope frame.
#[derive(Debug, Clone)]
struct Frame {
    /// Variable storage: name (without sigil) → value
    scalars: HashMap<String, PerlValue>,
    arrays: HashMap<String, Vec<PerlValue>>,
    hashes: HashMap<String, IndexMap<String, PerlValue>>,
}

impl Frame {
    fn new() -> Self {
        Self {
            scalars: HashMap::new(),
            arrays: HashMap::new(),
            hashes: HashMap::new(),
        }
    }
}

/// Manages lexical scoping with a stack of frames.
/// Innermost frame is last in the vector.
#[derive(Debug, Clone)]
pub struct Scope {
    frames: Vec<Frame>,
}

impl Scope {
    pub fn new() -> Self {
        let mut s = Self {
            frames: Vec::with_capacity(16),
        };
        s.frames.push(Frame::new()); // global frame
        s
    }

    pub fn push_frame(&mut self) {
        self.frames.push(Frame::new());
    }

    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    // ── Scalars ──

    pub fn declare_scalar(&mut self, name: &str, val: PerlValue) {
        if let Some(frame) = self.frames.last_mut() {
            frame.scalars.insert(name.to_string(), val);
        }
    }

    pub fn get_scalar(&self, name: &str) -> PerlValue {
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.scalars.get(name) {
                return val.clone();
            }
        }
        PerlValue::Undef
    }

    pub fn set_scalar(&mut self, name: &str, val: PerlValue) {
        // Walk frames from innermost; if declared, update there.
        for frame in self.frames.iter_mut().rev() {
            if frame.scalars.contains_key(name) {
                frame.scalars.insert(name.to_string(), val);
                return;
            }
        }
        // Not found — assign in global scope.
        self.frames[0].scalars.insert(name.to_string(), val);
    }

    // ── Arrays ──

    pub fn declare_array(&mut self, name: &str, val: Vec<PerlValue>) {
        if let Some(frame) = self.frames.last_mut() {
            frame.arrays.insert(name.to_string(), val);
        }
    }

    pub fn get_array(&self, name: &str) -> Vec<PerlValue> {
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.arrays.get(name) {
                return val.clone();
            }
        }
        Vec::new()
    }

    pub fn get_array_mut(&mut self, name: &str) -> &mut Vec<PerlValue> {
        // Find which frame contains this array (index-based to satisfy borrow checker).
        let mut target_idx = None;
        for i in (0..self.frames.len()).rev() {
            if self.frames[i].arrays.contains_key(name) {
                target_idx = Some(i);
                break;
            }
        }
        let idx = target_idx.unwrap_or(0);
        self.frames[idx]
            .arrays
            .entry(name.to_string())
            .or_insert_with(Vec::new)
    }

    pub fn set_array(&mut self, name: &str, val: Vec<PerlValue>) {
        for frame in self.frames.iter_mut().rev() {
            if frame.arrays.contains_key(name) {
                frame.arrays.insert(name.to_string(), val);
                return;
            }
        }
        self.frames[0].arrays.insert(name.to_string(), val);
    }

    pub fn get_array_element(&self, name: &str, index: i64) -> PerlValue {
        let arr = self.get_array(name);
        let idx = if index < 0 {
            (arr.len() as i64 + index) as usize
        } else {
            index as usize
        };
        arr.get(idx).cloned().unwrap_or(PerlValue::Undef)
    }

    pub fn set_array_element(&mut self, name: &str, index: i64, val: PerlValue) {
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

    pub fn declare_hash(&mut self, name: &str, val: IndexMap<String, PerlValue>) {
        if let Some(frame) = self.frames.last_mut() {
            frame.hashes.insert(name.to_string(), val);
        }
    }

    pub fn get_hash(&self, name: &str) -> IndexMap<String, PerlValue> {
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.hashes.get(name) {
                return val.clone();
            }
        }
        IndexMap::new()
    }

    pub fn get_hash_mut(&mut self, name: &str) -> &mut IndexMap<String, PerlValue> {
        let mut target_idx = None;
        for i in (0..self.frames.len()).rev() {
            if self.frames[i].hashes.contains_key(name) {
                target_idx = Some(i);
                break;
            }
        }
        let idx = target_idx.unwrap_or(0);
        self.frames[idx]
            .hashes
            .entry(name.to_string())
            .or_insert_with(IndexMap::new)
    }

    pub fn set_hash(&mut self, name: &str, val: IndexMap<String, PerlValue>) {
        for frame in self.frames.iter_mut().rev() {
            if frame.hashes.contains_key(name) {
                frame.hashes.insert(name.to_string(), val);
                return;
            }
        }
        self.frames[0].hashes.insert(name.to_string(), val);
    }

    pub fn get_hash_element(&self, name: &str, key: &str) -> PerlValue {
        let hash = self.get_hash(name);
        hash.get(key).cloned().unwrap_or(PerlValue::Undef)
    }

    pub fn set_hash_element(&mut self, name: &str, key: &str, val: PerlValue) {
        let hash = self.get_hash_mut(name);
        hash.insert(key.to_string(), val);
    }

    pub fn delete_hash_element(&mut self, name: &str, key: &str) -> PerlValue {
        let hash = self.get_hash_mut(name);
        hash.shift_remove(key).unwrap_or(PerlValue::Undef)
    }

    pub fn exists_hash_element(&self, name: &str, key: &str) -> bool {
        let hash = self.get_hash(name);
        hash.contains_key(key)
    }

    /// Capture current scope as flat list of (name, value) pairs for closures.
    pub fn capture(&self) -> Vec<(String, PerlValue)> {
        let mut captured = Vec::new();
        for frame in &self.frames {
            for (k, v) in &frame.scalars {
                captured.push((format!("${}", k), v.clone()));
            }
        }
        captured
    }

    /// Restore captured variables into a new frame.
    pub fn restore_capture(&mut self, captured: &[(String, PerlValue)]) {
        for (name, val) in captured {
            if let Some(stripped) = name.strip_prefix('$') {
                self.declare_scalar(stripped, val.clone());
            }
        }
    }
}
