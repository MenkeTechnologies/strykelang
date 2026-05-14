//! Probabilistic data structures as native stryke builtins.
//!
//! World-first claim: no other scripting language ships these as stdlib
//! primitives. Python/Ruby/Node/Perl all require third-party packages
//! (`pyprobables`, `hyperloglogplus` gem, `bloom-filters` npm, etc.).
//! stryke ships them next to `set` / `deque` / `heap` as first-class
//! `%b` builtins.
//!
//! Storage: each sketch is a `HeapObject` variant wrapped in
//! `Arc<Mutex<…>>` so they're cheap to clone (refcount) and safe under
//! `pmap` / `pgrep` parallel iteration.
//!
//! Hashing: `xxhash_rust::xxh3` already in deps; we use double-hashing
//! (Kirsch–Mitzenmacher) so any `k` hash functions are derived from a
//! single 128-bit xxh3 call. No new crate, no FFI.

#![allow(dead_code)]

use std::sync::Arc;
use parking_lot::Mutex;
use xxhash_rust::xxh3::{xxh3_64, xxh3_128};

use crate::error::{PerlError, PerlResult};
use crate::value::StrykeValue;

/// Power-of-two-sized bit array. Indexing is `bit & (cap - 1)` so we
/// dodge a modulo every add/contains.
#[derive(Clone, Debug)]
struct BitArr {
    bits: Vec<u64>,
    /// 1 << log2_cap == bit_count
    log2_cap: u32,
}

impl BitArr {
    fn new(bit_count: usize) -> Self {
        // Round up to next power of two, min 64 bits.
        let bc = bit_count.max(64).next_power_of_two();
        let log2_cap = bc.trailing_zeros();
        Self {
            bits: vec![0u64; bc / 64],
            log2_cap,
        }
    }

    #[inline]
    fn cap_mask(&self) -> u64 {
        (1u64 << self.log2_cap) - 1
    }

    #[inline]
    fn set(&mut self, bit: u64) {
        let idx = (bit & self.cap_mask()) as usize;
        self.bits[idx >> 6] |= 1u64 << (idx & 63);
    }

    #[inline]
    fn get(&self, bit: u64) -> bool {
        let idx = (bit & self.cap_mask()) as usize;
        (self.bits[idx >> 6] >> (idx & 63)) & 1 == 1
    }

    fn count_set(&self) -> u64 {
        self.bits.iter().map(|w| w.count_ones() as u64).sum()
    }

    fn merge_or(&mut self, other: &BitArr) -> bool {
        if other.log2_cap != self.log2_cap {
            return false;
        }
        for (a, b) in self.bits.iter_mut().zip(other.bits.iter()) {
            *a |= *b;
        }
        true
    }
}

/// Classic Bloom filter with double-hashed k probes.
///
/// Construction takes desired capacity `n` and false-positive rate `p`;
/// we compute `m = ceil(-n ln p / (ln 2)^2)` bits and `k = ceil((m/n) ln 2)`
/// probes, matching the Wikipedia/Mitzenmacher formula. Bit count is
/// rounded up to a power of two so probe indexing is a mask, not a mod.
#[derive(Clone, Debug)]
pub struct BloomFilter {
    bits: BitArr,
    k: u32,
    capacity_hint: u64,
    fpr_hint: f64,
    inserted: u64,
}

impl BloomFilter {
    pub fn new(capacity: u64, fpr: f64) -> Self {
        let fpr = fpr.clamp(1e-12, 0.5);
        let n = capacity.max(1) as f64;
        let m = (-n * fpr.ln() / std::f64::consts::LN_2.powi(2)).ceil() as usize;
        let k = ((m as f64 / n) * std::f64::consts::LN_2).ceil().max(1.0) as u32;
        Self {
            bits: BitArr::new(m),
            k: k.min(32),
            capacity_hint: capacity,
            fpr_hint: fpr,
            inserted: 0,
        }
    }

    /// Two-hash derive: `h_i = h1 + i * h2` per Kirsch–Mitzenmacher.
    /// xxh3_128 gives us both halves from one pass.
    #[inline]
    fn probes(&self, key: &[u8]) -> impl Iterator<Item = u64> {
        let h = xxh3_128(key);
        let h1 = h as u64;
        let h2 = (h >> 64) as u64 | 1; // ensure nonzero increment
        let k = self.k;
        (0..k).map(move |i| h1.wrapping_add((i as u64).wrapping_mul(h2)))
    }

    pub fn add(&mut self, key: &[u8]) -> bool {
        let mut already_in = true;
        for p in self.probes(key) {
            if !self.bits.get(p) {
                already_in = false;
            }
            self.bits.set(p);
        }
        if !already_in {
            self.inserted += 1;
        }
        !already_in
    }

    pub fn contains(&self, key: &[u8]) -> bool {
        self.probes(key).all(|p| self.bits.get(p))
    }

    pub fn estimated_fpr(&self) -> f64 {
        // (1 - e^{-kn/m})^k
        let m = (1u64 << self.bits.log2_cap) as f64;
        let kn_over_m = self.k as f64 * self.inserted as f64 / m;
        (1.0 - (-kn_over_m).exp()).powi(self.k as i32)
    }

    pub fn inserted(&self) -> u64 {
        self.inserted
    }
    pub fn bit_count(&self) -> u64 {
        1u64 << self.bits.log2_cap
    }
    pub fn k(&self) -> u32 {
        self.k
    }
    pub fn capacity_hint(&self) -> u64 {
        self.capacity_hint
    }
    pub fn fpr_target(&self) -> f64 {
        self.fpr_hint
    }
    pub fn bits_set(&self) -> u64 {
        self.bits.count_set()
    }

    pub fn merge(&mut self, other: &BloomFilter) -> bool {
        if self.k != other.k || !self.bits.merge_or(&other.bits) {
            return false;
        }
        // Union of two sets has a count we can't recover exactly without
        // re-counting; track inserted as upper bound.
        self.inserted = self.inserted.saturating_add(other.inserted);
        true
    }

    pub fn clear(&mut self) {
        for w in self.bits.bits.iter_mut() {
            *w = 0;
        }
        self.inserted = 0;
    }

