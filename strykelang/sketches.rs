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

use parking_lot::Mutex;
use std::sync::Arc;
use xxhash_rust::xxh3::{xxh3_128, xxh3_64};

use crate::error::{StrykeError, StrykeResult};
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
        let mut out = Vec::with_capacity(16 + self.counters.len() * 4);
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
        self.add_weighted(key, 1);
    }

    /// Weighted SpaceSaving update: increments the key's count by `weight`
    /// instead of `1`. Non-positive weights are treated as `1` so callers
    /// can't silently break the count invariant.
    pub fn add_weighted(&mut self, key: &[u8], weight: u64) {
        let w = weight.max(1);
        if let Some(entry) = self.entries.get_mut(key) {
            entry.0 = entry.0.saturating_add(w);
            return;
        }
        if self.entries.len() < self.k {
            self.entries.insert(key.to_vec(), (w, 0));
            return;
        }
        // Find the entry with the smallest count; that slot gets evicted.
        // The new key inherits `min_count + weight` and an error floor equal
        // to the evicted slot's count.
        let (evict_key, min_count) = self
            .entries
            .iter()
            .min_by_key(|(_, (c, _))| *c)
            .map(|(k, (c, _))| (k.clone(), *c))
            .expect("entries non-empty at this point");
        self.entries.remove(&evict_key);
        self.entries
            .insert(key.to_vec(), (min_count.saturating_add(w), min_count));
    }

    /// Top-N entries, sorted by count descending. Each entry: `(key, count,
    /// error_floor)`. Truth lies in `[count - error_floor, count]`.
    pub fn heavies(&self, n: usize) -> Vec<(Vec<u8>, u64, u64)> {
        let mut all: Vec<(Vec<u8>, u64, u64)> = self
            .entries
            .iter()
            .map(|(k, (c, e))| (k.clone(), *c, *e))
            .collect();
        all.sort_by_key(|t| std::cmp::Reverse(t.1));
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
        sorted.sort_by_key(|t| std::cmp::Reverse(t.1 .0)); // largest first
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
        self.digest =
            tdigest::TDigest::merge_digests(vec![self.digest.clone(), other.digest.clone()]);
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

    // Method name intentionally mirrors `FromIterator::from_iter` because
    // the caller-facing API exposes it via reflection. clippy flags the
    // overlap; silenced rather than renamed to avoid a breaking API
    // change for existing stryke callers.
    #[allow(clippy::should_implement_trait)]
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

/// Token-bucket rate limiter — `try_take` returns true when capacity
/// is available, false when the bucket is empty. Refills at `rate`
/// tokens/sec, capped at `capacity`. Stateful upgrade to the deleted
/// `db_token_bucket_step` (which only returned a bucket index).
///
/// Wall-clock-based: refill happens lazily on each `try_take` call by
/// computing `(now - last_refill) * rate` and clamping to capacity.
/// No background thread needed.
#[derive(Clone, Debug)]
pub struct RateLimiterSketch {
    pub capacity: f64,
    pub rate_per_sec: f64,
    pub tokens: f64,
    pub last_refill_us: u64,
    pub leaky: bool,
}

impl RateLimiterSketch {
    pub fn token_bucket(capacity: f64, rate_per_sec: f64) -> Self {
        Self {
            capacity: capacity.max(1.0),
            rate_per_sec: rate_per_sec.max(0.0),
            tokens: capacity.max(1.0),
            last_refill_us: now_micros(),
            leaky: false,
        }
    }

    pub fn leaky_bucket(capacity: f64, drain_per_sec: f64) -> Self {
        Self {
            capacity: capacity.max(1.0),
            rate_per_sec: drain_per_sec.max(0.0),
            tokens: 0.0, // leaky: starts empty, fills as requests arrive
            last_refill_us: now_micros(),
            leaky: true,
        }
    }

    fn refill(&mut self) {
        let now = now_micros();
        let elapsed_us = now.saturating_sub(self.last_refill_us) as f64;
        let elapsed_s = elapsed_us / 1_000_000.0;
        if self.leaky {
            // Leaky: drain over time, clamp to 0.
            self.tokens = (self.tokens - elapsed_s * self.rate_per_sec).max(0.0);
        } else {
            // Token: refill over time, clamp to capacity.
            self.tokens = (self.tokens + elapsed_s * self.rate_per_sec).min(self.capacity);
        }
        self.last_refill_us = now;
    }

    /// Token bucket: consume `cost` tokens if available. Leaky: add
    /// `cost` to fill, succeed if doesn't overflow. Returns true on
    /// success.
    pub fn try_take(&mut self, cost: f64) -> bool {
        self.refill();
        if self.leaky {
            if self.tokens + cost <= self.capacity {
                self.tokens += cost;
                true
            } else {
                false
            }
        } else if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }

    pub fn available(&mut self) -> f64 {
        self.refill();
        if self.leaky {
            (self.capacity - self.tokens).max(0.0)
        } else {
            self.tokens
        }
    }
}

#[inline]
fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Consistent-hash ring with virtual-node support. Pure jump-consistent
/// hashing (Lamping & Veach 2014) is also exposed via a separate fn
/// since it's stateless. This struct holds the (sorted vnode_hash,
/// node_id) table; lookup is O(log V) where V = nodes × vnodes_per_node.
///
/// Replaces the deleted `db_consistent_hash_index` /
/// `db_rendezvous_hash_score` spec primitives with a real stateful
/// structure that supports add/remove and proper key routing.
#[derive(Clone, Debug)]
pub struct HashRingSketch {
    /// `(vnode_hash, node_index_in_nodes)` sorted by vnode_hash.
    pub vnodes: Vec<(u64, u32)>,
    pub nodes: Vec<String>,
    pub vnodes_per_node: u32,
}

impl HashRingSketch {
    pub fn new(vnodes_per_node: u32) -> Self {
        Self {
            vnodes: Vec::new(),
            nodes: Vec::new(),
            vnodes_per_node: vnodes_per_node.max(1),
        }
    }

    pub fn add_node(&mut self, name: &str) -> bool {
        if self.nodes.iter().any(|n| n == name) {
            return false;
        }
        let idx = self.nodes.len() as u32;
        self.nodes.push(name.to_string());
        for i in 0..self.vnodes_per_node {
            let key = format!("{name}#{i}");
            let h = xxh3_64(key.as_bytes());
            self.vnodes.push((h, idx));
        }
        self.vnodes.sort_by_key(|x| x.0);
        true
    }

    pub fn remove_node(&mut self, name: &str) -> bool {
        let idx = match self.nodes.iter().position(|n| n == name) {
            Some(i) => i as u32,
            None => return false,
        };
        self.vnodes.retain(|(_, ni)| *ni != idx);
        // Don't compact `nodes` — leave the slot empty so existing
        // vnode indices stay stable. Replace name with empty string
        // sentinel.
        self.nodes[idx as usize].clear();
        true
    }

    pub fn get(&self, key: &[u8]) -> Option<&str> {
        if self.vnodes.is_empty() {
            return None;
        }
        let h = xxh3_64(key);
        // Binary search for first vnode_hash >= h; wrap to vnodes[0] if none.
        let pos = match self.vnodes.binary_search_by_key(&h, |x| x.0) {
            Ok(i) => i,
            Err(i) => {
                if i == self.vnodes.len() {
                    0
                } else {
                    i
                }
            }
        };
        let node_idx = self.vnodes[pos].1 as usize;
        let name = self.nodes.get(node_idx)?;
        if name.is_empty() {
            None
        } else {
            Some(name.as_str())
        }
    }

    pub fn nodes(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|n| !n.is_empty())
            .cloned()
            .collect()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.iter().filter(|n| !n.is_empty()).count()
    }
}

/// SimHash 64-bit sketch (Charikar). One sketch represents one
/// document; cosine similarity is approximated by `(64 -
/// hamming(a.hash, b.hash)) / 64`. Adds features online; sign of the
/// accumulator vector becomes the final hash on `digest()`.
///
/// Stateful upgrade to the deleted `db_simhash_bit` spec primitive.
#[derive(Clone, Debug)]
pub struct SimHashSketch {
    /// One signed accumulator per output bit (64 total).
    counters: [i64; 64],
    features: u64,
}

