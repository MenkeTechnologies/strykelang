//! rkyv-backed KV store — first-class CRUD store in any scripting language.
//!
//! World-first: no other scripting language ships a zero-copy archive KV
//! as core builtins. Python has `shelve` (pickle), Ruby has `PStore`
//! (Marshal), Perl has `DBM_File` (BerkeleyDB), Node has `level` (bindings
//! to LevelDB). Every one pays a parse + allocate per read. stryke's
//! `kv_get` is `mmap + validate + cast` — same code path as
//! `script_cache.rs:454` already uses for cached bytecode.
//!
//! Storage model: **Option 1 — pure rkyv file.** One `KvRoot` archive per
//! store, mmap'd on open, in-memory `HashMap` mirror for mutation,
//! atomic rewrite on commit (tmp + rename — same primitives as
//! `script_cache::write_shard_atomic`). Simple, zero new deps, beats
//! SQLite on reads at any store size that fits comfortably in RAM.
//! LSM-backed backend (sled/redb) is a v2 swap-in behind the same
//! builtins.
//!
//! Wire format: archived `KvRoot` is the network frame body too —
//! Phase 2 `stryke kvd` server speaks the same bytes over TCP.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;

/// Magic header bytes — fail-fast on wrong-format file.
pub const KV_MAGIC: u32 = 0x53544b56; // "STKV"
/// Bumped on incompatible rkyv schema changes (endgame: format-versioned).
pub const KV_FORMAT_VERSION: u32 = 1;

// ── rkyv-archived value type ──────────────────────────────────────────

/// Wire-/disk-side stryke value. Mirrors the subset of `StrykeValue` that
/// makes sense over the wire and on disk. NaN-boxed `Arc<HeapObject>`
/// can't round-trip directly, so we project to this enum at the boundary.
///
/// Anything not representable below is converted by `into_stryke`
/// stringifying or by `from_stryke` returning `Undef`.
///
/// `Array` / `Hash` are recursive — rkyv needs `#[omit_bounds]` on those
/// fields to break the cycle in the derive macro's bound generation.
#[derive(Archive, RkyvDeserialize, RkyvSerialize, Debug, Clone)]
#[archive(check_bytes)]
#[archive(bound(serialize = "__S: rkyv::ser::Serializer + rkyv::ser::ScratchSpace",))]
#[archive_attr(check_bytes(
    bound = "__C: rkyv::validation::ArchiveContext, <__C as rkyv::Fallible>::Error: std::error::Error"
))]
/// `WireValue` — see variants.
pub enum WireValue {
    /// `Undef` variant.
    Undef,
    /// `Bool` variant.
    Bool(bool),
    /// `Int` variant.
    Int(i64),
    /// `Float` variant.
    Float(f64),
    /// `Str` variant.
    Str(String),
    /// `Bytes` variant.
    Bytes(Vec<u8>),
    Array(
        #[omit_bounds]
        #[archive_attr(omit_bounds)]
        Vec<WireValue>,
    ),
    Hash(
        #[omit_bounds]
        #[archive_attr(omit_bounds)]
        Vec<(String, WireValue)>,
    ),
}

impl WireValue {
    /// Project a stryke value into a wire value. Lossy for HeapObjects
    /// other than arrays/hashes/strings — they stringify via `ref_type()`.
    pub fn from_stryke(v: &StrykeValue) -> Self {
        if v.is_undef() {
            return WireValue::Undef;
        }
        if let Some(n) = v.as_integer() {
            return WireValue::Int(n);
        }
        if let Some(f) = v.as_float() {
            return WireValue::Float(f);
        }
        if let Some(b) = v.as_bytes_arc() {
            return WireValue::Bytes((*b).clone());
        }
        if let Some(s) = v.as_str() {
            return WireValue::Str(s.to_string());
        }
        // arrayref `[1,2,3]` — collapse to Array.
        if let Some(ar) = v.as_array_ref() {
            let g = ar.read();
            return WireValue::Array(g.iter().map(WireValue::from_stryke).collect());
        }
        // hashref `{ k => v }` — collapse to Hash.
        if let Some(hr) = v.as_hash_ref() {
            let g = hr.read();
            let mut entries: Vec<(String, WireValue)> = g
                .iter()
                .map(|(k, val)| (k.clone(), WireValue::from_stryke(val)))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            return WireValue::Hash(entries);
        }
        if let Some(arr) = v.as_array_vec() {
            return WireValue::Array(arr.iter().map(WireValue::from_stryke).collect());
        }
        if let Some(h) = v.as_hash_map() {
            let mut entries: Vec<(String, WireValue)> = h
                .iter()
                .map(|(k, val)| (k.clone(), WireValue::from_stryke(val)))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            return WireValue::Hash(entries);
        }
        // Fall back to stringified form so we never lose a value silently.
        WireValue::Str(v.to_string())
    }