    /// Wire format: 8-byte magic + version + log2_cap + k + inserted +
    /// bit words. Versioned so future format changes don't silently
    /// load wrong data (CLAUDE.md endgame: "Bytecode and SQLite formats
    /// must be versioned and migration-safe").
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.bits.bits.len() * 8);
        out.extend_from_slice(b"STKBLOM\x01"); // magic + version 1
        out.extend_from_slice(&self.bits.log2_cap.to_le_bytes());
        out.extend_from_slice(&self.k.to_le_bytes());
        out.extend_from_slice(&self.inserted.to_le_bytes());
        out.extend_from_slice(&self.capacity_hint.to_le_bytes());
        out.extend_from_slice(&self.fpr_hint.to_le_bytes());
        for w in &self.bits.bits {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 || &bytes[..8] != b"STKBLOM\x01" {
            return None;
        }
        fn take<'b>(p: &mut usize, n: usize, bytes: &'b [u8]) -> Option<&'b [u8]> {
            if *p + n > bytes.len() {
                return None;
            }
            let s = &bytes[*p..*p + n];
            *p += n;
            Some(s)
        }
        let mut p = 8;
        let log2_cap = u32::from_le_bytes(take(&mut p, 4, bytes)?.try_into().ok()?);
        let k = u32::from_le_bytes(take(&mut p, 4, bytes)?.try_into().ok()?);
        let inserted = u64::from_le_bytes(take(&mut p, 8, bytes)?.try_into().ok()?);
        let capacity_hint = u64::from_le_bytes(take(&mut p, 8, bytes)?.try_into().ok()?);
        let fpr_hint = f64::from_le_bytes(take(&mut p, 8, bytes)?.try_into().ok()?);
        let nwords = (1usize << log2_cap) / 64;
        let mut bits = Vec::with_capacity(nwords);
        for _ in 0..nwords {
            bits.push(u64::from_le_bytes(take(&mut p, 8, bytes)?.try_into().ok()?));
        }
        if p != bytes.len() {
            return None;
        }
        Some(Self {
            bits: BitArr { bits, log2_cap },
            k,
            capacity_hint,
            fpr_hint,
            inserted,
        })
    }
}

/// HyperLogLog cardinality sketch with `m = 2^precision` 8-bit registers.
///
/// Replaces the deleted hashref-backed `hyperloglog_pp_*` and
/// `hyperloglog_*` slow impls which rebuilt a 2^p-element arrayref every
/// `_add` (16384 allocs/insert at the default precision=14 — unusable).
/// This impl mutates a single `Vec<u8>` in place; `_add` is two loads
/// and a conditional store, no allocation.
///
/// Estimator: standard alpha-corrected HLL with linear-counting small-
/// range fallback (HLL++ accuracy is ~1.6%/sqrt(m), so precision=14 gives
/// ~1.3% relative error — fine for typical analytics use). Full HLL++
/// bias-correction tables are deferred until someone hits a workload
/// that needs sub-1% accuracy.
#[derive(Clone, Debug)]
pub struct HllSketch {
    registers: Vec<u8>,
    precision: u32,
}

impl HllSketch {
    pub fn new(precision: u32) -> Self {
        let p = precision.clamp(4, 18);
        let m = 1usize << p;
        Self {
            registers: vec![0u8; m],
            precision: p,
        }
    }

    pub fn precision(&self) -> u32 {
        self.precision
    }
    pub fn registers_len(&self) -> usize {
        self.registers.len()
    }

    /// Single 64-bit xxh3 hash; top `precision` bits index the bucket,
    /// the remaining `64 - precision` bits give the position of the
    /// leftmost 1 (plus one). Standard HLL register update.
    pub fn add(&mut self, key: &[u8]) {
        let h = xxh3_64(key);
        let p = self.precision;
        let idx = (h >> (64 - p)) as usize;
        // Lower (64 - p) bits, sentinel the trailing bit to bound count at 64-p+1.
        let w = (h << p) | (1u64 << (p - 1));
        let leading = (w.leading_zeros() + 1) as u8;
        if leading > self.registers[idx] {
            self.registers[idx] = leading;
        }
    }

    /// Cardinality estimate.
    pub fn count(&self) -> f64 {
        let m = self.registers.len() as f64;
        let alpha = match self.registers.len() {
            16 => 0.673,
            32 => 0.697,
            64 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / m),
        };
        let mut sum = 0.0;
        let mut zeros = 0u32;
        for &r in &self.registers {
            if r == 0 {
                zeros += 1;
            }
            // 2^-r = ldexp(1.0, -r) — exact, no powi cost.
            sum += f64::from_bits(((1023u64 - r as u64) & 0x7FF) << 52);
        }
        let raw = alpha * m * m / sum;
        // Linear-counting small-range correction (zsh-style: keep simple).
        if raw <= 2.5 * m && zeros > 0 {
            m * (m / zeros as f64).ln()
        } else {
            raw
        }
    }

    pub fn merge(&mut self, other: &HllSketch) -> bool {
        if self.precision != other.precision {
            return false;
        }
        for (a, b) in self.registers.iter_mut().zip(other.registers.iter()) {
            if *b > *a {
                *a = *b;
            }
        }
        true
    }

    pub fn clear(&mut self) {
        for r in self.registers.iter_mut() {
            *r = 0;
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.registers.len());
        out.extend_from_slice(b"STKHLL\x00\x01");
        out.extend_from_slice(&self.precision.to_le_bytes());
        out.extend_from_slice(&self.registers);
        out
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 12 || &bytes[..8] != b"STKHLL\x00\x01" {
            return None;
        }
        let precision = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
        if !(4..=18).contains(&precision) {
            return None;
        }
        let m = 1usize << precision;
        if bytes.len() != 12 + m {
            return None;
        }
        Some(Self {
            registers: bytes[12..].to_vec(),
            precision,
        })
    }
}

/// Count-Min Sketch — sublinear frequency estimation.
///
/// `width` controls the over-estimation bound (`epsilon = e/width`),
/// `depth` controls the failure probability (`delta = 1/2^depth`).
/// Typical defaults `(2048, 5)` give epsilon ≈ 0.0013 with 97%
/// confidence. Counters are `u32`; values never decrement, so `_count`
/// can drift up under collisions but never returns less than the true
/// count.
#[derive(Clone, Debug)]
pub struct CmsSketch {
    counters: Vec<u32>,
    width: u32,
    depth: u32,
}