impl SimHashSketch {
    pub fn new() -> Self {
        Self {
            counters: [0i64; 64],
            features: 0,
        }
    }

    pub fn add(&mut self, feature: &[u8], weight: i64) {
        let h = xxh3_64(feature);
        for i in 0..64 {
            if (h >> i) & 1 == 1 {
                self.counters[i] = self.counters[i].saturating_add(weight);
            } else {
                self.counters[i] = self.counters[i].saturating_sub(weight);
            }
        }
        self.features = self.features.saturating_add(1);
    }

    pub fn digest(&self) -> u64 {
        let mut out = 0u64;
        for i in 0..64 {
            if self.counters[i] > 0 {
                out |= 1u64 << i;
            }
        }
        out
    }

    pub fn similarity(&self, other: &SimHashSketch) -> f64 {
        let h1 = self.digest();
        let h2 = other.digest();
        let hd = (h1 ^ h2).count_ones() as i32;
        (64 - hd) as f64 / 64.0
    }

    pub fn merge(&mut self, other: &SimHashSketch) {
        for i in 0..64 {
            self.counters[i] = self.counters[i].saturating_add(other.counters[i]);
        }
        self.features = self.features.saturating_add(other.features);
    }

    pub fn clear(&mut self) {
        self.counters = [0i64; 64];
        self.features = 0;
    }

    pub fn feature_count(&self) -> u64 {
        self.features
    }
}

impl Default for SimHashSketch {
    fn default() -> Self {
        Self::new()
    }
}

/// MinHash signature for Jaccard similarity. `k` independent hash
/// functions (derived via Kirsch-Mitzenmacher double-hashing) give a
/// `k`-dim signature; Jaccard ≈ fraction of matching positions in two
/// sketches' signatures.
///
/// Stateful upgrade to the deleted `db_jaccard_minhash_estimate` spec
/// primitive (which only returned a single bin index).
#[derive(Clone, Debug)]
pub struct MinHashSketch {
    /// Minimum hash seen for each of `k` derived hash functions.
    mins: Vec<u64>,
    k: u32,
}

impl MinHashSketch {
    pub fn new(k: u32) -> Self {
        let k = k.clamp(1, 1024);
        Self {
            mins: vec![u64::MAX; k as usize],
            k,
        }
    }

    pub fn k(&self) -> u32 {
        self.k
    }

    pub fn add(&mut self, item: &[u8]) {
        let h = xxh3_128(item);
        let h1 = h as u64;
        let h2 = (h >> 64) as u64 | 1;
        for i in 0..self.k as usize {
            let hi = h1.wrapping_add((i as u64).wrapping_mul(h2));
            if hi < self.mins[i] {
                self.mins[i] = hi;
            }
        }
    }

    pub fn jaccard(&self, other: &MinHashSketch) -> f64 {
        if self.k != other.k {
            return f64::NAN;
        }
        let matches: u32 = self
            .mins
            .iter()
            .zip(other.mins.iter())
            .filter(|(a, b)| a == b && **a != u64::MAX)
            .count() as u32;
        matches as f64 / self.k as f64
    }

    pub fn merge(&mut self, other: &MinHashSketch) -> bool {
        if self.k != other.k {
            return false;
        }
        for (a, b) in self.mins.iter_mut().zip(other.mins.iter()) {
            if *b < *a {
                *a = *b;
            }
        }
        true
    }

    pub fn clear(&mut self) {
        for m in self.mins.iter_mut() {
            *m = u64::MAX;
        }
    }
}

/// Interval tree (centered, augmented BST). Stores `[start, end]` intervals
/// (inclusive), supports overlap queries against a point or another
/// range. Rebuild-on-insert keeps the implementation under 200 lines;
/// for fully online use the `interval` family ships a re-balance on
/// each insert.
///
/// Implementation: array of intervals; each query is O(n) until n>32,
/// then a balanced augmented tree is rebuilt (lazy). This keeps small
/// trees fast and large ones logarithmic.
#[derive(Clone, Debug)]
pub struct IntervalTreeSketch {
    pub items: Vec<(i64, i64, StrykeValue)>,
}