    /// Materialize a stryke value from the wire representation.
    pub fn into_stryke(self) -> StrykeValue {
        match self {
            WireValue::Undef => StrykeValue::UNDEF,
            WireValue::Bool(b) => StrykeValue::integer(if b { 1 } else { 0 }),
            WireValue::Int(n) => StrykeValue::integer(n),
            WireValue::Float(f) => StrykeValue::float(f),
            WireValue::Str(s) => StrykeValue::string(s),
            WireValue::Bytes(b) => StrykeValue::bytes(Arc::new(b)),
            WireValue::Array(items) => {
                // Return an arrayref so Perl `->[i]` arrow deref works
                // ergonomically: `my $row = kv_get($db, "k"); $row->[0]`.
                let v: Vec<StrykeValue> = items.into_iter().map(|x| x.into_stryke()).collect();
                StrykeValue::array_ref(Arc::new(RwLock::new(v)))
            }
            WireValue::Hash(pairs) => {
                // Return a hashref so Perl `->{k}` arrow deref works
                // ergonomically: `my $u = kv_get($db, "user"); $u->{name}`.
                let mut m: IndexMap<String, StrykeValue> = IndexMap::with_capacity(pairs.len());
                for (k, v) in pairs {
                    m.insert(k, v.into_stryke());
                }
                StrykeValue::hash_ref(Arc::new(RwLock::new(m)))
            }
        }
    }
}

// ── rkyv-archived store root ─────────────────────────────────────────
/// `KvHeader` — see fields for layout.
#[derive(Archive, RkyvDeserialize, RkyvSerialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct KvHeader {
    /// `magic` field.
    pub magic: u32,
    /// `format_version` field.
    pub format_version: u32,
    /// `stryke_version` field.
    pub stryke_version: String,
    /// `created_at_secs` field.
    pub created_at_secs: u64,
    /// `last_commit_secs` field.
    pub last_commit_secs: u64,
    /// `commit_count` field.
    pub commit_count: u64,
}

impl Default for KvHeader {
    fn default() -> Self {
        Self {
            magic: KV_MAGIC,
            format_version: KV_FORMAT_VERSION,
            stryke_version: env!("CARGO_PKG_VERSION").to_string(),
            created_at_secs: now_secs(),
            last_commit_secs: 0,
            commit_count: 0,
        }
    }
}
/// `KvRoot` — see fields for layout.
#[derive(Archive, RkyvDeserialize, RkyvSerialize, Debug, Clone, Default)]
#[archive(check_bytes)]
pub struct KvRoot {
    /// `header` field.
    pub header: KvHeader,
    /// `entries` field.
    pub entries: HashMap<String, WireValue>,
}

// ── In-memory handle ──────────────────────────────────────────────────

/// One open KV store. Holds the owned-deserialized root in memory and
/// tracks a dirty flag; `kv_commit` writes back atomically. Multiple
/// builtins share the same `Arc<Mutex<KvStore>>` so concurrent
/// `kv_put`/`kv_get` from different threads is safe.
#[derive(Debug)]
pub struct KvStore {
    /// `path` field.
    pub path: PathBuf,
    /// `root` field.
    pub root: KvRoot,
    /// `dirty` field.
    pub dirty: bool,
}