impl CmsSketch {
    pub fn new(width: u32, depth: u32) -> Self {
        let w = width.max(8);
        let d = depth.clamp(1, 32);
        Self {
            counters: vec![0u32; (w as usize) * (d as usize)],
            width: w,
            depth: d,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn depth(&self) -> u32 {
        self.depth
    }

    #[inline]
    fn probes(&self, key: &[u8]) -> impl Iterator<Item = usize> {
        let h = xxh3_128(key);
        let h1 = h as u64;
        let h2 = (h >> 64) as u64 | 1;
        let w = self.width;
        let d = self.depth;
        (0..d).map(move |i| {
            let combined = h1.wrapping_add((i as u64).wrapping_mul(h2));
            (i as usize) * w as usize + (combined % w as u64) as usize
        })
    }

    /// Add `count` occurrences of `key` (default 1 caller-side).
    pub fn add(&mut self, key: &[u8], count: u32) {
        for idx in self.probes(key) {
            self.counters[idx] = self.counters[idx].saturating_add(count);
        }
    }

    /// Estimate count of `key`: min over all `depth` rows. Always an
    /// upper bound on the true count.
    pub fn count(&self, key: &[u8]) -> u32 {
        let mut min = u32::MAX;
        for idx in self.probes(key) {
            let v = self.counters[idx];
            if v < min {
                min = v;
            }
        }
        if min == u32::MAX {
            0
        } else {
            min
        }
    }

    pub fn merge(&mut self, other: &CmsSketch) -> bool {
        if self.width != other.width || self.depth != other.depth {
            return false;
        }
        for (a, b) in self.counters.iter_mut().zip(other.counters.iter()) {
            *a = a.saturating_add(*b);
        }
        true
    }

    pub fn clear(&mut self) {
        for c in self.counters.iter_mut() {
            *c = 0;
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(16 + self.counters.len() * 4);
        out.extend_from_slice(b"STKCMS\x00\x01");
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.depth.to_le_bytes());
        for c in &self.counters {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 || &bytes[..8] != b"STKCMS\x00\x01" {
            return None;
        }
        let width = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
        let depth = u32::from_le_bytes(bytes[12..16].try_into().ok()?);
        let need = (width as usize) * (depth as usize) * 4 + 16;
        if bytes.len() != need {
            return None;
        }
        let mut counters = Vec::with_capacity((width as usize) * (depth as usize));
        for chunk in bytes[16..].chunks_exact(4) {
            counters.push(u32::from_le_bytes(chunk.try_into().ok()?));
        }
        Some(Self {
            counters,
            width,
            depth,
        })
    }
}

/// SpaceSaving (Metwally et al.) top-K heavy-hitters sketch.
///
/// Maintains exactly `k` (key, count) pairs in O(k) space. On overflow,
/// the minimum-count slot is replaced with the new key and its count
/// becomes `min + 1` — a strict upper-bound estimator for the new
/// arrival's true frequency. Each query is O(k log k) (sort by count).
///
/// Use case: streaming top-N analytics ("which 50 IPs sent the most
/// requests in the last hour?") on data too large to keep an exact
/// hashmap for.
#[derive(Clone, Debug)]
pub struct TopKSketch {
    /// (key, count, over_estimate_floor) — `over_estimate_floor` is the
    /// SpaceSaving error bound: the true count of the current key is at
    /// least `count - over_estimate_floor`.
    entries: std::collections::HashMap<Vec<u8>, (u64, u64)>,
    k: usize,
}

impl TopKSketch {
    pub fn new(k: usize) -> Self {
        Self {
            entries: std::collections::HashMap::with_capacity(k.max(1)),
            k: k.max(1),
        }
    }

    pub fn k(&self) -> usize {
        self.k
    }
    pub fn size(&self) -> usize {
        self.entries.len()
    }

    pub fn add(&mut self, key: &[u8]) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.0 += 1;
            return;
        }
        if self.entries.len() < self.k {
            self.entries.insert(key.to_vec(), (1, 0));
            return;
        }
        // Find the entry with the smallest count; that slot gets evicted.
        // The new key inherits `min_count + 1` and an error floor equal to
        // the evicted slot's count.
        let (evict_key, min_count) = self
            .entries
            .iter()
            .min_by_key(|(_, (c, _))| *c)
            .map(|(k, (c, _))| (k.clone(), *c))
            .expect("entries non-empty at this point");
        self.entries.remove(&evict_key);
        self.entries
            .insert(key.to_vec(), (min_count + 1, min_count));
    }

    /// Top-N entries, sorted by count descending. Each entry: `(key, count,
    /// error_floor)`. Truth lies in `[count - error_floor, count]`.
    pub fn heavies(&self, n: usize) -> Vec<(Vec<u8>, u64, u64)> {
        let mut all: Vec<(Vec<u8>, u64, u64)> = self
            .entries
            .iter()
            .map(|(k, (c, e))| (k.clone(), *c, *e))
            .collect();
        all.sort_by(|a, b| b.1.cmp(&a.1));
        all.truncate(n);
        all
    }

    /// Get the (possibly over-counted) frequency of `key`. Returns `0`
    /// when the key isn't in the sketch (i.e. wasn't heavy enough to
    /// survive eviction).
    pub fn count(&self, key: &[u8]) -> u64 {
        self.entries.get(key).map(|(c, _)| *c).unwrap_or(0)
    }

    pub fn merge(&mut self, other: &TopKSketch) -> bool {
        // Merging two SpaceSaving sketches: drop into self by re-inserting
        // each (key, count) from `other`, treating count as a virtual
        // batch of `count` arrivals. Simpler than the original paper's
        // merge but correct: each replay just re-runs the standard
        // online algorithm. Cost: O(total_count) — heavy for large
        // sketches. Document; callers can use bigger k to avoid this.
        let mut sorted: Vec<_> = other.entries.iter().collect();
        sorted.sort_by(|a, b| b.1.0.cmp(&a.1.0)); // largest first
        for (key, (count, _)) in sorted {
            for _ in 0..*count {
                self.add(key);
            }
        }
        true
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn serialize(&self) -> Vec<u8> {
        // 8-byte magic + version, 8-byte k, 8-byte entry count, then per
        // entry: 4-byte key-len + key bytes + 8-byte count + 8-byte error.
        let mut out = Vec::with_capacity(24 + self.entries.len() * 32);
        out.extend_from_slice(b"STKTOP\x00\x01");
        out.extend_from_slice(&(self.k as u64).to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u64).to_le_bytes());
        for (key, (count, err)) in &self.entries {
            out.extend_from_slice(&(key.len() as u32).to_le_bytes());
            out.extend_from_slice(key);
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&err.to_le_bytes());
        }
        out
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 24 || &bytes[..8] != b"STKTOP\x00\x01" {
            return None;
        }
        let k = u64::from_le_bytes(bytes[8..16].try_into().ok()?) as usize;
        let n = u64::from_le_bytes(bytes[16..24].try_into().ok()?) as usize;
        if k == 0 || n > k {
            return None;
        }
        let mut entries = std::collections::HashMap::with_capacity(k);
        let mut p = 24;
        for _ in 0..n {
            if p + 4 > bytes.len() {
                return None;
            }
            let klen = u32::from_le_bytes(bytes[p..p + 4].try_into().ok()?) as usize;
            p += 4;
            if p + klen + 16 > bytes.len() {
                return None;
            }
            let key = bytes[p..p + klen].to_vec();
            p += klen;
            let count = u64::from_le_bytes(bytes[p..p + 8].try_into().ok()?);
            p += 8;
            let err = u64::from_le_bytes(bytes[p..p + 8].try_into().ok()?);
            p += 8;
            entries.insert(key, (count, err));
        }
        if p != bytes.len() {
            return None;
        }
        Some(Self { entries, k })
    }
}

/// t-digest streaming-quantile sketch (Dunning).
///
/// Pure-Rust impl from the `tdigest` crate. The crate's `TDigest` type
/// is immutable (each merge returns a fresh value), so we wrap it with
/// a pending buffer + `flush-on-query` so per-`add` is amortized O(1)
/// instead of O(n) per insert.
///
/// Use case: streaming quantiles ("what's the 99th-percentile latency
/// over the last hour?") with mergeable digests and bounded memory.
/// Accuracy is best at the extremes (p1, p99) where it matters most for
/// SLO monitoring.
#[derive(Clone, Debug)]
pub struct TDigestSketch {
    digest: tdigest::TDigest,
    pending: Vec<f64>,
}

impl TDigestSketch {
    pub fn new(compression: usize) -> Self {
        Self {
            digest: tdigest::TDigest::new_with_size(compression.max(20)),
            pending: Vec::new(),
        }
    }