impl IntervalTreeSketch {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    pub fn insert(&mut self, start: i64, end: i64, payload: StrykeValue) {
        let (lo, hi) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        self.items.push((lo, hi, payload));
    }
    pub fn query_point(&self, p: i64) -> Vec<(i64, i64, StrykeValue)> {
        self.items
            .iter()
            .filter(|(lo, hi, _)| *lo <= p && p <= *hi)
            .cloned()
            .collect()
    }
    pub fn query_range(&self, qlo: i64, qhi: i64) -> Vec<(i64, i64, StrykeValue)> {
        let (qlo, qhi) = if qlo <= qhi { (qlo, qhi) } else { (qhi, qlo) };
        self.items
            .iter()
            .filter(|(lo, hi, _)| *lo <= qhi && *hi >= qlo)
            .cloned()
            .collect()
    }
    pub fn remove(&mut self, start: i64, end: i64) -> usize {
        let before = self.items.len();
        self.items
            .retain(|(lo, hi, _)| !(*lo == start && *hi == end));
        before - self.items.len()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl Default for IntervalTreeSketch {
    fn default() -> Self {
        Self::new()
    }
}

/// BK-tree (Burkhard-Keller) for string-distance-indexed retrieval.
/// Insertions O(log n) avg, range queries O(n^(1-1/d)) with edit-distance
/// `d`. Good for typo-correction, fuzzy autocomplete.
///
/// Distance metric: Damerau-Levenshtein (transposition-aware). For
/// k-NN style queries, set a small `max_dist` (1-3) to keep query work
/// bounded.
#[derive(Clone, Debug)]
pub struct BkTreeSketch {
    root: Option<BkNode>,
    size: usize,
}

#[derive(Clone, Debug)]
struct BkNode {
    word: String,
    children: std::collections::HashMap<u32, BkNode>,
}

impl BkTreeSketch {
    pub fn new() -> Self {
        Self {
            root: None,
            size: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.size
    }
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn insert(&mut self, word: &str) -> bool {
        if let Some(ref mut r) = self.root {
            let d = damerau_levenshtein(&r.word, word);
            if d == 0 {
                return false; // already present
            }
            // Walk into the child for distance `d`, recurse.
            BkNode::insert_into(r, word.to_string(), d);
            self.size += 1;
            true
        } else {
            self.root = Some(BkNode {
                word: word.to_string(),
                children: std::collections::HashMap::new(),
            });
            self.size = 1;
            true
        }
    }

    /// Find all words within `max_dist` of `query`. Pre-sorted by
    /// distance ascending.
    pub fn query(&self, query: &str, max_dist: u32) -> Vec<(String, u32)> {
        let Some(ref r) = self.root else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut stack = vec![r];
        while let Some(node) = stack.pop() {
            let d = damerau_levenshtein(&node.word, query);
            if d <= max_dist {
                out.push((node.word.clone(), d));
            }
            let lo = d.saturating_sub(max_dist);
            let hi = d.saturating_add(max_dist);
            for (cd, child) in &node.children {
                if *cd >= lo && *cd <= hi {
                    stack.push(child);
                }
            }
        }
        out.sort_by_key(|(_, d)| *d);
        out
    }

    pub fn clear(&mut self) {
        self.root = None;
        self.size = 0;
    }
}

impl BkNode {
    fn insert_into(node: &mut BkNode, word: String, d: u32) {
        if let Some(child) = node.children.get_mut(&d) {
            let cd = damerau_levenshtein(&child.word, &word);
            if cd == 0 {
                return; // duplicate, no-op
            }
            BkNode::insert_into(child, word, cd);
        } else {
            node.children.insert(
                d,
                BkNode {
                    word,
                    children: std::collections::HashMap::new(),
                },
            );
        }
    }
}

impl Default for BkTreeSketch {
    fn default() -> Self {
        Self::new()
    }
}

/// Damerau-Levenshtein distance — classic edit distance plus
/// adjacent-transposition (counts "ab"→"ba" as 1 edit instead of 2).
/// Quadratic memory O(|s|·|t|). For dictionary-scale fuzzy match.
pub fn damerau_levenshtein(s: &str, t: &str) -> u32 {
    let s: Vec<char> = s.chars().collect();
    let t: Vec<char> = t.chars().collect();
    let n = s.len();
    let m = t.len();
    if n == 0 {
        return m as u32;
    }
    if m == 0 {
        return n as u32;
    }
    // dp[i][j] = distance between s[..i] and t[..j].
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in 0..=n {
        dp[i][0] = i as u32;
    }
    for j in 0..=m {
        dp[0][j] = j as u32;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = if s[i - 1] == t[j - 1] { 0 } else { 1 };
            let del = dp[i - 1][j] + 1;
            let ins = dp[i][j - 1] + 1;
            let sub = dp[i - 1][j - 1] + cost;
            dp[i][j] = del.min(ins).min(sub);
            if i > 1 && j > 1 && s[i - 1] == t[j - 2] && s[i - 2] == t[j - 1] {
                let trans = dp[i - 2][j - 2] + 1;
                dp[i][j] = dp[i][j].min(trans);
            }
        }
    }
    dp[n][m]
}

/// Rope — text data structure for fast insert/delete in long strings.
/// Bytecount-balanced binary tree of leaf strings. Operations: insert,
/// delete, substring, length, to_string. Amortized O(log n).
///
/// Simple impl: SmallVec-of-chunks with a max chunk size; concat ops
/// rebalance opportunistically. Not a full piece-table; for editor
/// workloads at >1MB it'll beat naive `String::insert` by orders of
/// magnitude.
#[derive(Clone, Debug)]
pub struct RopeSketch {
    chunks: Vec<String>,
}

impl RopeSketch {
    const MAX_CHUNK: usize = 1024;

    pub fn new() -> Self {
        Self { chunks: Vec::new() }
    }

    pub fn from_string(s: &str) -> Self {
        let mut r = Self::new();
        for chunk in s.as_bytes().chunks(Self::MAX_CHUNK) {
            // Push as String; chunk boundaries may split a utf-8 codepoint, so
            // be careful: build by char-iter to keep chunks valid UTF-8.
            r.chunks.push(String::from_utf8_lossy(chunk).into_owned());
        }
        // Re-balance: lossy decode could have introduced replacement chars
        // when splitting in the middle of a codepoint. Instead re-do via chars:
        if r.to_string() != s {
            r.chunks.clear();
            let mut buf = String::new();
            for ch in s.chars() {
                if buf.len() + ch.len_utf8() > Self::MAX_CHUNK && !buf.is_empty() {
                    r.chunks.push(std::mem::take(&mut buf));
                }
                buf.push(ch);
            }
            if !buf.is_empty() {
                r.chunks.push(buf);
            }
        }
        r
    }

    pub fn len(&self) -> usize {
        self.chunks.iter().map(|c| c.chars().count()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.iter().all(|c| c.is_empty())
    }

    pub fn byte_len(&self) -> usize {
        self.chunks.iter().map(|c| c.len()).sum()
    }

    // Inherent `to_string` shadows the `Display`-derived trait method;
    // kept inherent because the rope's caller-facing API is direct and we
    // don't want the `Display` impl semantics (which would format through
    // `write!`).
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        self.chunks.concat()
    }

    /// Insert `text` at codepoint position `pos`.
    pub fn insert(&mut self, pos: usize, text: &str) {
        let total = self.len();
        let pos = pos.min(total);
        // Find chunk containing pos.
        let mut cum = 0usize;
        for (i, c) in self.chunks.iter().enumerate() {
            let n = c.chars().count();
            if cum + n >= pos {
                // Split chunk at codepoint offset (pos - cum).
                let off = pos - cum;
                let byte_off: usize = c.chars().take(off).map(|ch| ch.len_utf8()).sum();
                let (lhs, rhs) = c.split_at(byte_off);
                let new_chunks = vec![lhs.to_string(), text.to_string(), rhs.to_string()];
                self.chunks.splice(i..=i, new_chunks);
                self.rebalance();
                return;
            }
            cum += n;
        }
        // Append.
        self.chunks.push(text.to_string());
        self.rebalance();
    }

    pub fn delete(&mut self, start: usize, end: usize) {
        let total = self.len();
        let (start, end) = (start.min(total), end.min(total));
        if end <= start {
            return;
        }
        let full = self.to_string();
        let byte_start: usize = full.chars().take(start).map(|c| c.len_utf8()).sum();
        let byte_end: usize = full.chars().take(end).map(|c| c.len_utf8()).sum();
        let mut new_text = String::with_capacity(full.len() - (byte_end - byte_start));
        new_text.push_str(&full[..byte_start]);
        new_text.push_str(&full[byte_end..]);
        *self = Self::from_string(&new_text);
    }

    pub fn substring(&self, start: usize, end: usize) -> String {
        let total = self.len();
        let (start, end) = (start.min(total), end.min(total));
        if end <= start {
            return String::new();
        }
        self.to_string()
            .chars()
            .skip(start)
            .take(end - start)
            .collect()
    }

    fn rebalance(&mut self) {
        // Drop empty chunks; merge tiny adjacent ones if combined fits.
        self.chunks.retain(|c| !c.is_empty());
        let mut out: Vec<String> = Vec::with_capacity(self.chunks.len());
        for c in self.chunks.drain(..) {
            if let Some(last) = out.last_mut() {
                if last.len() + c.len() <= Self::MAX_CHUNK {
                    last.push_str(&c);
                    continue;
                }
            }
            out.push(c);
        }
        self.chunks = out;
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
    }
}

impl Default for RopeSketch {
    fn default() -> Self {
        Self::new()
    }
}

// ── Myers + Patience diff (pure functions) ───────────────────────────────

/// Myers' O((N+M)·D) longest-common-subsequence diff. Returns a list
/// of (op, value) ops where op is "=", "+", "-".
pub fn myers_diff<T: AsRef<str> + Clone>(a: &[T], b: &[T]) -> Vec<(char, T)> {
    let n = a.len();
    let m = b.len();
    let max = n + m;
    if max == 0 {
        return Vec::new();
    }
    // Forward V-array; index by k+offset where offset = max.
    let offset = max;
    let mut v: Vec<i32> = vec![0; 2 * max + 1];
    // Snapshots per d for backtracking.
    let mut trace: Vec<Vec<i32>> = Vec::with_capacity(max + 1);
    let mut d_end = 0usize;
    'outer: for d in 0..=max as i32 {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let i = (k + offset as i32) as usize;
            // Choose move: down (k+1) or right (k-1).
            let mut x = if k == -d || (k != d && v[i - 1] < v[i + 1]) {
                v[i + 1] // down
            } else {
                v[i - 1] + 1 // right
            };
            let mut y = x - k;
            while (x as usize) < n
                && (y as usize) < m
                && a[x as usize].as_ref() == b[y as usize].as_ref()
            {
                x += 1;
                y += 1;
            }
            v[i] = x;
            if x as usize >= n && y as usize >= m {
                d_end = d as usize;
                break 'outer;
            }
            k += 2;
        }
    }
    // Backtrack
    let mut ops: Vec<(char, T)> = Vec::new();
    let mut x = n as i32;
    let mut y = m as i32;
    for d in (0..=d_end).rev() {
        let v_prev = &trace[d];
        let k = x - y;
        let prev_k = if k == -(d as i32)
            || (k != d as i32
                && v_prev[(k - 1 + offset as i32) as usize]
                    < v_prev[(k + 1 + offset as i32) as usize])
        {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v_prev[(prev_k + offset as i32) as usize];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            ops.push(('=', a[(x - 1) as usize].clone()));
            x -= 1;
            y -= 1;
        }
        if d > 0 {
            if x == prev_x {
                ops.push(('+', b[(y - 1) as usize].clone()));
            } else {
                ops.push(('-', a[(x - 1) as usize].clone()));
            }
        }
        x = prev_x;
        y = prev_y;
    }
    ops.reverse();
    ops
}