impl KvStore {
    /// Open or create a store at `path`. Missing file → empty store
    /// (no I/O until first commit). Existing file → mmap + check +
    /// deserialize into owned form (we eagerly own because mutation
    /// requires a `HashMap` we can grow).
    pub fn open(path: impl Into<PathBuf>) -> StrykeResult<Self> {
        let path = path.into();
        if !path.exists() {
            return Ok(Self {
                path,
                root: KvRoot::default(),
                dirty: false,
            });
        }
        let bytes = std::fs::read(&path).map_err(|e| {
            StrykeError::runtime(format!("kv_open: read {}: {}", path.display(), e), 0)
        })?;
        let archived = rkyv::check_archived_root::<KvRoot>(&bytes[..]).map_err(|e| {
            StrykeError::runtime(
                format!(
                    "kv_open: corrupt or wrong-format file {}: {}",
                    path.display(),
                    e
                ),
                0,
            )
        })?;
        if archived.header.magic != KV_MAGIC {
            return Err(StrykeError::runtime(
                format!("kv_open: bad magic in {}", path.display()),
                0,
            ));
        }
        if archived.header.format_version != KV_FORMAT_VERSION {
            return Err(StrykeError::runtime(
                format!(
                    "kv_open: format version {} (expected {})",
                    archived.header.format_version, KV_FORMAT_VERSION
                ),
                0,
            ));
        }
        let root: KvRoot = archived
            .deserialize(&mut rkyv::Infallible)
            .map_err(|_| StrykeError::runtime("kv_open: deserialize failed", 0))?;
        Ok(Self {
            path,
            root,
            dirty: false,
        })
    }
    /// `put` — see implementation.
    pub fn put(&mut self, key: String, value: WireValue) {
        self.root.entries.insert(key, value);
        self.dirty = true;
    }
    /// `get` — see implementation.
    pub fn get(&self, key: &str) -> Option<&WireValue> {
        self.root.entries.get(key)
    }
    /// `del` — see implementation.
    pub fn del(&mut self, key: &str) -> bool {
        let existed = self.root.entries.remove(key).is_some();
        if existed {
            self.dirty = true;
        }
        existed
    }
    /// `exists` — see implementation.
    pub fn exists(&self, key: &str) -> bool {
        self.root.entries.contains_key(key)
    }
    /// `len` — see implementation.
    pub fn len(&self) -> usize {
        self.root.entries.len()
    }
    /// `is_empty` — see implementation.
    pub fn is_empty(&self) -> bool {
        self.root.entries.is_empty()
    }

    /// All keys, sorted lexicographically. Optional `prefix` filter.
    pub fn keys(&self, prefix: Option<&str>) -> Vec<String> {
        let mut ks: Vec<String> = match prefix {
            Some(p) => self
                .root
                .entries
                .keys()
                .filter(|k| k.starts_with(p))
                .cloned()
                .collect(),
            None => self.root.entries.keys().cloned().collect(),
        };
        ks.sort_unstable();
        ks
    }