    fn flush(&mut self) {
        if !self.pending.is_empty() {
            let p = std::mem::take(&mut self.pending);
            self.digest = self.digest.clone().merge_unsorted(p);
        }
    }

    pub fn add(&mut self, value: f64) {
        if value.is_finite() {
            self.pending.push(value);
            if self.pending.len() >= 100 {
                self.flush();
            }
        }
    }

    pub fn quantile(&mut self, q: f64) -> f64 {
        self.flush();
        if self.digest.is_empty() {
            return f64::NAN;
        }
        self.digest.estimate_quantile(q.clamp(0.0, 1.0))
    }

    pub fn count(&mut self) -> u64 {
        self.flush();
        self.digest.count() as u64
    }

    pub fn min(&mut self) -> f64 {
        self.flush();
        self.digest.min()
    }

    pub fn max(&mut self) -> f64 {
        self.flush();
        self.digest.max()
    }

    pub fn sum(&mut self) -> f64 {
        self.flush();
        self.digest.sum()
    }

    pub fn mean(&mut self) -> f64 {
        self.flush();
        if self.digest.is_empty() {
            f64::NAN
        } else {
            self.digest.mean()
        }
    }

    pub fn merge(&mut self, other: &mut TDigestSketch) {
        self.flush();
        other.flush();
        self.digest = tdigest::TDigest::merge_digests(vec![
            self.digest.clone(),
            other.digest.clone(),
        ]);
    }

    pub fn clear(&mut self) {
        self.digest = tdigest::TDigest::new_with_size(self.digest.max_size());
        self.pending.clear();
    }

    pub fn compression(&self) -> usize {
        self.digest.max_size()
    }

    pub fn serialize(&mut self) -> Vec<u8> {
        self.flush();
        let mut out = Vec::new();
        out.extend_from_slice(b"STKTDG\x00\x01");
        let json = serde_json::to_vec(&self.digest).unwrap_or_default();
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&json);
        out
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 12 || &bytes[..8] != b"STKTDG\x00\x01" {
            return None;
        }
        let n = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
        if bytes.len() != 12 + n {
            return None;
        }
        let digest: tdigest::TDigest = serde_json::from_slice(&bytes[12..]).ok()?;
        Some(Self {
            digest,
            pending: Vec::new(),
        })
    }
}

/// Roaring Bitmap — compressed bitset over `u32`.
///
/// Apache-licensed `roaring` crate (used by quickwit, tantivy, lucene-rs).
/// O(1) set / contains, O(n/run-length) for set operations. Compresses
/// dense ranges as runs and sparse blocks as sorted arrays — typically
/// 10-100× smaller than `HashSet<u32>` for natural datasets.
#[derive(Clone, Debug)]
pub struct RoaringBitmapSketch {
    inner: roaring::RoaringBitmap,
}

impl RoaringBitmapSketch {
    pub fn new() -> Self {
        Self {
            inner: roaring::RoaringBitmap::new(),
        }
    }

    pub fn from_iter<I: IntoIterator<Item = u32>>(items: I) -> Self {
        Self {
            inner: items.into_iter().collect(),
        }
    }

    pub fn add(&mut self, v: u32) -> bool {
        self.inner.insert(v)
    }
    pub fn remove(&mut self, v: u32) -> bool {
        self.inner.remove(v)
    }
    pub fn contains(&self, v: u32) -> bool {
        self.inner.contains(v)
    }
    pub fn len(&self) -> u64 {
        self.inner.len()
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn min(&self) -> Option<u32> {
        self.inner.min()
    }
    pub fn max(&self) -> Option<u32> {
        self.inner.max()
    }
    pub fn to_vec(&self) -> Vec<u32> {
        self.inner.iter().collect()
    }
    pub fn rank(&self, v: u32) -> u64 {
        self.inner.rank(v)
    }

    pub fn union_with(&mut self, other: &RoaringBitmapSketch) {
        self.inner |= &other.inner;
    }
    pub fn intersect_with(&mut self, other: &RoaringBitmapSketch) {
        self.inner &= &other.inner;
    }
    pub fn xor_with(&mut self, other: &RoaringBitmapSketch) {
        self.inner ^= &other.inner;
    }
    pub fn andnot_with(&mut self, other: &RoaringBitmapSketch) {
        self.inner -= &other.inner;
    }
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.inner.serialized_size());
        out.extend_from_slice(b"STKRB\x00\x00\x01");
        let _ = self.inner.serialize_into(&mut out);
        out
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 || &bytes[..8] != b"STKRB\x00\x00\x01" {
            return None;
        }
        let inner = roaring::RoaringBitmap::deserialize_from(&bytes[8..]).ok()?;
        Some(Self { inner })
    }
}

impl Default for RoaringBitmapSketch {
    fn default() -> Self {
        Self::new()
    }
}

// ── Builtin handlers ─────────────────────────────────────────────────────

fn bf_lock_arg(v: &StrykeValue, fname: &str, line: usize) -> PerlResult<Arc<Mutex<BloomFilter>>> {
    v.as_bloom_filter()
        .ok_or_else(|| PerlError::runtime(format!("{fname}: expected BloomFilter operand"), line))
}

fn key_bytes(v: &StrykeValue) -> Vec<u8> {
    // Honor explicit BYTES first (avoids round-tripping binary through UTF-8 lossy);
    // else use the Display form so integers, floats, and strings all hash consistently.
    if let Some(b) = v.as_bytes_arc() {
        return (*b).clone();
    }
    v.to_string().into_bytes()
}

/// `bloom_filter(CAPACITY, FPR)` — construct a Bloom filter sized for
/// `CAPACITY` distinct items with target false-positive rate `FPR`
/// (default `0.01`). Bit count is `ceil(-n ln p / (ln 2)^2)` rounded up
/// to a power of two; probe count is `k = ceil((m/n) ln 2)`, capped at
/// 32. Capacity must be > 0; FPR is clamped to `[1e-12, 0.5]`.
pub(crate) fn builtin_bloom_filter(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let capacity = args.first().map(|v| v.to_int()).unwrap_or(1000).max(1) as u64;
    let fpr = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    if !fpr.is_finite() || fpr <= 0.0 || fpr >= 1.0 {
        return Err(PerlError::runtime(
            "bloom_filter: FPR must be in (0, 1)",
            line,
        ));
    }
    let b = BloomFilter::new(capacity, fpr);
    Ok(StrykeValue::bloom_filter(Arc::new(Mutex::new(b))))
}

/// `bloom_add(BF, KEY)` — insert `KEY` into the filter. Returns `1` if
/// the key was newly inserted (k bits flipped from 0→1), `0` if every
/// probe already hit a set bit (key already present, or false positive).
pub(crate) fn builtin_bloom_add(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let bf = bf_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "bloom_add", line)?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let newly = bf.lock().add(&key);
    Ok(StrykeValue::integer(if newly { 1 } else { 0 }))
}