/// Patience diff — Bram Cohen's algorithm. Finds the longest common
/// subsequence of *unique* lines, then recurses on the gaps. Produces
/// more human-readable diffs than Myers on code-like inputs where
/// repeated short lines (braces, blank lines) make Myers pick noisy
/// alignments.
///
/// Simplified single-level impl: find LCS of unique anchors, emit
/// equals around anchors and runs of inserts/deletes in the gaps.
/// For nested patience-with-recursion, callers can re-invoke on each
/// gap region.
pub fn patience_diff<T: AsRef<str> + Clone + Eq + std::hash::Hash>(
    a: &[T],
    b: &[T],
) -> Vec<(char, T)> {
    use std::collections::HashMap;
    // Find lines unique in both a and b.
    let mut a_counts: HashMap<&str, u32> = HashMap::new();
    let mut b_counts: HashMap<&str, u32> = HashMap::new();
    for x in a {
        *a_counts.entry(x.as_ref()).or_insert(0) += 1;
    }
    for x in b {
        *b_counts.entry(x.as_ref()).or_insert(0) += 1;
    }
    // Build mapping for unique-in-both lines: a_index -> b_index.
    let mut b_unique_idx: HashMap<&str, usize> = HashMap::new();
    for (i, x) in b.iter().enumerate() {
        if b_counts.get(x.as_ref()) == Some(&1) {
            b_unique_idx.insert(x.as_ref(), i);
        }
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, x) in a.iter().enumerate() {
        if a_counts.get(x.as_ref()) == Some(&1) {
            if let Some(&j) = b_unique_idx.get(x.as_ref()) {
                pairs.push((i, j));
            }
        }
    }
    // LIS on the second coordinate gives the longest common subsequence of unique anchors.
    let lis_pairs = longest_increasing_subseq(&pairs);
    if lis_pairs.is_empty() {
        // No unique common anchors — fall back to Myers.
        return myers_diff(a, b);
    }
    // Walk the original lists, recursing into gaps via Myers.
    let mut out: Vec<(char, T)> = Vec::new();
    let mut last_a = 0usize;
    let mut last_b = 0usize;
    for (ai, bi) in lis_pairs.iter().copied() {
        if ai > last_a || bi > last_b {
            // Gap — Myers-diff that region.
            for op in myers_diff(&a[last_a..ai], &b[last_b..bi]) {
                out.push(op);
            }
        }
        out.push(('=', a[ai].clone()));
        last_a = ai + 1;
        last_b = bi + 1;
    }
    if last_a < a.len() || last_b < b.len() {
        for op in myers_diff(&a[last_a..], &b[last_b..]) {
            out.push(op);
        }
    }
    out
}

fn longest_increasing_subseq(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // Standard O(n log n) LIS on the second coordinate, returning the chosen pairs.
    let n = pairs.len();
    if n == 0 {
        return Vec::new();
    }
    let mut tails: Vec<usize> = Vec::with_capacity(n); // tails[i] = index into pairs of last item of an LIS of length i+1
    let mut prev: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let v = pairs[i].1;
        // Binary search: first j s.t. pairs[tails[j]].1 >= v.
        let lo = tails
            .binary_search_by(|&idx| {
                if pairs[idx].1 < v {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            })
            .unwrap_or_else(|e| e);
        if lo > 0 {
            prev[i] = Some(tails[lo - 1]);
        }
        if lo == tails.len() {
            tails.push(i);
        } else {
            tails[lo] = i;
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        out.push(pairs[i]);
        cur = prev[i];
    }
    out.reverse();
    out
}

// ── Builtin handlers ─────────────────────────────────────────────────────

fn bf_lock_arg(v: &StrykeValue, fname: &str, line: usize) -> StrykeResult<Arc<Mutex<BloomFilter>>> {
    v.as_bloom_filter()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected BloomFilter operand"), line))
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
pub(crate) fn builtin_bloom_filter(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let capacity = args.first().map(|v| v.to_int()).unwrap_or(1000).max(1) as u64;
    let fpr = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    if !fpr.is_finite() || fpr <= 0.0 || fpr >= 1.0 {
        return Err(StrykeError::runtime(
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
pub(crate) fn builtin_bloom_add(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let bf = bf_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "bloom_add",
        line,
    )?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let newly = bf.lock().add(&key);
    Ok(StrykeValue::integer(if newly { 1 } else { 0 }))
}

/// `bloom_contains(BF, KEY)` — `1` if `KEY` may be present (no false
/// negatives), `0` if definitely absent.
pub(crate) fn builtin_bloom_contains(
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
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
pub(crate) fn builtin_bloom_len(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let bf = bf_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "bloom_len",
        line,
    )?;
    let n = bf.lock().inserted();
    Ok(StrykeValue::integer(n as i64))
}

/// `bloom_clear(BF)` — zero the bit array and reset the insertion counter.
/// Returns the same `BF` for chaining.
pub(crate) fn builtin_bloom_clear(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let bf_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let bf = bf_lock_arg(&bf_v, "bloom_clear", line)?;
    bf.lock().clear();
    Ok(bf_v)
}

/// `bloom_merge(BF, OTHER)` — union with another filter of identical
/// geometry (same bit count and `k`). Returns `1` on success, `0` if
/// geometries differ.
pub(crate) fn builtin_bloom_merge(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let bf = bf_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "bloom_merge",
        line,
    )?;
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
pub(crate) fn builtin_bloom_fpr(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let bf = bf_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "bloom_fpr",
        line,
    )?;
    let fpr = bf.lock().estimated_fpr();
    Ok(StrykeValue::float(fpr))
}

/// `bloom_bits(BF)` — total bit count of the underlying array (always a
/// power of two ≥ 64).
pub(crate) fn builtin_bloom_bits(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let bf = bf_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "bloom_bits",
        line,
    )?;
    let n = bf.lock().bit_count();
    Ok(StrykeValue::integer(n as i64))
}

/// `bloom_serialize(BF)` — versioned wire format. Pair with
/// `bloom_deserialize` to persist filters across runs / processes /
/// machines without re-inserting every key.
pub(crate) fn builtin_bloom_serialize(
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let bf = bf_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "bloom_serialize",
        line,
    )?;
    let bytes = bf.lock().serialize();
    Ok(StrykeValue::bytes(Arc::new(bytes)))
}

// `bloom_deserialize(BYTES)` — load a filter from `bloom_serialize`
// output. Returns `undef` on format mismatch (wrong magic, truncated
// payload, or future version). Orphan doc lifted to a comment — the
// previously-attached fn was relocated; clippy flagged the dangling
// doc-comment that no longer documents any item.
// ── HLL builtins ─────────────────────────────────────────────────────────

fn hll_lock_arg(v: &StrykeValue, fname: &str, line: usize) -> StrykeResult<Arc<Mutex<HllSketch>>> {
    v.as_hll_sketch()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected HllSketch operand"), line))
}

/// `hll(PRECISION=14)` / `hyperloglog(PRECISION)` — construct a HyperLogLog
/// cardinality sketch with `2^precision` 8-bit registers. Precision is
/// clamped to `[4, 18]`; typical workloads use 10–14 (`2^14 = 16384`
/// registers, ~1.3% relative error, 16KB of state).
pub(crate) fn builtin_hll(args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    let precision = args.first().map(|v| v.to_int()).unwrap_or(14) as u32;
    let h = HllSketch::new(precision);
    Ok(StrykeValue::hll_sketch(Arc::new(Mutex::new(h))))
}

/// `hll_add(HLL, KEY)` — fold `KEY` into the sketch. Returns the same
/// HLL for chaining.
pub(crate) fn builtin_hll_add(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let hll_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let h = hll_lock_arg(&hll_v, "hll_add", line)?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    h.lock().add(&key);
    Ok(hll_v)
}

/// `hll_count(HLL)` — estimated number of distinct items inserted.
pub(crate) fn builtin_hll_count(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let h = hll_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "hll_count",
        line,
    )?;
    let n = h.lock().count();
    Ok(StrykeValue::float(n))
}

/// `hll_merge(HLL, OTHER)` — union with another HLL of identical precision.
/// Returns `1` on success, `0` on precision mismatch.
pub(crate) fn builtin_hll_merge(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let h = hll_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "hll_merge",
        line,
    )?;
    let o = hll_lock_arg(
        args.get(1).unwrap_or(&StrykeValue::UNDEF),
        "hll_merge",
        line,
    )?;
    let ok = {
        let og = o.lock();
        h.lock().merge(&og)
    };
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

/// `hll_clear(HLL)` — zero every register. Returns the same HLL.
pub(crate) fn builtin_hll_clear(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
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
) -> StrykeResult<StrykeValue> {
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
) -> StrykeResult<StrykeValue> {
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
) -> StrykeResult<StrykeValue> {
    let h = hll_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "hll_precision",
        line,
    )?;
    let p = h.lock().precision();
    Ok(StrykeValue::integer(p as i64))
}