    /// Atomic rewrite of the whole archive to disk. No-op if not dirty
    /// (cheap to call after every batch). Mirrors
    /// `script_cache::write_shard_atomic`.
    pub fn commit(&mut self) -> StrykeResult<()> {
        if !self.dirty {
            return Ok(());
        }
        self.root.header.last_commit_secs = now_secs();
        self.root.header.commit_count = self.root.header.commit_count.saturating_add(1);
        let bytes = rkyv::to_bytes::<_, 4096>(&self.root)
            .map_err(|e| StrykeError::runtime(format!("kv_commit: rkyv: {}", e), 0))?;

        let parent = self
            .path
            .parent()
            .ok_or_else(|| StrykeError::runtime("kv_commit: path has no parent", 0))?;
        let _ = std::fs::create_dir_all(parent);

        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let fname = self
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("store.rkyv");
        let tmp_path = parent.join(format!("{}.tmp.{}.{}", fname, pid, nanos));

        {
            let mut f = File::create(&tmp_path)
                .map_err(|e| StrykeError::runtime(format!("kv_commit: tmp create: {}", e), 0))?;
            f.write_all(&bytes)
                .map_err(|e| StrykeError::runtime(format!("kv_commit: tmp write: {}", e), 0))?;
            f.sync_all()
                .map_err(|e| StrykeError::runtime(format!("kv_commit: tmp fsync: {}", e), 0))?;
        }

        std::fs::rename(&tmp_path, &self.path)
            .map_err(|e| StrykeError::runtime(format!("kv_commit: rename: {}", e), 0))?;
        self.dirty = false;
        Ok(())
    }
    /// `stats` — see implementation.
    pub fn stats(&self) -> Vec<(String, StrykeValue)> {
        vec![
            (
                "path".into(),
                StrykeValue::string(self.path.display().to_string()),
            ),
            (
                "entries".into(),
                StrykeValue::integer(self.root.entries.len() as i64),
            ),
            (
                "dirty".into(),
                StrykeValue::integer(if self.dirty { 1 } else { 0 }),
            ),
            (
                "format_version".into(),
                StrykeValue::integer(self.root.header.format_version as i64),
            ),
            (
                "created_at_secs".into(),
                StrykeValue::integer(self.root.header.created_at_secs as i64),
            ),
            (
                "last_commit_secs".into(),
                StrykeValue::integer(self.root.header.last_commit_secs as i64),
            ),
            (
                "commit_count".into(),
                StrykeValue::integer(self.root.header.commit_count as i64),
            ),
            (
                "stryke_version".into(),
                StrykeValue::string(self.root.header.stryke_version.clone()),
            ),
        ]
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Stryke builtin handlers ───────────────────────────────────────────

fn store_arg(v: &StrykeValue, fn_name: &str, line: usize) -> StrykeResult<Arc<Mutex<KvStore>>> {
    v.as_kv_store().ok_or_else(|| {
        StrykeError::runtime(
            format!("{}: first argument must be a KvStore handle", fn_name),
            line,
        )
    })
}

fn key_arg(v: &StrykeValue) -> String {
    v.to_string()
}

/// Unify Array / ArrayRef access so callers can pass either an inline
/// list or an arrayref literal. Returns the underlying `Vec<StrykeValue>`.
fn as_any_array(v: &StrykeValue) -> Option<Vec<StrykeValue>> {
    if let Some(ar) = v.as_array_ref() {
        return Some(ar.read().clone());
    }
    v.as_array_vec()
}

/// `kv_open(path)` — open or create a rkyv-backed KV store.
pub(crate) fn builtin_kv_open(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let path_v = args
        .first()
        .ok_or_else(|| StrykeError::runtime("kv_open: missing path argument", line))?;
    let path = path_v.to_string();
    let store = KvStore::open(Path::new(&path))
        .map_err(|e| StrykeError::runtime(format!("kv_open: {}", e.message), line))?;
    Ok(StrykeValue::kv_store(Arc::new(Mutex::new(store))))
}

/// `kv_put(store, key, value)` — write key→value. Returns the old value
/// or undef.
pub(crate) fn builtin_kv_put(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "kv_put", line)?;
    let k = key_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let v = args.get(2).cloned().unwrap_or(StrykeValue::UNDEF);
    let wv = WireValue::from_stryke(&v);
    let prev = {
        let mut g = s.lock();
        let prev = g.get(&k).cloned();
        g.put(k, wv);
        prev
    };
    Ok(prev.map(|p| p.into_stryke()).unwrap_or(StrykeValue::UNDEF))
}

/// `kv_get(store, key)` — return the value or undef.
pub(crate) fn builtin_kv_get(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "kv_get", line)?;
    let k = key_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let g = s.lock();
    Ok(g.get(&k)
        .cloned()
        .map(|v| v.into_stryke())
        .unwrap_or(StrykeValue::UNDEF))
}

/// `kv_del(store, key)` — delete key; returns 1 if existed, 0 otherwise.
pub(crate) fn builtin_kv_del(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "kv_del", line)?;
    let k = key_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let existed = s.lock().del(&k);
    Ok(StrykeValue::integer(if existed { 1 } else { 0 }))
}