/// `bloom_contains(BF, KEY)` — `1` if `KEY` may be present (no false
/// negatives), `0` if definitely absent.
pub(crate) fn builtin_bloom_contains(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let bf = bf_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "bloom_contains",
        line,
    )?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let hit = bf.lock().contains(&key);
    Ok(StrykeValue::integer(if hit { 1 } else { 0 }))
}

/// `bloom_len(BF)` — items inserted so far (newly-added count; collisions
/// don't increment). Upper-bound-ish after merges.
pub(crate) fn builtin_bloom_len(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let bf = bf_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "bloom_len", line)?;
    let n = bf.lock().inserted();
    Ok(StrykeValue::integer(n as i64))
}

/// `bloom_clear(BF)` — zero the bit array and reset the insertion counter.
/// Returns the same `BF` for chaining.
pub(crate) fn builtin_bloom_clear(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let bf_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let bf = bf_lock_arg(&bf_v, "bloom_clear", line)?;
    bf.lock().clear();
    Ok(bf_v)
}

/// `bloom_merge(BF, OTHER)` — union with another filter of identical
/// geometry (same bit count and `k`). Returns `1` on success, `0` if
/// geometries differ.
pub(crate) fn builtin_bloom_merge(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let bf = bf_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "bloom_merge", line)?;
    let other = bf_lock_arg(
        args.get(1).unwrap_or(&StrykeValue::UNDEF),
        "bloom_merge",
        line,
    )?;
    let ok = {
        let other_g = other.lock();
        bf.lock().merge(&other_g)
    };
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

/// `bloom_fpr(BF)` — estimated current false-positive rate given the
/// running insertion count. Useful for "is this filter saturated?" checks
/// — when it exceeds your target FPR, rebuild with a larger capacity.
pub(crate) fn builtin_bloom_fpr(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let bf = bf_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "bloom_fpr", line)?;
    let fpr = bf.lock().estimated_fpr();
    Ok(StrykeValue::float(fpr))
}

/// `bloom_bits(BF)` — total bit count of the underlying array (always a
/// power of two ≥ 64).
pub(crate) fn builtin_bloom_bits(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let bf = bf_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "bloom_bits", line)?;
    let n = bf.lock().bit_count();
    Ok(StrykeValue::integer(n as i64))
}

/// `bloom_serialize(BF)` — versioned wire format. Pair with
/// `bloom_deserialize` to persist filters across runs / processes /
/// machines without re-inserting every key.
pub(crate) fn builtin_bloom_serialize(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let bf = bf_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "bloom_serialize",
        line,
    )?;
    let bytes = bf.lock().serialize();
    Ok(StrykeValue::bytes(Arc::new(bytes)))
}

/// `bloom_deserialize(BYTES)` — load a filter from `bloom_serialize`
/// output. Returns `undef` on format mismatch (wrong magic, truncated
/// payload, or future version).
// ── HLL builtins ─────────────────────────────────────────────────────────

fn hll_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> PerlResult<Arc<Mutex<HllSketch>>> {
    v.as_hll_sketch()
        .ok_or_else(|| PerlError::runtime(format!("{fname}: expected HllSketch operand"), line))
}

/// `hll(PRECISION=14)` / `hyperloglog(PRECISION)` — construct a HyperLogLog
/// cardinality sketch with `2^precision` 8-bit registers. Precision is
/// clamped to `[4, 18]`; typical workloads use 10–14 (`2^14 = 16384`
/// registers, ~1.3% relative error, 16KB of state).
pub(crate) fn builtin_hll(args: &[StrykeValue], _line: usize) -> PerlResult<StrykeValue> {
    let precision = args.first().map(|v| v.to_int()).unwrap_or(14) as u32;
    let h = HllSketch::new(precision);
    Ok(StrykeValue::hll_sketch(Arc::new(Mutex::new(h))))
}

/// `hll_add(HLL, KEY)` — fold `KEY` into the sketch. Returns the same
/// HLL for chaining.
pub(crate) fn builtin_hll_add(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let hll_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let h = hll_lock_arg(&hll_v, "hll_add", line)?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    h.lock().add(&key);
    Ok(hll_v)
}

/// `hll_count(HLL)` — estimated number of distinct items inserted.
pub(crate) fn builtin_hll_count(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let h = hll_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "hll_count", line)?;
    let n = h.lock().count();
    Ok(StrykeValue::float(n))
}

/// `hll_merge(HLL, OTHER)` — union with another HLL of identical precision.
/// Returns `1` on success, `0` on precision mismatch.
pub(crate) fn builtin_hll_merge(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let h = hll_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "hll_merge", line)?;
    let o = hll_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "hll_merge", line)?;
    let ok = {
        let og = o.lock();
        h.lock().merge(&og)
    };
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

/// `hll_clear(HLL)` — zero every register. Returns the same HLL.
pub(crate) fn builtin_hll_clear(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let hll_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let h = hll_lock_arg(&hll_v, "hll_clear", line)?;
    h.lock().clear();
    Ok(hll_v)
}

/// `hll_serialize(HLL)` — versioned wire format (12-byte header +
/// register vec).
pub(crate) fn builtin_hll_serialize(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let h = hll_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "hll_serialize",
        line,
    )?;
    let bytes = h.lock().serialize();
    Ok(StrykeValue::bytes(Arc::new(bytes)))
}

/// `hll_deserialize(BYTES)` — restore an HLL from `hll_serialize` output.
/// Returns `undef` on format mismatch.
pub(crate) fn builtin_hll_deserialize(
    args: &[StrykeValue],
    _line: usize,
) -> PerlResult<StrykeValue> {
    let Some(v) = args.first() else {
        return Ok(StrykeValue::UNDEF);
    };
    let bytes: Vec<u8> = if let Some(b) = v.as_bytes_arc() {
        (*b).clone()
    } else {
        v.to_string().into_bytes()
    };
    match HllSketch::deserialize(&bytes) {
        Some(h) => Ok(StrykeValue::hll_sketch(Arc::new(Mutex::new(h)))),
        None => Ok(StrykeValue::UNDEF),
    }
}

/// `hll_precision(HLL)` — the `precision` parameter the sketch was
/// constructed with.
pub(crate) fn builtin_hll_precision(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let h = hll_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "hll_precision",
        line,
    )?;
    let p = h.lock().precision();
    Ok(StrykeValue::integer(p as i64))
}

// ── CMS builtins ─────────────────────────────────────────────────────────

fn cms_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> PerlResult<Arc<Mutex<CmsSketch>>> {
    v.as_cms_sketch()
        .ok_or_else(|| PerlError::runtime(format!("{fname}: expected CmsSketch operand"), line))
}

/// `count_min_sketch(WIDTH=2048, DEPTH=5)` / `cms(W, D)` — construct a
/// Count-Min frequency sketch. Defaults give epsilon ≈ 0.0013 (1.3‰
/// over-estimation upper bound) with delta ≈ 0.03 (3% failure
/// probability per query).
pub(crate) fn builtin_cms(args: &[StrykeValue], _line: usize) -> PerlResult<StrykeValue> {
    let width = args.first().map(|v| v.to_int().max(8)).unwrap_or(2048) as u32;
    let depth = args.get(1).map(|v| v.to_int().max(1)).unwrap_or(5) as u32;
    Ok(StrykeValue::cms_sketch(Arc::new(Mutex::new(CmsSketch::new(
        width, depth,
    )))))
}