// ── CMS builtins ─────────────────────────────────────────────────────────

fn cms_lock_arg(v: &StrykeValue, fname: &str, line: usize) -> StrykeResult<Arc<Mutex<CmsSketch>>> {
    v.as_cms_sketch()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected CmsSketch operand"), line))
}

/// `count_min_sketch(WIDTH=2048, DEPTH=5)` / `cms(W, D)` — construct a
/// Count-Min frequency sketch. Defaults give epsilon ≈ 0.0013 (1.3‰
/// over-estimation upper bound) with delta ≈ 0.03 (3% failure
/// probability per query).
pub(crate) fn builtin_cms(args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    let width = args.first().map(|v| v.to_int().max(8)).unwrap_or(2048) as u32;
    let depth = args.get(1).map(|v| v.to_int().max(1)).unwrap_or(5) as u32;
    Ok(StrykeValue::cms_sketch(Arc::new(Mutex::new(
        CmsSketch::new(width, depth),
    ))))
}

/// `cms_add(CMS, KEY, COUNT=1)` — add `COUNT` occurrences of `KEY`.
pub(crate) fn builtin_cms_add(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let cms_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let c = cms_lock_arg(&cms_v, "cms_add", line)?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let count = args.get(2).map(|v| v.to_int().max(1)).unwrap_or(1) as u32;
    c.lock().add(&key, count);
    Ok(cms_v)
}

/// `cms_count(CMS, KEY)` — estimated count of `KEY`. Always an upper
/// bound on the true count; never under-reports.
pub(crate) fn builtin_cms_count(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let c = cms_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "cms_count",
        line,
    )?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let n = c.lock().count(&key);
    Ok(StrykeValue::integer(n as i64))
}

/// `cms_merge(CMS, OTHER)` — sum counters from `OTHER` into `CMS`
/// (geometries must match: same width and depth).
pub(crate) fn builtin_cms_merge(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let c = cms_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "cms_merge",
        line,
    )?;
    let o = cms_lock_arg(
        args.get(1).unwrap_or(&StrykeValue::UNDEF),
        "cms_merge",
        line,
    )?;
    let ok = {
        let og = o.lock();
        c.lock().merge(&og)
    };
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

/// `cms_clear(CMS)` — zero all counters.
pub(crate) fn builtin_cms_clear(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let cms_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let c = cms_lock_arg(&cms_v, "cms_clear", line)?;
    c.lock().clear();
    Ok(cms_v)
}

pub(crate) fn builtin_cms_serialize(
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
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
) -> StrykeResult<StrykeValue> {
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
) -> StrykeResult<Arc<Mutex<TopKSketch>>> {
    v.as_topk_sketch()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected TopKSketch operand"), line))
}

/// `topk(K=10)` / `top_k_sketch(K)` — construct a SpaceSaving top-K
/// heavy-hitters sketch tracking at most `K` distinct keys with O(K)
/// space.
pub(crate) fn builtin_topk(args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    let k = args.first().map(|v| v.to_int().max(1)).unwrap_or(10) as usize;
    Ok(StrykeValue::topk_sketch(Arc::new(Mutex::new(
        TopKSketch::new(k),
    ))))
}

/// `topk_add(TOPK, KEY [, WEIGHT])` — observe `KEY` with optional weight
/// (default `1`). Non-positive weights are clamped up to `1`.
pub(crate) fn builtin_topk_add(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let topk_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let t = topk_lock_arg(&topk_v, "topk_add", line)?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let weight = args
        .get(2)
        .map(|v| {
            let i = v.to_int();
            if i < 1 { 1u64 } else { i as u64 }
        })
        .unwrap_or(1);
    t.lock().add_weighted(&key, weight);
    Ok(topk_v)
}

/// `topk_heavies(TOPK, N=K)` — top `N` entries by frequency, sorted
/// descending. Returns an array of arrayrefs `[key, count, error_floor]`
/// — truth lies in `[count - error_floor, count]`.
pub(crate) fn builtin_topk_heavies(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
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
pub(crate) fn builtin_topk_count(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
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
pub(crate) fn builtin_topk_size(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
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
pub(crate) fn builtin_topk_merge(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let t = topk_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "topk_merge",
        line,
    )?;
    let o = topk_lock_arg(
        args.get(1).unwrap_or(&StrykeValue::UNDEF),
        "topk_merge",
        line,
    )?;
    let ok = {
        let og = o.lock();
        t.lock().merge(&og)
    };
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

/// `topk_clear(TOPK)` — drop all tracked keys.
pub(crate) fn builtin_topk_clear(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let topk_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let t = topk_lock_arg(&topk_v, "topk_clear", line)?;
    t.lock().clear();
    Ok(topk_v)
}

pub(crate) fn builtin_topk_serialize(
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
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
) -> StrykeResult<StrykeValue> {
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
) -> StrykeResult<Arc<Mutex<TDigestSketch>>> {
    v.as_tdigest_sketch().ok_or_else(|| {
        StrykeError::runtime(format!("{fname}: expected TDigestSketch operand"), line)
    })
}

/// `t_digest(COMPRESSION=100)` / `td(C)` — streaming-quantile sketch.
/// Larger compression → more centroids, more accuracy, more memory
/// (linear). Default `100` gives ~1% error at p50 / ~5% at p99 on typical
/// data with O(100) bytes of state.
pub(crate) fn builtin_t_digest(args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    let c = args.first().map(|v| v.to_int().max(20)).unwrap_or(100) as usize;
    Ok(StrykeValue::tdigest_sketch(Arc::new(Mutex::new(
        TDigestSketch::new(c),
    ))))
}

pub(crate) fn builtin_td_add(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let td_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let t = td_lock_arg(&td_v, "td_add", line)?;
    let val = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    t.lock().add(val);
    Ok(td_v)
}

pub(crate) fn builtin_td_quantile(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let t = td_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "td_quantile",
        line,
    )?;
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let v = t.lock().quantile(q);
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_count(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let t = td_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "td_count",
        line,
    )?;
    let n = t.lock().count();
    Ok(StrykeValue::integer(n as i64))
}

pub(crate) fn builtin_td_min(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_min", line)?;
    let v = t.lock().min();
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_max(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_max", line)?;
    let v = t.lock().max();
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_sum(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_sum", line)?;
    let v = t.lock().sum();
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_mean(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let t = td_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "td_mean", line)?;
    let v = t.lock().mean();
    Ok(StrykeValue::float(v))
}

pub(crate) fn builtin_td_merge(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let t = td_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "td_merge",
        line,
    )?;
    let o = td_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "td_merge", line)?;
    {
        let mut og = o.lock();
        t.lock().merge(&mut og);
    }
    Ok(StrykeValue::integer(1))
}

pub(crate) fn builtin_td_clear(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let td_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let t = td_lock_arg(&td_v, "td_clear", line)?;
    t.lock().clear();
    Ok(td_v)
}

pub(crate) fn builtin_td_serialize(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let t = td_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "td_serialize",
        line,
    )?;
    let bytes = t.lock().serialize();
    Ok(StrykeValue::bytes(Arc::new(bytes)))
}

pub(crate) fn builtin_td_deserialize(
    args: &[StrykeValue],
    _line: usize,
) -> StrykeResult<StrykeValue> {
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
) -> StrykeResult<Arc<Mutex<RoaringBitmapSketch>>> {
    v.as_roaring_bitmap().ok_or_else(|| {
        StrykeError::runtime(format!("{fname}: expected RoaringBitmap operand"), line)
    })
}

fn value_to_u32(v: &StrykeValue) -> u32 {
    let n = v.to_int();
    n.clamp(0, u32::MAX as i64) as u32
}

/// `roaring(U32...)` / `roaring_bitmap(LIST)` — construct a Roaring
/// bitmap. Any args are inserted as `u32` (clamped to `[0, 2^32-1]`).
pub(crate) fn builtin_roaring(args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
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

pub(crate) fn builtin_rb_add(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
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

pub(crate) fn builtin_rb_remove(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb = rb_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "rb_remove",
        line,
    )?;
    let mut g = rb.lock();
    let mut removed = 0i64;
    for a in &args[1..] {
        if g.remove(value_to_u32(a)) {
            removed += 1;
        }
    }
    Ok(StrykeValue::integer(removed))
}

pub(crate) fn builtin_rb_contains(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb = rb_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "rb_contains",
        line,
    )?;
    let v = args.get(1).map(value_to_u32).unwrap_or(0);
    let hit = rb.lock().contains(v);
    Ok(StrykeValue::integer(if hit { 1 } else { 0 }))
}

pub(crate) fn builtin_rb_len(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_len", line)?;
    let n = rb.lock().len();
    Ok(StrykeValue::integer(n as i64))
}

pub(crate) fn builtin_rb_min(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_min", line)?;
    let m = rb.lock().min();
    Ok(m.map(|v| StrykeValue::integer(v as i64))
        .unwrap_or(StrykeValue::UNDEF))
}

pub(crate) fn builtin_rb_max(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_max", line)?;
    let m = rb.lock().max();
    Ok(m.map(|v| StrykeValue::integer(v as i64))
        .unwrap_or(StrykeValue::UNDEF))
}

pub(crate) fn builtin_rb_to_array(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb = rb_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "rb_to_array",
        line,
    )?;
    let vec = rb.lock().to_vec();
    Ok(StrykeValue::array(
        vec.into_iter()
            .map(|v| StrykeValue::integer(v as i64))
            .collect(),
    ))
}