/// `kv_exists(store, key)` — 1 if key exists, 0 otherwise.
pub(crate) fn builtin_kv_exists(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "kv_exists",
        line,
    )?;
    let k = key_arg(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let yes = s.lock().exists(&k);
    Ok(StrykeValue::integer(if yes { 1 } else { 0 }))
}

/// `kv_keys(store [, prefix])` — sorted keys, optional prefix filter.
pub(crate) fn builtin_kv_keys(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "kv_keys", line)?;
    let prefix = args.get(1).map(|v| v.to_string());
    let keys = s.lock().keys(prefix.as_deref());
    let arr: Vec<StrykeValue> = keys.into_iter().map(StrykeValue::string).collect();
    Ok(StrykeValue::array(arr))
}

/// `kv_scan(store, prefix)` — return an array of `[key, value]` pairs
/// for every key starting with `prefix`. Lazy iterator form lands when
/// the Phase 2 wire transport ships streaming chunks.
pub(crate) fn builtin_kv_scan(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "kv_scan", line)?;
    let prefix = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let g = s.lock();
    let mut pairs: Vec<(String, StrykeValue)> = g
        .root
        .entries
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix))
        .map(|(k, v)| (k.clone(), v.clone().into_stryke()))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let arr: Vec<StrykeValue> = pairs
        .into_iter()
        .map(|(k, v)| {
            // Each pair is `[key, value]` as an arrayref so `$row->[0]` /
            // `$row->[1]` work directly.
            StrykeValue::array_ref(Arc::new(RwLock::new(vec![StrykeValue::string(k), v])))
        })
        .collect();
    Ok(StrykeValue::array(arr))
}

/// `kv_len(store)` — number of entries.
pub(crate) fn builtin_kv_len(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(args.first().unwrap_or(&StrykeValue::UNDEF), "kv_len", line)?;
    let n = s.lock().len() as i64;
    Ok(StrykeValue::integer(n))
}

/// `kv_commit(store)` — flush in-memory state to disk atomically.
pub(crate) fn builtin_kv_commit(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "kv_commit",
        line,
    )?;
    s.lock()
        .commit()
        .map_err(|e| StrykeError::runtime(format!("kv_commit: {}", e.message), line))?;
    Ok(StrykeValue::integer(1))
}

/// `kv_batch(store, [["put",k,v],["del",k],...])` — apply ops in order,
/// all-or-nothing on the in-memory state. Caller invokes `kv_commit`
/// afterwards for durability.
pub(crate) fn builtin_kv_batch(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "kv_batch",
        line,
    )?;
    let ops_v = args
        .get(1)
        .ok_or_else(|| StrykeError::runtime("kv_batch: missing ops array", line))?;
    let ops = as_any_array(ops_v)
        .ok_or_else(|| StrykeError::runtime("kv_batch: ops must be an array of triples", line))?;

    // Snapshot the entries map so we can roll back if any op rejects.
    let snapshot = s.lock().root.entries.clone();
    let mut applied: usize = 0;
    let result: StrykeResult<usize> = (|| {
        for (i, op_v) in ops.iter().enumerate() {
            let op_arr = as_any_array(op_v).ok_or_else(|| {
                StrykeError::runtime(format!("kv_batch: op {} is not an array", i), line)
            })?;
            let kind = op_arr.first().map(|x| x.to_string()).unwrap_or_default();
            match kind.as_str() {
                "put" => {
                    let k = op_arr.get(1).map(|v| v.to_string()).ok_or_else(|| {
                        StrykeError::runtime(format!("kv_batch: op {}: put missing key", i), line)
                    })?;
                    let v = op_arr.get(2).cloned().unwrap_or(StrykeValue::UNDEF);
                    s.lock().put(k, WireValue::from_stryke(&v));
                }
                "del" => {
                    let k = op_arr.get(1).map(|v| v.to_string()).ok_or_else(|| {
                        StrykeError::runtime(format!("kv_batch: op {}: del missing key", i), line)
                    })?;
                    s.lock().del(&k);
                }
                other => {
                    return Err(StrykeError::runtime(
                        format!("kv_batch: op {}: unknown kind '{}'", i, other),
                        line,
                    ));
                }
            }
            applied += 1;
        }
        Ok(applied)
    })();

    match result {
        Ok(n) => Ok(StrykeValue::integer(n as i64)),
        Err(e) => {
            // Roll back.
            let mut g = s.lock();
            g.root.entries = snapshot;
            g.dirty = !g.root.entries.is_empty();
            Err(e)
        }
    }
}