/// `cms_add(CMS, KEY, COUNT=1)` — add `COUNT` occurrences of `KEY`.
pub(crate) fn builtin_cms_add(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let cms_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let c = cms_lock_arg(&cms_v, "cms_add", line)?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let count = args.get(2).map(|v| v.to_int().max(1)).unwrap_or(1) as u32;
    c.lock().add(&key, count);
    Ok(cms_v)
}

/// `cms_count(CMS, KEY)` — estimated count of `KEY`. Always an upper
/// bound on the true count; never under-reports.
pub(crate) fn builtin_cms_count(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let c = cms_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "cms_count", line)?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let n = c.lock().count(&key);
    Ok(StrykeValue::integer(n as i64))
}

/// `cms_merge(CMS, OTHER)` — sum counters from `OTHER` into `CMS`
/// (geometries must match: same width and depth).
pub(crate) fn builtin_cms_merge(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let c = cms_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "cms_merge", line)?;
    let o = cms_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "cms_merge", line)?;
    let ok = {
        let og = o.lock();
        c.lock().merge(&og)
    };
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

/// `cms_clear(CMS)` — zero all counters.
pub(crate) fn builtin_cms_clear(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let cms_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let c = cms_lock_arg(&cms_v, "cms_clear", line)?;
    c.lock().clear();
    Ok(cms_v)
}

pub(crate) fn builtin_cms_serialize(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let c = cms_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "cms_serialize",
        line,
    )?;
    let bytes = c.lock().serialize();
    Ok(StrykeValue::bytes(Arc::new(bytes)))
}

pub(crate) fn builtin_cms_deserialize(
    args: &[StrykeValue],
    _line: usize,
) -> PerlResult<StrykeValue> {
    let Some(v) = args.first() else {
        return Ok(StrykeValue::UNDEF);
    };
    let bytes: Vec<u8> = if let Some(b) = v.as_bytes_arc() {
        (*b).clone()
    } else {
        v.to_string().into_bytes()
    };
    match CmsSketch::deserialize(&bytes) {
        Some(c) => Ok(StrykeValue::cms_sketch(Arc::new(Mutex::new(c)))),
        None => Ok(StrykeValue::UNDEF),
    }
}

// ── TopK builtins ────────────────────────────────────────────────────────

fn topk_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> PerlResult<Arc<Mutex<TopKSketch>>> {
    v.as_topk_sketch()
        .ok_or_else(|| PerlError::runtime(format!("{fname}: expected TopKSketch operand"), line))
}

/// `topk(K=10)` / `top_k_sketch(K)` — construct a SpaceSaving top-K
/// heavy-hitters sketch tracking at most `K` distinct keys with O(K)
/// space.
pub(crate) fn builtin_topk(args: &[StrykeValue], _line: usize) -> PerlResult<StrykeValue> {
    let k = args.first().map(|v| v.to_int().max(1)).unwrap_or(10) as usize;
    Ok(StrykeValue::topk_sketch(Arc::new(Mutex::new(TopKSketch::new(k)))))
}

/// `topk_add(TOPK, KEY)` — observe one occurrence of `KEY`.
pub(crate) fn builtin_topk_add(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let topk_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let t = topk_lock_arg(&topk_v, "topk_add", line)?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    t.lock().add(&key);
    Ok(topk_v)
}

/// `topk_heavies(TOPK, N=K)` — top `N` entries by frequency, sorted
/// descending. Returns an array of arrayrefs `[key, count, error_floor]`
/// — truth lies in `[count - error_floor, count]`.
pub(crate) fn builtin_topk_heavies(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let t = topk_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "topk_heavies",
        line,
    )?;
    let n = args
        .get(1)
        .map(|v| v.to_int().max(0) as usize)
        .unwrap_or_else(|| t.lock().k());
    let rows = t.lock().heavies(n);
    let out: Vec<StrykeValue> = rows
        .into_iter()
        .map(|(k, c, e)| {
            // Strings if the bytes are valid UTF-8; else bytes.
            let k_val = match String::from_utf8(k.clone()) {
                Ok(s) => StrykeValue::string(s),
                Err(_) => StrykeValue::bytes(Arc::new(k)),
            };
            StrykeValue::array_ref(Arc::new(parking_lot::RwLock::new(vec![
                k_val,
                StrykeValue::integer(c as i64),
                StrykeValue::integer(e as i64),
            ])))
        })
        .collect();
    Ok(StrykeValue::array(out))
}

/// `topk_count(TOPK, KEY)` — estimated count of `KEY`. `0` if the key
/// isn't currently tracked (i.e. wasn't heavy enough to survive).
pub(crate) fn builtin_topk_count(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let t = topk_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "topk_count",
        line,
    )?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let n = t.lock().count(&key);
    Ok(StrykeValue::integer(n as i64))
}

/// `topk_size(TOPK)` — current number of tracked entries (`<= K`).
pub(crate) fn builtin_topk_size(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let t = topk_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "topk_size",
        line,
    )?;
    let n = t.lock().size();
    Ok(StrykeValue::integer(n as i64))
}

/// `topk_merge(TOPK, OTHER)` — fold `OTHER`'s observations into `TOPK`.
/// Replays each `(key, count)` pair through the standard online update;
/// cost is O(sum_of_counts), so prefer larger K for heavy workloads.
/// Returns `1`.
pub(crate) fn builtin_topk_merge(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let t = topk_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "topk_merge", line)?;
    let o = topk_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "topk_merge", line)?;
    let ok = {
        let og = o.lock();
        t.lock().merge(&og)
    };
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

/// `topk_clear(TOPK)` — drop all tracked keys.
pub(crate) fn builtin_topk_clear(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let topk_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let t = topk_lock_arg(&topk_v, "topk_clear", line)?;
    t.lock().clear();
    Ok(topk_v)
}

pub(crate) fn builtin_topk_serialize(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let t = topk_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "topk_serialize",
        line,
    )?;
    let bytes = t.lock().serialize();
    Ok(StrykeValue::bytes(Arc::new(bytes)))
}

pub(crate) fn builtin_topk_deserialize(
    args: &[StrykeValue],
    _line: usize,
) -> PerlResult<StrykeValue> {
    let Some(v) = args.first() else {
        return Ok(StrykeValue::UNDEF);
    };
    let bytes: Vec<u8> = if let Some(b) = v.as_bytes_arc() {
        (*b).clone()
    } else {
        v.to_string().into_bytes()
    };
    match TopKSketch::deserialize(&bytes) {
        Some(t) => Ok(StrykeValue::topk_sketch(Arc::new(Mutex::new(t)))),
        None => Ok(StrykeValue::UNDEF),
    }
}

// ── t-digest builtins ────────────────────────────────────────────────────

fn td_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> PerlResult<Arc<Mutex<TDigestSketch>>> {
    v.as_tdigest_sketch()
        .ok_or_else(|| PerlError::runtime(format!("{fname}: expected TDigestSketch operand"), line))
}