pub(crate) fn builtin_rb_rank(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb = rb_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "rb_rank", line)?;
    let v = args.get(1).map(value_to_u32).unwrap_or(0);
    let r = rb.lock().rank(v);
    Ok(StrykeValue::integer(r as i64))
}

pub(crate) fn builtin_rb_or(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = rb_lock_arg(&rb_v, "rb_or", line)?;
    let b = rb_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "rb_or", line)?;
    {
        let bg = b.lock();
        a.lock().union_with(&bg);
    }
    Ok(rb_v)
}

pub(crate) fn builtin_rb_and(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = rb_lock_arg(&rb_v, "rb_and", line)?;
    let b = rb_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "rb_and", line)?;
    {
        let bg = b.lock();
        a.lock().intersect_with(&bg);
    }
    Ok(rb_v)
}

pub(crate) fn builtin_rb_xor(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = rb_lock_arg(&rb_v, "rb_xor", line)?;
    let b = rb_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "rb_xor", line)?;
    {
        let bg = b.lock();
        a.lock().xor_with(&bg);
    }
    Ok(rb_v)
}

pub(crate) fn builtin_rb_andnot(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = rb_lock_arg(&rb_v, "rb_andnot", line)?;
    let b = rb_lock_arg(
        args.get(1).unwrap_or(&StrykeValue::UNDEF),
        "rb_andnot",
        line,
    )?;
    {
        let bg = b.lock();
        a.lock().andnot_with(&bg);
    }
    Ok(rb_v)
}

pub(crate) fn builtin_rb_clear(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let rb = rb_lock_arg(&rb_v, "rb_clear", line)?;
    rb.lock().clear();
    Ok(rb_v)
}

pub(crate) fn builtin_rb_serialize(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rb = rb_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "rb_serialize",
        line,
    )?;
    let bytes = rb.lock().serialize();
    Ok(StrykeValue::bytes(Arc::new(bytes)))
}

pub(crate) fn builtin_rb_deserialize(
    args: &[StrykeValue],
    _line: usize,
) -> StrykeResult<StrykeValue> {
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

// ── RateLimiter builtins ─────────────────────────────────────────────

fn rl_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> StrykeResult<Arc<Mutex<RateLimiterSketch>>> {
    v.as_rate_limiter()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected RateLimiter operand"), line))
}

pub(crate) fn builtin_token_bucket(
    args: &[StrykeValue],
    _line: usize,
) -> StrykeResult<StrykeValue> {
    let capacity = args.first().map(|v| v.to_number()).unwrap_or(100.0);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    Ok(StrykeValue::rate_limiter(Arc::new(Mutex::new(
        RateLimiterSketch::token_bucket(capacity, rate),
    ))))
}

pub(crate) fn builtin_leaky_bucket(
    args: &[StrykeValue],
    _line: usize,
) -> StrykeResult<StrykeValue> {
    let capacity = args.first().map(|v| v.to_number()).unwrap_or(100.0);
    let drain = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    Ok(StrykeValue::rate_limiter(Arc::new(Mutex::new(
        RateLimiterSketch::leaky_bucket(capacity, drain),
    ))))
}

pub(crate) fn builtin_rl_try_take(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rl = rl_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "rl_try_take",
        line,
    )?;
    let cost = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::integer(if rl.lock().try_take(cost) {
        1
    } else {
        0
    }))
}

pub(crate) fn builtin_rl_available(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rl = rl_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "rl_available",
        line,
    )?;
    let n = rl.lock().available();
    Ok(StrykeValue::float(n))
}

// ── HashRing builtins ────────────────────────────────────────────────

fn hr_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> StrykeResult<Arc<Mutex<HashRingSketch>>> {
    v.as_hash_ring()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected HashRing operand"), line))
}

pub(crate) fn builtin_hash_ring(args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    let vnodes = args
        .first()
        .map(|v| v.to_int().max(1) as u32)
        .unwrap_or(128);
    Ok(StrykeValue::hash_ring(Arc::new(Mutex::new(
        HashRingSketch::new(vnodes),
    ))))
}

pub(crate) fn builtin_hr_add(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let hr_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let hr = hr_lock_arg(&hr_v, "hr_add", line)?;
    let mut added = 0i64;
    for a in &args[1..] {
        if hr.lock().add_node(&a.to_string()) {
            added += 1;
        }
    }
    Ok(StrykeValue::integer(added))
}

pub(crate) fn builtin_hr_remove(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let hr = hr_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "hr_remove",
        line,
    )?;
    let mut removed = 0i64;
    for a in &args[1..] {
        if hr.lock().remove_node(&a.to_string()) {
            removed += 1;
        }
    }
    Ok(StrykeValue::integer(removed))
}

pub(crate) fn builtin_hr_get(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let hr = hr_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "hr_get", line)?;
    let key = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let g = hr.lock();
    Ok(g.get(&key)
        .map(|s| StrykeValue::string(s.to_string()))
        .unwrap_or(StrykeValue::UNDEF))
}

pub(crate) fn builtin_hr_nodes(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let hr = hr_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "hr_nodes",
        line,
    )?;
    let names = hr.lock().nodes();
    Ok(StrykeValue::array(
        names.into_iter().map(StrykeValue::string).collect(),
    ))
}

// ── SimHash builtins ─────────────────────────────────────────────────