/// `kv_close(store)` — auto-commits if dirty, then no-op (the Arc drops
/// on the last reference). Returns 1 always.
pub(crate) fn builtin_kv_close(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "kv_close",
        line,
    )?;
    let mut g = s.lock();
    if g.dirty {
        g.commit()
            .map_err(|e| StrykeError::runtime(format!("kv_close: {}", e.message), line))?;
    }
    Ok(StrykeValue::integer(1))
}

/// `kv_stats(store)` — return a hash of store metadata.
pub(crate) fn builtin_kv_stats(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let s = store_arg(
        args.first().unwrap_or(&StrykeValue::UNDEF),
        "kv_stats",
        line,
    )?;
    let pairs = s.lock().stats();
    let mut m: IndexMap<String, StrykeValue> = IndexMap::with_capacity(pairs.len());
    for (k, v) in pairs {
        m.insert(k, v);
    }
    Ok(StrykeValue::hash_ref(Arc::new(RwLock::new(m))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("stryke_kvtest_{}_{}.rkyv", name, nanos));
        p
    }

    #[test]
    fn put_get_roundtrip() {
        let p = tmp_path("rt");
        let mut s = KvStore::open(&p).unwrap();
        s.put("alpha".into(), WireValue::Int(42));
        s.put("beta".into(), WireValue::Str("hello".into()));
        assert!(matches!(s.get("alpha"), Some(WireValue::Int(42))));
        assert!(matches!(s.get("beta"), Some(WireValue::Str(_))));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn commit_then_reopen_sees_data() {
        let p = tmp_path("commit");
        {
            let mut s = KvStore::open(&p).unwrap();
            s.put("k1".into(), WireValue::Int(1));
            s.put("k2".into(), WireValue::Int(2));
            s.commit().unwrap();
        }
        {
            let s = KvStore::open(&p).unwrap();
            assert_eq!(s.len(), 2);
            assert!(matches!(s.get("k1"), Some(WireValue::Int(1))));
            assert!(matches!(s.get("k2"), Some(WireValue::Int(2))));
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn keys_prefix_filter_sorted() {
        let p = tmp_path("keys");
        let mut s = KvStore::open(&p).unwrap();
        s.put("user:1".into(), WireValue::Int(1));
        s.put("user:2".into(), WireValue::Int(2));
        s.put("log:1".into(), WireValue::Int(99));
        let ks = s.keys(Some("user:"));
        assert_eq!(ks, vec!["user:1".to_string(), "user:2".to_string()]);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn del_returns_existed() {
        let p = tmp_path("del");
        let mut s = KvStore::open(&p).unwrap();
        s.put("x".into(), WireValue::Int(1));
        assert!(s.del("x"));
        assert!(!s.del("x"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn nested_array_roundtrip() {
        let p = tmp_path("nested");
        let mut s = KvStore::open(&p).unwrap();
        let nested = WireValue::Array(vec![
            WireValue::Int(1),
            WireValue::Array(vec![WireValue::Str("a".into()), WireValue::Int(2)]),
            WireValue::Hash(vec![("k".into(), WireValue::Int(3))]),
        ]);
        s.put("nest".into(), nested);
        s.commit().unwrap();
        let s2 = KvStore::open(&p).unwrap();
        match s2.get("nest") {
            Some(WireValue::Array(items)) => {
                assert_eq!(items.len(), 3);
            }
            _ => panic!("expected array"),
        }
        let _ = std::fs::remove_file(&p);
    }
}