/// `t_digest(COMPRESSION=100)` / `td(C)` — streaming-quantile sketch.
/// Larger compression → more centroids, more accuracy, more memory
/// (linear). Default `100` gives ~1% error at p50 / ~5% at p99 on typical
/// data with O(100) bytes of state.
pub(crate) fn builtin_t_digest(args: &[StrykeValue], _line: usize) -> PerlResult<StrykeValue> {
    let c = args.first().map(|v| v.to_int().max(20)).unwrap_or(100) as usize;
    Ok(StrykeValue::tdigest_sketch(Arc::new(Mutex::new(TDigestSketch::new(c)))))
}

pub(crate) fn builtin_td_add(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let td_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let t = td_lock_arg(&td_v, "td_add", line)?;
    let val = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    t.lock().add(val);
    Ok(td_v)
}

pub(crate) fn builtin_td_quantile(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_quantile", line)?;
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let v = t.lock().quantile(q);
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_count(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_count", line)?;
    let n = t.lock().count();
    Ok(StrykeValue::integer(n as i64))
}

pub(crate) fn builtin_td_min(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_min", line)?;
    let v = t.lock().min();
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_max(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_max", line)?;
    let v = t.lock().max();
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_sum(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_sum", line)?;
    let v = t.lock().sum();
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_mean(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_mean", line)?;
    let v = t.lock().mean();
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_merge(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_merge", line)?;
    let o = td_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "td_merge", line)?;
    {
        let mut og = o.lock();
        t.lock().merge(&mut og);
    }
    Ok(StrykeValue::integer(1))
}

pub(crate) fn builtin_td_clear(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let td_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let t = td_lock_arg(&td_v, "td_clear", line)?;
    t.lock().clear();
    Ok(td_v)
}

pub(crate) fn builtin_td_serialize(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_serialize", line)?;
    let bytes = t.lock().serialize();
    Ok(StrykeValue::bytes(Arc::new(bytes)))
}

pub(crate) fn builtin_td_deserialize(
    args: &[StrykeValue],
    _line: usize,
) -> PerlResult<StrykeValue> {
    let Some(v) = args.first() else {
        return Ok(StrykeValue::UNDEF);
    };
    let bytes: Vec<u8> = if let Some(b) = v.as_bytes_arc() {
        (*b).clone()
    } else {
        v.to_string().into_bytes()
    };
    match TDigestSketch::deserialize(&bytes) {
        Some(t) => Ok(StrykeValue::tdigest_sketch(Arc::new(Mutex::new(t)))),
        None => Ok(StrykeValue::UNDEF),
    }
}

// ── Roaring Bitmap builtins ──────────────────────────────────────────────

fn rb_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> PerlResult<Arc<Mutex<RoaringBitmapSketch>>> {
    v.as_roaring_bitmap()
        .ok_or_else(|| PerlError::runtime(format!("{fname}: expected RoaringBitmap operand"), line))
}

fn value_to_u32(v: &StrykeValue) -> u32 {
    let n = v.to_int();
    n.clamp(0, u32::MAX as i64) as u32
}

/// `roaring(U32...)` / `roaring_bitmap(LIST)` — construct a Roaring
/// bitmap. Any args are inserted as `u32` (clamped to `[0, 2^32-1]`).
pub(crate) fn builtin_roaring(args: &[StrykeValue], _line: usize) -> PerlResult<StrykeValue> {
    let mut rb = RoaringBitmapSketch::new();
    for a in args {
        if let Some(vec) = a.as_array_vec() {
            for v in vec {
                rb.add(value_to_u32(&v));
            }
        } else if let Some(arr) = a.as_array_ref() {
            for v in arr.read().iter() {
                rb.add(value_to_u32(v));
            }
        } else {
            rb.add(value_to_u32(a));
        }
    }
    Ok(StrykeValue::roaring_bitmap(Arc::new(Mutex::new(rb))))
}

pub(crate) fn builtin_rb_add(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let rb = rb_lock_arg(&rb_v, "rb_add", line)?;
    let mut g = rb.lock();
    let mut added = 0i64;
    for a in &args[1..] {
        if let Some(vec) = a.as_array_vec() {
            for v in vec {
                if g.add(value_to_u32(&v)) {
                    added += 1;
                }
            }
        } else if let Some(arr) = a.as_array_ref() {
            for v in arr.read().iter() {
                if g.add(value_to_u32(v)) {
                    added += 1;
                }
            }
        } else if g.add(value_to_u32(a)) {
            added += 1;
        }
    }
    Ok(StrykeValue::integer(added))
}

pub(crate) fn builtin_rb_remove(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_remove", line)?;
    let mut g = rb.lock();
    let mut removed = 0i64;
    for a in &args[1..] {
        if g.remove(value_to_u32(a)) {
            removed += 1;
        }
    }
    Ok(StrykeValue::integer(removed))
}

pub(crate) fn builtin_rb_contains(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_contains", line)?;
    let v = args.get(1).map(value_to_u32).unwrap_or(0);
    let hit = rb.lock().contains(v);
    Ok(StrykeValue::integer(if hit { 1 } else { 0 }))
}

pub(crate) fn builtin_rb_len(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_len", line)?;
    let n = rb.lock().len();
    Ok(StrykeValue::integer(n as i64))
}

pub(crate) fn builtin_rb_min(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_min", line)?;
    let m = rb.lock().min();
    Ok(m.map(|v| StrykeValue::integer(v as i64))
        .unwrap_or(StrykeValue::UNDEF))
}

pub(crate) fn builtin_rb_max(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_max", line)?;
    let m = rb.lock().max();
    Ok(m.map(|v| StrykeValue::integer(v as i64))
        .unwrap_or(StrykeValue::UNDEF))
}

pub(crate) fn builtin_rb_to_array(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_to_array", line)?;
    let vec = rb.lock().to_vec();
    Ok(StrykeValue::array(
        vec.into_iter().map(|v| StrykeValue::integer(v as i64)).collect(),
    ))
}

pub(crate) fn builtin_rb_rank(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_rank", line)?;
    let v = args.get(1).map(value_to_u32).unwrap_or(0);
    let r = rb.lock().rank(v);
    Ok(StrykeValue::integer(r as i64))
}

pub(crate) fn builtin_rb_or(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = rb_lock_arg(&rb_v, "rb_or", line)?;
    let b = rb_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "rb_or", line)?;
    {
        let bg = b.lock();
        a.lock().union_with(&bg);
    }
    Ok(rb_v)
}

pub(crate) fn builtin_rb_and(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = rb_lock_arg(&rb_v, "rb_and", line)?;
    let b = rb_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "rb_and", line)?;
    {
        let bg = b.lock();
        a.lock().intersect_with(&bg);
    }
    Ok(rb_v)
}

pub(crate) fn builtin_rb_xor(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = rb_lock_arg(&rb_v, "rb_xor", line)?;
    let b = rb_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "rb_xor", line)?;
    {
        let bg = b.lock();
        a.lock().xor_with(&bg);
    }
    Ok(rb_v)
}

pub(crate) fn builtin_rb_andnot(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = rb_lock_arg(&rb_v, "rb_andnot", line)?;
    let b = rb_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "rb_andnot", line)?;
    {
        let bg = b.lock();
        a.lock().andnot_with(&bg);
    }
    Ok(rb_v)
}