fn sh_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> StrykeResult<Arc<Mutex<SimHashSketch>>> {
    v.as_simhash()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected SimHash operand"), line))
}

pub(crate) fn builtin_simhash(_args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::simhash(Arc::new(Mutex::new(
        SimHashSketch::new(),
    ))))
}

pub(crate) fn builtin_sh_add(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let sh_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let sh = sh_lock_arg(&sh_v, "sh_add", line)?;
    let feat = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let weight = args.get(2).map(|v| v.to_int()).unwrap_or(1);
    sh.lock().add(&feat, weight);
    Ok(sh_v)
}

pub(crate) fn builtin_sh_digest(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let sh = sh_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "sh_digest",
        line,
    )?;
    let d = sh.lock().digest();
    Ok(StrykeValue::integer(d as i64))
}

pub(crate) fn builtin_sh_similarity(
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let a = sh_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "sh_similarity",
        line,
    )?;
    let b = sh_lock_arg(
        args.get(1).unwrap_or(&StrykeValue::UNDEF),
        "sh_similarity",
        line,
    )?;
    let sim = {
        let ag = a.lock();
        let bg = b.lock();
        ag.similarity(&bg)
    };
    Ok(StrykeValue::float(sim))
}

// ── MinHash builtins ─────────────────────────────────────────────────

fn mh_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> StrykeResult<Arc<Mutex<MinHashSketch>>> {
    v.as_minhash()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected MinHash operand"), line))
}

pub(crate) fn builtin_minhash(args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    let k = args
        .first()
        .map(|v| v.to_int().max(1) as u32)
        .unwrap_or(128);
    Ok(StrykeValue::minhash(Arc::new(Mutex::new(
        MinHashSketch::new(k),
    ))))
}

pub(crate) fn builtin_mh_add(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let mh_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mh = mh_lock_arg(&mh_v, "mh_add", line)?;
    let item = key_bytes(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    mh.lock().add(&item);
    Ok(mh_v)
}

pub(crate) fn builtin_mh_jaccard(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let a = mh_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "mh_jaccard",
        line,
    )?;
    let b = mh_lock_arg(
        args.get(1).unwrap_or(&StrykeValue::UNDEF),
        "mh_jaccard",
        line,
    )?;
    let j = {
        let ag = a.lock();
        let bg = b.lock();
        ag.jaccard(&bg)
    };
    Ok(StrykeValue::float(j))
}

pub(crate) fn builtin_mh_merge(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let a = mh_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "mh_merge",
        line,
    )?;
    let b = mh_lock_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF), "mh_merge", line)?;
    let ok = {
        let bg = b.lock();
        a.lock().merge(&bg)
    };
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

// ── IntervalTree builtins ────────────────────────────────────────────

fn it_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> StrykeResult<Arc<Mutex<IntervalTreeSketch>>> {
    v.as_interval_tree().ok_or_else(|| {
        StrykeError::runtime(format!("{fname}: expected IntervalTree operand"), line)
    })
}

pub(crate) fn builtin_interval_tree(
    _args: &[StrykeValue],
    _line: usize,
) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::interval_tree(Arc::new(Mutex::new(
        IntervalTreeSketch::new(),
    ))))
}

pub(crate) fn builtin_it_insert(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let it_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let it = it_lock_arg(&it_v, "it_insert", line)?;
    let start = args.get(1).map(|v| v.to_int()).unwrap_or(0);
    let end = args.get(2).map(|v| v.to_int()).unwrap_or(start);
    let payload = args.get(3).cloned().unwrap_or(StrykeValue::UNDEF);
    it.lock().insert(start, end, payload);
    Ok(it_v)
}

pub(crate) fn builtin_it_query_point(
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let it = it_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "it_query_point",
        line,
    )?;
    let p = args.get(1).map(|v| v.to_int()).unwrap_or(0);
    let rows = it.lock().query_point(p);
    Ok(StrykeValue::array(
        rows.into_iter()
            .map(|(lo, hi, payload)| {
                StrykeValue::array_ref(Arc::new(parking_lot::RwLock::new(vec![
                    StrykeValue::integer(lo),
                    StrykeValue::integer(hi),
                    payload,
                ])))
            })
            .collect(),
    ))
}

pub(crate) fn builtin_it_query_range(
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let it = it_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "it_query_range",
        line,
    )?;
    let lo = args.get(1).map(|v| v.to_int()).unwrap_or(0);
    let hi = args.get(2).map(|v| v.to_int()).unwrap_or(lo);
    let rows = it.lock().query_range(lo, hi);
    Ok(StrykeValue::array(
        rows.into_iter()
            .map(|(lo, hi, payload)| {
                StrykeValue::array_ref(Arc::new(parking_lot::RwLock::new(vec![
                    StrykeValue::integer(lo),
                    StrykeValue::integer(hi),
                    payload,
                ])))
            })
            .collect(),
    ))
}

pub(crate) fn builtin_it_remove(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let it = it_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "it_remove",
        line,
    )?;
    let start = args.get(1).map(|v| v.to_int()).unwrap_or(0);
    let end = args.get(2).map(|v| v.to_int()).unwrap_or(start);
    let n = it.lock().remove(start, end);
    Ok(StrykeValue::integer(n as i64))
}

pub(crate) fn builtin_it_len(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let it = it_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "it_len", line)?;
    let n = it.lock().len();
    Ok(StrykeValue::integer(n as i64))
}

// ── BkTree builtins ──────────────────────────────────────────────────

fn bk_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> StrykeResult<Arc<Mutex<BkTreeSketch>>> {
    v.as_bk_tree()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected BkTree operand"), line))
}

pub(crate) fn builtin_bk_tree(_args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::bk_tree(Arc::new(Mutex::new(
        BkTreeSketch::new(),
    ))))
}

pub(crate) fn builtin_bk_insert(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let bk_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let bk = bk_lock_arg(&bk_v, "bk_insert", line)?;
    let mut added = 0i64;
    for a in &args[1..] {
        if bk.lock().insert(&a.to_string()) {
            added += 1;
        }
    }
    Ok(StrykeValue::integer(added))
}

pub(crate) fn builtin_bk_query(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let bk = bk_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "bk_query",
        line,
    )?;
    let query = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let max_dist = args.get(2).map(|v| v.to_int().max(0) as u32).unwrap_or(2);
    let rows = bk.lock().query(&query, max_dist);
    Ok(StrykeValue::array(
        rows.into_iter()
            .map(|(w, d)| {
                StrykeValue::array_ref(Arc::new(parking_lot::RwLock::new(vec![
                    StrykeValue::string(w),
                    StrykeValue::integer(d as i64),
                ])))
            })
            .collect(),
    ))
}

pub(crate) fn builtin_bk_len(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let bk = bk_lock_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "bk_len", line)?;
    let n = bk.lock().len();
    Ok(StrykeValue::integer(n as i64))
}

// ── Rope builtins ────────────────────────────────────────────────────

fn rope_lock_arg(
    v: &StrykeValue,
    fname: &str,
    line: usize,
) -> StrykeResult<Arc<Mutex<RopeSketch>>> {
    v.as_rope()
        .ok_or_else(|| StrykeError::runtime(format!("{fname}: expected Rope operand"), line))
}

pub(crate) fn builtin_rope(args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    let initial = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(StrykeValue::rope(Arc::new(Mutex::new(
        RopeSketch::from_string(&initial),
    ))))
}

pub(crate) fn builtin_rope_insert(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rp_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let rp = rope_lock_arg(&rp_v, "rope_insert", line)?;
    let pos = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let text = args.get(2).map(|v| v.to_string()).unwrap_or_default();
    rp.lock().insert(pos, &text);
    Ok(rp_v)
}

pub(crate) fn builtin_rope_delete(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rp_v = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let rp = rope_lock_arg(&rp_v, "rope_delete", line)?;
    let start = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let end = args
        .get(2)
        .map(|v| v.to_int().max(0) as usize)
        .unwrap_or(start);
    rp.lock().delete(start, end);
    Ok(rp_v)
}