pub(crate) fn builtin_rb_clear(args: &[StrykeValue], line: usize) -> PerlResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let rb = rb_lock_arg(&rb_v, "rb_clear", line)?;
    rb.lock().clear();
    Ok(rb_v)
}

pub(crate) fn builtin_rb_serialize(
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_serialize", line)?;
    let bytes = rb.lock().serialize();
    Ok(StrykeValue::bytes(Arc::new(bytes)))
}

pub(crate) fn builtin_rb_deserialize(
    args: &[StrykeValue],
    _line: usize,
) -> PerlResult<StrykeValue> {
    let Some(v) = args.first() else {
        return Ok(StrykeValue::UNDEF);
    };
    let bytes: Vec<u8> = if let Some(b) = v.as_bytes_arc() {
        (*b).clone()
    } else {
        v.to_string().into_bytes()
    };
    match RoaringBitmapSketch::deserialize(&bytes) {
        Some(rb) => Ok(StrykeValue::roaring_bitmap(Arc::new(Mutex::new(rb)))),
        None => Ok(StrykeValue::UNDEF),
    }
}

pub(crate) fn builtin_bloom_deserialize(
    args: &[StrykeValue],
    _line: usize,
) -> PerlResult<StrykeValue> {
    let Some(v) = args.first() else {
        return Ok(StrykeValue::UNDEF);
    };
    // Accept BYTES first (the canonical input from `bloom_serialize`); fall back
    // to the Display form so users can pass a string they read from disk
    // verbatim without explicit byte conversion. Invalid payloads return
    // `undef` rather than throwing — caller can do `defined()` to detect.
    let bytes: Vec<u8> = if let Some(b) = v.as_bytes_arc() {
        (*b).clone()
    } else {
        v.to_string().into_bytes()
    };
    match BloomFilter::deserialize(&bytes) {
        Some(b) => Ok(StrykeValue::bloom_filter(Arc::new(Mutex::new(b)))),
        None => Ok(StrykeValue::UNDEF),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloom_basic_membership() {
        let mut b = BloomFilter::new(1000, 0.01);
        b.add(b"alice");
        b.add(b"bob");
        assert!(b.contains(b"alice"));
        assert!(b.contains(b"bob"));
        assert!(!b.contains(b"carol"));
        assert_eq!(b.inserted(), 2);
    }

    #[test]
    fn bloom_fpr_within_target() {
        // Insert exactly capacity items, check FPR stays within 2x target.
        let target_fpr = 0.01;
        let n = 10_000;
        let mut b = BloomFilter::new(n, target_fpr);
        for i in 0..n {
            b.add(format!("inserted_{i}").as_bytes());
        }
        // Probe 10k unseen keys.
        let probes = 10_000u64;
        let mut fp = 0u64;
        for i in 0..probes {
            if b.contains(format!("probe_{i}").as_bytes()) {
                fp += 1;
            }
        }
        let observed = fp as f64 / probes as f64;
        assert!(
            observed < target_fpr * 2.5,
            "observed FPR {} >> target {}",
            observed,
            target_fpr
        );
    }

    #[test]
    fn bloom_no_false_negatives() {
        let mut b = BloomFilter::new(10_000, 0.001);
        let items: Vec<String> = (0..5_000).map(|i| format!("k{i}")).collect();
        for s in &items {
            b.add(s.as_bytes());
        }
        for s in &items {
            assert!(b.contains(s.as_bytes()), "missing inserted item {s}");
        }
    }

    #[test]
    fn bloom_serialize_roundtrip() {
        let mut b = BloomFilter::new(1000, 0.01);
        for i in 0..500 {
            b.add(format!("x{i}").as_bytes());
        }
        let bytes = b.serialize();
        let b2 = BloomFilter::deserialize(&bytes).expect("roundtrip");
        assert_eq!(b2.inserted(), b.inserted());
        for i in 0..500 {
            assert!(b2.contains(format!("x{i}").as_bytes()));
        }
        assert_eq!(b.bit_count(), b2.bit_count());
        assert_eq!(b.k(), b2.k());
    }

    #[test]
    fn bloom_merge_union() {
        let mut a = BloomFilter::new(1000, 0.01);
        let mut b = BloomFilter::new(1000, 0.01);
        a.add(b"x");
        a.add(b"y");
        b.add(b"y");
        b.add(b"z");
        assert!(a.merge(&b));
        assert!(a.contains(b"x"));
        assert!(a.contains(b"y"));
        assert!(a.contains(b"z"));
    }

    #[test]
    fn bloom_merge_rejects_mismatched_geometry() {
        let mut a = BloomFilter::new(1000, 0.01);
        let b = BloomFilter::new(10_000, 0.01); // different bit count
        a.add(b"x");
        assert!(!a.merge(&b));
    }

    #[test]
    fn hll_estimate_within_two_percent() {
        let mut h = HllSketch::new(14);
        let n = 100_000usize;
        for i in 0..n {
            h.add(format!("k{i}").as_bytes());
        }
        let est = h.count();
        let rel = (est - n as f64).abs() / n as f64;
        assert!(
            rel < 0.02,
            "HLL p=14 should be within 2%; got {est} for {n} (rel {rel:.4})"
        );
    }

    #[test]
    fn hll_empty_is_zero() {
        let h = HllSketch::new(12);
        assert_eq!(h.count(), 0.0);
    }

    #[test]
    fn hll_clear_resets() {
        let mut h = HllSketch::new(10);
        for i in 0..1000 {
            h.add(format!("k{i}").as_bytes());
        }
        h.clear();
        assert_eq!(h.count(), 0.0);
    }

    #[test]
    fn hll_serialize_roundtrip() {
        let mut h = HllSketch::new(12);
        for i in 0..5000 {
            h.add(format!("v{i}").as_bytes());
        }
        let bytes = h.serialize();
        let h2 = HllSketch::deserialize(&bytes).unwrap();
        assert_eq!(h.precision(), h2.precision());
        assert!((h.count() - h2.count()).abs() < 1e-9);
    }

    #[test]
    fn hll_merge_union_is_correct() {
        let mut a = HllSketch::new(14);
        let mut b = HllSketch::new(14);
        for i in 0..50_000 {
            a.add(format!("k{i}").as_bytes());
        }
        for i in 50_000..100_000 {
            b.add(format!("k{i}").as_bytes());
        }
        assert!(a.merge(&b));
        let est = a.count();
        let rel = (est - 100_000.0_f64).abs() / 100_000.0;
        assert!(rel < 0.02, "merged HLL got {est} for 100k (rel {rel:.4})");
    }

    #[test]
    fn hll_merge_rejects_precision_mismatch() {
        let mut a = HllSketch::new(12);
        let b = HllSketch::new(14);
        assert!(!a.merge(&b));
    }
}