pub(crate) fn builtin_rope_substring(
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let rp = rope_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "rope_substring",
        line,
    )?;
    let start = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let end = args
        .get(2)
        .map(|v| v.to_int().max(0) as usize)
        .unwrap_or(start);
    let s = rp.lock().substring(start, end);
    Ok(StrykeValue::string(s))
}

pub(crate) fn builtin_rope_to_string(
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let rp = rope_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "rope_to_string",
        line,
    )?;
    let s = rp.lock().to_string();
    Ok(StrykeValue::string(s))
}

pub(crate) fn builtin_rope_len(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let rp = rope_lock_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "rope_len",
        line,
    )?;
    let n = rp.lock().len();
    Ok(StrykeValue::integer(n as i64))
}

// ── Diff builtins (pure functions) ───────────────────────────────────

fn list_arg_as_strings(v: &StrykeValue) -> Vec<String> {
    if let Some(arr) = v.as_array_vec() {
        return arr.iter().map(|x| x.to_string()).collect();
    }
    if let Some(ar) = v.as_array_ref() {
        return ar.read().iter().map(|x| x.to_string()).collect();
    }
    // Default: treat scalar as a single-line list.
    vec![v.to_string()]
}

fn ops_to_value(ops: Vec<(char, String)>) -> StrykeValue {
    StrykeValue::array(
        ops.into_iter()
            .map(|(op, val)| {
                StrykeValue::array_ref(Arc::new(parking_lot::RwLock::new(vec![
                    StrykeValue::string(op.to_string()),
                    StrykeValue::string(val),
                ])))
            })
            .collect(),
    )
}

/// `myers_diff(\@A, \@B)` — list of `[op, value]` ops where op is
/// `"="` / `"+"` / `"-"`. World's-first as scripting-lang stdlib
/// builtin (Python's `difflib` is closest but uses ratcliff-obershelp
/// not Myers; Node / Ruby / Perl need third-party).
pub(crate) fn builtin_myers_diff(args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    let a = list_arg_as_strings(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = list_arg_as_strings(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let ops = myers_diff(&a, &b);
    let typed: Vec<(char, String)> = ops.into_iter().collect();
    Ok(ops_to_value(typed))
}

/// `patience_diff(\@A, \@B)` — Bram Cohen's patience diff for
/// human-readable code diffs (unique-anchor LCS + Myers in the gaps).
pub(crate) fn builtin_patience_diff(
    args: &[StrykeValue],
    _line: usize,
) -> StrykeResult<StrykeValue> {
    let a = list_arg_as_strings(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = list_arg_as_strings(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let ops = patience_diff(&a, &b);
    let typed: Vec<(char, String)> = ops.into_iter().collect();
    Ok(ops_to_value(typed))
}

pub(crate) fn builtin_damerau_levenshtein(
    args: &[StrykeValue],
    _line: usize,
) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    Ok(StrykeValue::integer(damerau_levenshtein(&a, &b) as i64))
}

pub(crate) fn builtin_bloom_deserialize(
    args: &[StrykeValue],
    _line: usize,
) -> StrykeResult<StrykeValue> {
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

// ─────────────────────────────────────────────────────────────────────
// Sketch algebra — operator overloads on probabilistic data structures.
//
// World-first: no other language ships `bf1 + bf2` (Bloom union),
// `hll1 + hll2` (HLL merge), `cms1 + cms2` (counter sum),
// `tk1 + tk2` (SpaceSaving merge), `td1 + td2` (centroid merge),
// `rb1 | rb2` / `& ^ -` (Roaring set algebra) as syntactic primitives.
// Everywhere else these are library function calls.
//
// Returned value is a *new* sketch — operators are functional, never
// mutate either operand. Sketches all derive Clone so the deep copy is
// cheap (Vec / HashMap clone, no allocator churn beyond that).
// ─────────────────────────────────────────────────────────────────────

/// Which arithmetic / bitwise op the caller is dispatching.
#[derive(Copy, Clone, Debug)]
pub enum SketchOp {
    /// `+` — union for Bloom/HLL/Roaring, sum for CMS, merge for TopK/TDigest.
    Add,
    /// `|` — set union (Bloom/HLL/Roaring).
    Or,
    /// `&` — set intersection (Roaring only — Bloom/HLL union-only).
    And,
    /// `^` — symmetric difference (Roaring only).
    Xor,
    /// `-` — andnot / set difference (Roaring only).
    Sub,
}

/// Try to dispatch a binary operator on two stryke values as a sketch
/// algebra op. Returns `Some(new_sketch_value)` if both operands are
/// matching sketch HeapObjects, `None` otherwise so the caller can fall
/// back to its default numeric / bitwise path.
///
/// All branches lock the operand mutexes briefly, clone the inner sketch
/// data, run the merge on the clone, and wrap the result in a fresh
/// `Arc<Mutex<…>>`. Operators do not mutate either operand.
pub fn try_sketch_binop(op: SketchOp, a: &StrykeValue, b: &StrykeValue) -> Option<StrykeValue> {
    // Bloom + Bloom → union. Also valid for `|`.
    if matches!(op, SketchOp::Add | SketchOp::Or) {
        if let (Some(la), Some(lb)) = (a.as_bloom_filter(), b.as_bloom_filter()) {
            let mut out = la.lock().clone();
            let other = lb.lock().clone();
            if !out.merge(&other) {
                return None; // shape mismatch — fall through to numeric add
            }
            return Some(StrykeValue::bloom_filter(Arc::new(Mutex::new(out))));
        }
    }
    // HLL + HLL → union. Also valid for `|`.
    if matches!(op, SketchOp::Add | SketchOp::Or) {
        if let (Some(la), Some(lb)) = (a.as_hll_sketch(), b.as_hll_sketch()) {
            let mut out = la.lock().clone();
            let other = lb.lock().clone();
            if !out.merge(&other) {
                return None;
            }
            return Some(StrykeValue::hll_sketch(Arc::new(Mutex::new(out))));
        }
    }
    // CMS + CMS → pointwise counter sum (only `+` makes semantic sense).
    if matches!(op, SketchOp::Add) {
        if let (Some(la), Some(lb)) = (a.as_cms_sketch(), b.as_cms_sketch()) {
            let mut out = la.lock().clone();
            let other = lb.lock().clone();
            if !out.merge(&other) {
                return None;
            }
            return Some(StrykeValue::cms_sketch(Arc::new(Mutex::new(out))));
        }
    }
    // TopK + TopK → SpaceSaving merge.
    if matches!(op, SketchOp::Add) {
        if let (Some(la), Some(lb)) = (a.as_topk_sketch(), b.as_topk_sketch()) {
            let mut out = la.lock().clone();
            let other = lb.lock().clone();
            if !out.merge(&other) {
                return None;
            }
            return Some(StrykeValue::topk_sketch(Arc::new(Mutex::new(out))));
        }
    }
    // TDigest + TDigest → centroid merge.
    if matches!(op, SketchOp::Add) {
        if let (Some(la), Some(lb)) = (a.as_tdigest_sketch(), b.as_tdigest_sketch()) {
            let mut out = la.lock().clone();
            let mut other = lb.lock().clone();
            out.merge(&mut other);
            return Some(StrykeValue::tdigest_sketch(Arc::new(Mutex::new(out))));
        }
    }
    // Roaring — full set algebra: `+` and `|` union, `&` intersect, `^` xor, `-` andnot.
    if let (Some(la), Some(lb)) = (a.as_roaring_bitmap(), b.as_roaring_bitmap()) {
        let mut out = la.lock().clone();
        let other = lb.lock().clone();
        match op {
            SketchOp::Add | SketchOp::Or => out.union_with(&other),
            SketchOp::And => out.intersect_with(&other),
            SketchOp::Xor => out.xor_with(&other),
            SketchOp::Sub => out.andnot_with(&other),
        }
        return Some(StrykeValue::roaring_bitmap(Arc::new(Mutex::new(out))));
    }
    None
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
