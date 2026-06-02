use crossbeam::channel::{Receiver, Sender};
use indexmap::IndexMap;
use num_bigint::BigInt;
use parking_lot::{Mutex, RwLock};
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::sync::Barrier;

use crate::ast::{Block, ClassDef, EnumDef, StructDef, SubSigParam};
use crate::error::StrykeResult;
use crate::nanbox;
use crate::perl_decode::decode_utf8_or_latin1;
use crate::perl_regex::PerlCompiledRegex;

/// Handle returned by `async { ... }` / `spawn { ... }`; join with `await`.
#[derive(Debug)]
pub struct StrykeAsyncTask {
    pub(crate) result: Arc<Mutex<Option<StrykeResult<StrykeValue>>>>,
    pub(crate) join: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl Clone for StrykeAsyncTask {
    fn clone(&self) -> Self {
        Self {
            result: self.result.clone(),
            join: self.join.clone(),
        }
    }
}

impl StrykeAsyncTask {
    /// Join the worker thread (once) and return the block's value or error.
    pub fn await_result(&self) -> StrykeResult<StrykeValue> {
        if let Some(h) = self.join.lock().take() {
            let _ = h.join();
        }
        self.result
            .lock()
            .clone()
            .unwrap_or_else(|| Ok(StrykeValue::UNDEF))
    }
}

// ── Lazy iterator protocol (`|>` streaming) ─────────────────────────────────

/// Pull-based lazy iterator.  Sources (`frs`, `drs`) produce one; transform
/// stages (`rev`) wrap one; terminals (`e`/`fore`) consume one item at a time.
pub trait StrykeIterator: Send + Sync {
    /// Return the next item, or `None` when exhausted.
    fn next_item(&self) -> Option<StrykeValue>;

    /// Collect all remaining items into a `Vec`.
    fn collect_all(&self) -> Vec<StrykeValue> {
        let mut out = Vec::new();
        while let Some(v) = self.next_item() {
            out.push(v);
        }
        out
    }
}

impl fmt::Debug for dyn StrykeIterator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PerlIterator")
    }
}

/// Lazy recursive file walker — yields one relative path per `next_item()` call.
pub struct FsWalkIterator {
    /// `(base_path, relative_prefix)` stack.
    stack: Mutex<Vec<(std::path::PathBuf, String)>>,
    /// Buffered sorted entries from the current directory level.
    buf: Mutex<Vec<(String, bool)>>, // (child_rel, is_dir)
    /// Pending subdirs to push (reversed, so first is popped next).
    pending_dirs: Mutex<Vec<(std::path::PathBuf, String)>>,
    /// `files_only` field.
    files_only: bool,
}

impl FsWalkIterator {
    /// `new` — see implementation.
    pub fn new(dir: &str, files_only: bool) -> Self {
        Self {
            stack: Mutex::new(vec![(std::path::PathBuf::from(dir), String::new())]),
            buf: Mutex::new(Vec::new()),
            pending_dirs: Mutex::new(Vec::new()),
            files_only,
        }
    }

    /// Refill `buf` from the next directory on the stack.
    /// Loops until items are found or the stack is fully exhausted.
    fn refill(&self) -> bool {
        loop {
            let mut stack = self.stack.lock();
            // Push any pending subdirs from the previous level.
            let mut pending = self.pending_dirs.lock();
            while let Some(d) = pending.pop() {
                stack.push(d);
            }
            drop(pending);

            let (base, rel) = match stack.pop() {
                Some(v) => v,
                None => return false,
            };
            drop(stack);

            let entries = match std::fs::read_dir(&base) {
                Ok(e) => e,
                Err(_) => continue, // skip unreadable, try next
            };
            let mut children: Vec<(std::ffi::OsString, String, bool, bool)> = Vec::new();
            for entry in entries.flatten() {
                let ft = match entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                let os_name = entry.file_name();
                let name = match os_name.to_str() {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let child_rel = if rel.is_empty() {
                    name.clone()
                } else {
                    format!("{rel}/{name}")
                };
                children.push((os_name, child_rel, ft.is_file(), ft.is_dir()));
            }
            children.sort_by(|a, b| a.0.cmp(&b.0));

            let mut buf = self.buf.lock();
            let mut pending = self.pending_dirs.lock();
            let mut subdirs = Vec::new();
            for (os_name, child_rel, is_file, is_dir) in children {
                if is_dir {
                    if !self.files_only {
                        buf.push((child_rel.clone(), true));
                    }
                    subdirs.push((base.join(os_name), child_rel));
                } else if is_file && self.files_only {
                    buf.push((child_rel, false));
                }
            }
            for s in subdirs.into_iter().rev() {
                pending.push(s);
            }
            buf.reverse();
            if !buf.is_empty() {
                return true;
            }
            // buf empty but pending_dirs may have subdirs to explore — loop.
        }
    }
}

impl StrykeIterator for FsWalkIterator {
    fn next_item(&self) -> Option<StrykeValue> {
        loop {
            {
                let mut buf = self.buf.lock();
                if let Some((path, _)) = buf.pop() {
                    return Some(StrykeValue::string(path));
                }
            }
            if !self.refill() {
                return None;
            }
        }
    }
}

/// Reverses the source iterator's *sequence* of items. Drains lazily on the
/// first `next_item` call — `rev` cannot stream, since the last item must
/// be produced first.
///
/// Don't be tempted to per-item `chars().rev()` here: that's `scalar reverse`
/// at the item level, not list reversal. `~> $s chars rev` and friends rely
/// on this reversing the sequence (`a,b,c,d` → `d,c,b,a`).
pub struct RevIterator {
    /// `source` field.
    source: Arc<dyn StrykeIterator>,
    /// `drained` field.
    drained: Mutex<Option<Vec<StrykeValue>>>,
}

impl RevIterator {
    /// `new` — see implementation.
    pub fn new(source: Arc<dyn StrykeIterator>) -> Self {
        Self {
            source,
            drained: Mutex::new(None),
        }
    }
}

impl StrykeIterator for RevIterator {
    fn next_item(&self) -> Option<StrykeValue> {
        let mut g = self.drained.lock();
        if g.is_none() {
            let mut buf = Vec::new();
            while let Some(v) = self.source.next_item() {
                buf.push(v);
            }
            *g = Some(buf);
        }
        // Pop yields items in reverse order (last → first), which IS the
        // reversal we want.
        g.as_mut().and_then(|v| v.pop())
    }
}

/// Lazy generator from `gen { }`; resume with `->next` on the value.
#[derive(Debug)]
pub struct PerlGenerator {
    pub(crate) block: Block,
    pub(crate) pc: Mutex<usize>,
    pub(crate) scope_started: Mutex<bool>,
    pub(crate) exhausted: Mutex<bool>,
}

/// `Set->new` storage: canonical key → member value (insertion order preserved).
pub type PerlSet = IndexMap<String, StrykeValue>;

/// Min-heap ordered by a Perl comparator (`$a` / `$b` in scope, like `sort { }`).
#[derive(Debug, Clone)]
pub struct PerlHeap {
    /// `items` field.
    pub items: Vec<StrykeValue>,
    /// `cmp` field.
    pub cmp: Arc<StrykeSub>,
}

/// Exclusive mutex backing `StrykeValue::Mutex`. Locks are advisory: the
/// `mutex_lock` / `mutex_unlock` builtins toggle the `held` flag under the
/// inner `parking_lot::Mutex`, and contention parks waiters on `condvar`
/// (NOT a busy spin). This separation keeps any [`parking_lot::MutexGuard`]
/// strictly inside the builtin function — guards never live in a
/// [`StrykeValue`] across VM dispatch boundaries.
#[derive(Debug)]
pub struct MutexHandle {
    /// `held` field.
    pub held: parking_lot::Mutex<bool>,
    /// `condvar` field.
    pub condvar: parking_lot::Condvar,
}

impl MutexHandle {
    /// `new` — see implementation.
    pub fn new() -> Self {
        Self {
            held: parking_lot::Mutex::new(false),
            condvar: parking_lot::Condvar::new(),
        }
    }
}

impl Default for MutexHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Counting semaphore backing `StrykeValue::Semaphore`. `permits` tracks the
/// current available count (`permits >= 0` always); `limit` is the initial
/// `semaphore(N)` capacity (kept for reporting via `semaphore_limit`).
/// Acquire blocks on `condvar` until a permit becomes available; release
/// notifies one waiter.
#[derive(Debug)]
pub struct SemaphoreHandle {
    /// `permits` field.
    pub permits: parking_lot::Mutex<i64>,
    /// `limit` field.
    pub limit: i64,
    /// `condvar` field.
    pub condvar: parking_lot::Condvar,
}

impl SemaphoreHandle {
    /// `n` must be `>= 0`; callers ensure this before construction.
    pub fn new(n: i64) -> Self {
        Self {
            permits: parking_lot::Mutex::new(n),
            limit: n,
            condvar: parking_lot::Condvar::new(),
        }
    }
}

/// One SSH worker lane: a single `ssh HOST PE_PATH --remote-worker` process. The persistent
/// dispatcher in [`crate::cluster`] holds one of these per concurrent worker thread.
///
/// `pe_path` is the path to the `stryke` binary on the **remote** host — the basic implementation
/// used `std::env::current_exe()` which is wrong by definition (a local `/Users/...` path
/// rarely exists on a remote machine). Default is the bare string `"stryke"` so the remote
/// host's `$PATH` resolves it like any other ssh command.
#[derive(Debug, Clone)]
pub struct RemoteSlot {
    /// Argument passed to `ssh` (e.g. `host`, `user@host`, `host` with `~/.ssh/config` host alias).
    pub host: String,
    /// Path to `stryke` on the remote host. `"stryke"` resolves via remote `$PATH`.
    pub pe_path: String,
}

#[cfg(test)]
mod cluster_parsing_tests {
    use super::*;

    fn s(v: &str) -> StrykeValue {
        StrykeValue::string(v.to_string())
    }

    #[test]
    fn parses_simple_host() {
        let c = RemoteCluster::from_list_args(&[s("host1")]).expect("parse");
        assert_eq!(c.slots.len(), 1);
        assert_eq!(c.slots[0].host, "host1");
        assert_eq!(c.slots[0].pe_path, "stryke");
    }

    #[test]
    fn parses_host_with_slot_count() {
        let c = RemoteCluster::from_list_args(&[s("host1:4")]).expect("parse");
        assert_eq!(c.slots.len(), 4);
        assert!(c.slots.iter().all(|s| s.host == "host1"));
    }

    #[test]
    fn parses_user_at_host_with_slots() {
        let c = RemoteCluster::from_list_args(&[s("alice@build1:2")]).expect("parse");
        assert_eq!(c.slots.len(), 2);
        assert_eq!(c.slots[0].host, "alice@build1");
    }

    #[test]
    fn parses_host_slots_stryke_path_triple() {
        let c =
            RemoteCluster::from_list_args(&[s("build1:3:/usr/local/bin/stryke")]).expect("parse");
        assert_eq!(c.slots.len(), 3);
        assert!(c.slots.iter().all(|sl| sl.host == "build1"));
        assert!(c
            .slots
            .iter()
            .all(|sl| sl.pe_path == "/usr/local/bin/stryke"));
    }

    #[test]
    fn parses_multiple_hosts_in_one_call() {
        let c = RemoteCluster::from_list_args(&[s("host1:2"), s("host2:1")]).expect("parse");
        assert_eq!(c.slots.len(), 3);
        assert_eq!(c.slots[0].host, "host1");
        assert_eq!(c.slots[1].host, "host1");
        assert_eq!(c.slots[2].host, "host2");
    }

    #[test]
    fn parses_hashref_slot_form() {
        let mut h = indexmap::IndexMap::new();
        h.insert("host".to_string(), s("data1"));
        h.insert("slots".to_string(), StrykeValue::integer(2));
        h.insert("stryke".to_string(), s("/opt/stryke"));
        let c = RemoteCluster::from_list_args(&[StrykeValue::hash(h)]).expect("parse");
        assert_eq!(c.slots.len(), 2);
        assert_eq!(c.slots[0].host, "data1");
        assert_eq!(c.slots[0].pe_path, "/opt/stryke");
    }

    #[test]
    fn parses_trailing_tunables_hashref() {
        let mut tun = indexmap::IndexMap::new();
        tun.insert("timeout".to_string(), StrykeValue::integer(30));
        tun.insert("retries".to_string(), StrykeValue::integer(2));
        tun.insert("connect_timeout".to_string(), StrykeValue::integer(5));
        let c = RemoteCluster::from_list_args(&[s("h1:1"), StrykeValue::hash(tun)]).expect("parse");
        // Tunables hash should NOT be treated as a slot.
        assert_eq!(c.slots.len(), 1);
        assert_eq!(c.job_timeout_ms, 30_000);
        assert_eq!(c.max_attempts, 3); // retries=2 + initial = 3
        assert_eq!(c.connect_timeout_ms, 5_000);
    }

    #[test]
    fn defaults_when_no_tunables() {
        let c = RemoteCluster::from_list_args(&[s("h1")]).expect("parse");
        assert_eq!(c.job_timeout_ms, RemoteCluster::DEFAULT_JOB_TIMEOUT_MS);
        assert_eq!(c.max_attempts, RemoteCluster::DEFAULT_MAX_ATTEMPTS);
        assert_eq!(
            c.connect_timeout_ms,
            RemoteCluster::DEFAULT_CONNECT_TIMEOUT_MS
        );
    }

    #[test]
    fn rejects_empty_cluster() {
        assert!(RemoteCluster::from_list_args(&[]).is_err());
    }

    #[test]
    fn slot_count_minimum_one() {
        let c = RemoteCluster::from_list_args(&[s("h1:0")]).expect("parse");
        // `host:0` clamps to 1 slot — better to give the user something than to silently
        // produce a cluster that does nothing.
        assert_eq!(c.slots.len(), 1);
    }
}

/// SSH worker pool for `pmap_on`. The dispatcher spawns one persistent ssh process per slot,
/// performs HELLO + SESSION_INIT once, then streams JOB frames over the same stdin/stdout.
///
/// **Tunables:**
/// - `job_timeout_ms` — per-job wall-clock budget. A slot that exceeds this is killed and the
///   job is re-enqueued (counted against the retry budget).
/// - `max_attempts` — total attempts (initial + retries) per job before it is failed.
/// - `connect_timeout_ms` — `ssh -o ConnectTimeout=N`-equivalent for the initial handshake.
#[derive(Debug, Clone)]
pub struct RemoteCluster {
    /// `slots` field.
    pub slots: Vec<RemoteSlot>,
    /// `job_timeout_ms` field.
    pub job_timeout_ms: u64,
    /// `max_attempts` field.
    pub max_attempts: u32,
    /// `connect_timeout_ms` field.
    pub connect_timeout_ms: u64,
}

impl RemoteCluster {
    /// `DEFAULT_JOB_TIMEOUT_MS` constant.
    pub const DEFAULT_JOB_TIMEOUT_MS: u64 = 60_000;
    /// `DEFAULT_MAX_ATTEMPTS` constant.
    pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
    /// `DEFAULT_CONNECT_TIMEOUT_MS` constant.
    pub const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 10_000;

    /// Parse a list of cluster spec values into a [`RemoteCluster`]. Accepted forms (any may
    /// appear in the same call):
    ///
    /// - `"host"`                       — 1 slot, default `stryke` path
    /// - `"host:N"`                     — N slots
    /// - `"host:N:/path/to/stryke"`         — N slots, custom remote `stryke`
    /// - `"user@host:N"`                — ssh user override (kept verbatim in `host`)
    /// - hashref `{ host => "h", slots => N, stryke => "/usr/local/bin/stryke" }`
    /// - trailing hashref `{ timeout => 30, retries => 2, connect_timeout => 5 }` — global
    ///   tunables that apply to the whole cluster (must be the **last** argument; consumed
    ///   only when its keys are all known tunable names so it cannot be confused with a slot)
    ///
    /// Backwards compatible with the basic v1 `"host:N"` syntax.
    pub fn from_list_args(items: &[StrykeValue]) -> Result<Self, String> {
        let mut slots: Vec<RemoteSlot> = Vec::new();
        let mut job_timeout_ms = Self::DEFAULT_JOB_TIMEOUT_MS;
        let mut max_attempts = Self::DEFAULT_MAX_ATTEMPTS;
        let mut connect_timeout_ms = Self::DEFAULT_CONNECT_TIMEOUT_MS;

        // Trailing tunable hashref: peel it off if all its keys are known tunable names.
        let (slot_items, tunables) = if let Some(last) = items.last() {
            let h = last
                .as_hash_map()
                .or_else(|| last.as_hash_ref().map(|r| r.read().clone()));
            if let Some(map) = h {
                let known = |k: &str| {
                    matches!(k, "timeout" | "retries" | "connect_timeout" | "job_timeout")
                };
                if !map.is_empty() && map.keys().all(|k| known(k.as_str())) {
                    (&items[..items.len() - 1], Some(map))
                } else {
                    (items, None)
                }
            } else {
                (items, None)
            }
        } else {
            (items, None)
        };

        if let Some(map) = tunables {
            if let Some(v) = map.get("timeout").or_else(|| map.get("job_timeout")) {
                job_timeout_ms = (v.to_number() * 1000.0) as u64;
            }
            if let Some(v) = map.get("retries") {
                // `retries=2` means 2 RETRIES on top of the first attempt → 3 total.
                max_attempts = v.to_int().max(0) as u32 + 1;
            }
            if let Some(v) = map.get("connect_timeout") {
                connect_timeout_ms = (v.to_number() * 1000.0) as u64;
            }
        }

        for it in slot_items {
            // Hashref form: { host => "h", slots => N, stryke => "/path" }
            if let Some(map) = it
                .as_hash_map()
                .or_else(|| it.as_hash_ref().map(|r| r.read().clone()))
            {
                let host = map
                    .get("host")
                    .map(|v| v.to_string())
                    .ok_or_else(|| "cluster: hashref slot needs `host`".to_string())?;
                let n = map.get("slots").map(|v| v.to_int().max(1)).unwrap_or(1) as usize;
                let stryke = map
                    .get("stryke")
                    .or_else(|| map.get("pe_path"))
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "stryke".to_string());
                for _ in 0..n {
                    slots.push(RemoteSlot {
                        host: host.clone(),
                        pe_path: stryke.clone(),
                    });
                }
                continue;
            }

            // String form. Split into up to 3 colon-separated fields, but be careful: a
            // pe_path may itself contain a colon (rare but possible). We use rsplitn(2) to
            // peel off the optional stryke path only when the segment after the second colon
            // looks like a path (starts with `/` or `.`) — otherwise treat the trailing
            // segment as part of the stryke path candidate.
            let s = it.to_string();
            // Heuristic: split into (left = host[:N], pe_path) if the third field is present.
            let (left, pe_path) = if let Some(idx) = s.find(':') {
                // first colon is host:rest
                let rest = &s[idx + 1..];
                if let Some(jdx) = rest.find(':') {
                    // host:N:pe_path
                    let count_seg = &rest[..jdx];
                    if count_seg.parse::<usize>().is_ok() {
                        (
                            format!("{}:{}", &s[..idx], count_seg),
                            Some(rest[jdx + 1..].to_string()),
                        )
                    } else {
                        (s.clone(), None)
                    }
                } else {
                    (s.clone(), None)
                }
            } else {
                (s.clone(), None)
            };
            let pe_path = pe_path.unwrap_or_else(|| "stryke".to_string());

            // Now `left` is either `host` or `host:N`. The N suffix is digits only, so
            // `user@host` (which contains `@` but no trailing `:digits`) is preserved.
            let (host, n) = if let Some((h, nstr)) = left.rsplit_once(':') {
                if let Ok(n) = nstr.parse::<usize>() {
                    (h.to_string(), n.max(1))
                } else {
                    (left.clone(), 1)
                }
            } else {
                (left.clone(), 1)
            };
            for _ in 0..n {
                slots.push(RemoteSlot {
                    host: host.clone(),
                    pe_path: pe_path.clone(),
                });
            }
        }

        if slots.is_empty() {
            return Err("cluster: need at least one host".into());
        }
        Ok(RemoteCluster {
            slots,
            job_timeout_ms,
            max_attempts,
            connect_timeout_ms,
        })
    }
}

/// `barrier(N)` — `std::sync::Barrier` for phased parallelism (`->wait`).
#[derive(Clone)]
pub struct PerlBarrier(pub Arc<Barrier>);

impl fmt::Debug for PerlBarrier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Barrier")
    }
}

/// Structured stdout/stderr/exit from `capture("cmd")`.
#[derive(Debug, Clone)]
pub struct CaptureResult {
    /// `stdout` field.
    pub stdout: String,
    /// `stderr` field.
    pub stderr: String,
    /// `exitcode` field.
    pub exitcode: i64,
}

/// Columnar table from `dataframe(path)`; chain `filter`, `group_by`, `sum`, `nrow`.
#[derive(Debug, Clone)]
pub struct PerlDataFrame {
    /// `columns` field.
    pub columns: Vec<String>,
    /// `cols` field.
    pub cols: Vec<Vec<StrykeValue>>,
    /// When set, `sum(col)` aggregates rows by this column.
    pub group_by: Option<String>,
}

impl PerlDataFrame {
    /// `nrows` — see implementation.
    #[inline]
    pub fn nrows(&self) -> usize {
        self.cols.first().map(|c| c.len()).unwrap_or(0)
    }
    /// `ncols` — see implementation.
    #[inline]
    pub fn ncols(&self) -> usize {
        self.columns.len()
    }
    /// `col_index` — see implementation.
    #[inline]
    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }
}

/// Heap payload when [`StrykeValue`] is not an immediate or raw [`f64`] bits.
#[derive(Debug, Clone)]
pub(crate) enum HeapObject {
    Integer(i64),
    /// Arbitrary-precision integer — produced by `--compat` arithmetic when an
    /// `i64` op overflows. Native stryke (no `--compat`) never creates this.
    BigInt(Arc<BigInt>),
    Float(f64),
    String(String),
    Bytes(Arc<Vec<u8>>),
    Array(Vec<StrykeValue>),
    Hash(IndexMap<String, StrykeValue>),
    ArrayRef(Arc<RwLock<Vec<StrykeValue>>>),
    HashRef(Arc<RwLock<IndexMap<String, StrykeValue>>>),
    ScalarRef(Arc<RwLock<StrykeValue>>),
    /// Closure-capture cell: same `Arc<RwLock>` sharing as ScalarRef but transparently unwrapped
    /// by [`crate::scope::Scope::get_scalar_slot`] and [`crate::scope::Scope::get_scalar`].
    /// Created by [`crate::scope::Scope::capture`] to share lexical scalars between closures.
    CaptureCell(Arc<RwLock<StrykeValue>>),
    /// `\\$name` when `name` is a plain scalar variable — aliases that binding (Perl ref to lexical).
    ScalarBindingRef(String),
    /// `\\@name` — aliases the live array in [`crate::scope::Scope`] (same stash key as [`Op::GetArray`]).
    ArrayBindingRef(String),
    /// `\\%name` — aliases the live hash in scope.
    HashBindingRef(String),
    CodeRef(Arc<StrykeSub>),
    /// Compiled regex: pattern source and flag chars (e.g. `"i"`, `"g"`) for re-match without re-parse.
    Regex(Arc<PerlCompiledRegex>, String, String),
    Blessed(Arc<BlessedRef>),
    IOHandle(String),
    Atomic(Arc<Mutex<StrykeValue>>),
    Set(Arc<PerlSet>),
    ChannelTx(Arc<Sender<StrykeValue>>),
    ChannelRx(Arc<Receiver<StrykeValue>>),
    AsyncTask(Arc<StrykeAsyncTask>),
    Generator(Arc<PerlGenerator>),
    Deque(Arc<Mutex<VecDeque<StrykeValue>>>),
    Heap(Arc<Mutex<PerlHeap>>),
    /// Exclusive mutex — see [`MutexHandle`]. Created by the `mutex()` builtin
    /// and used by `mutex_lock` / `mutex_unlock` / `mutex_try_lock` /
    /// `mutex_is_locked`. Reference-shared via [`Arc`] across threads.
    Mutex(Arc<MutexHandle>),
    /// Counting semaphore — see [`SemaphoreHandle`]. Created by
    /// `semaphore(N)` / `sem(N)`; manipulated by `semaphore_acquire` /
    /// `semaphore_release` / `semaphore_try_acquire` / `semaphore_permits` /
    /// `semaphore_limit`. Reference-shared via [`Arc`] across threads.
    Semaphore(Arc<SemaphoreHandle>),
    /// Probabilistic-data-structure family — see `sketches.rs`.
    /// Bloom filter: capacity/FPR-parameterized set-membership sketch.
    BloomFilter(Arc<Mutex<crate::sketches::BloomFilter>>),
    /// HyperLogLog: cardinality estimation (distinct-count sketch).
    HllSketch(Arc<Mutex<crate::sketches::HllSketch>>),
    /// Count-Min Sketch: per-key frequency estimation.
    CmsSketch(Arc<Mutex<crate::sketches::CmsSketch>>),
    /// SpaceSaving top-K heavy-hitters sketch.
    TopKSketch(Arc<Mutex<crate::sketches::TopKSketch>>),
    /// t-digest streaming quantile sketch.
    TDigestSketch(Arc<Mutex<crate::sketches::TDigestSketch>>),
    /// Roaring bitmap — compressed bitset over u32.
    RoaringBitmap(Arc<Mutex<crate::sketches::RoaringBitmapSketch>>),
    /// Token-bucket / leaky-bucket rate limiter.
    RateLimiter(Arc<Mutex<crate::sketches::RateLimiterSketch>>),
    /// Consistent-hash ring (Karger '97 style with virtual nodes).
    HashRing(Arc<Mutex<crate::sketches::HashRingSketch>>),
    /// SimHash 64-bit document sketch.
    SimHash(Arc<Mutex<crate::sketches::SimHashSketch>>),
    /// MinHash k-dim signature for Jaccard similarity.
    MinHash(Arc<Mutex<crate::sketches::MinHashSketch>>),
    /// Interval tree — store + query overlap intervals.
    IntervalTree(Arc<Mutex<crate::sketches::IntervalTreeSketch>>),
    /// BK-tree — string-distance index for fuzzy / typo search.
    BkTree(Arc<Mutex<crate::sketches::BkTreeSketch>>),
    /// Rope — fast insert/delete in long strings.
    Rope(Arc<Mutex<crate::sketches::RopeSketch>>),
    /// rkyv-backed KV store handle — see `kvstore.rs`.
    KvStore(Arc<Mutex<crate::kvstore::KvStore>>),
    Pipeline(Arc<Mutex<PipelineInner>>),
    Capture(Arc<CaptureResult>),
    Ppool(PerlPpool),
    RemoteCluster(Arc<RemoteCluster>),
    Barrier(PerlBarrier),
    SqliteConn(Arc<Mutex<rusqlite::Connection>>),
    StructInst(Arc<StructInstance>),
    DataFrame(Arc<Mutex<PerlDataFrame>>),
    EnumInst(Arc<EnumInstance>),
    ClassInst(Arc<ClassInstance>),
    /// Lazy pull-based iterator (`frs`, `drs`, `rev` wrapping, etc.).
    Iterator(Arc<dyn StrykeIterator>),
    /// Numeric/string dualvar: **`$!`** (errno + message) and **`$@`** (numeric flag or code + message).
    ErrnoDual {
        code: i32,
        msg: String,
    },
}

/// NaN-boxed value: one `u64` (immediates, raw float bits, or tagged heap pointer).
#[repr(transparent)]
pub struct StrykeValue(pub(crate) u64);

impl Default for StrykeValue {
    fn default() -> Self {
        Self::UNDEF
    }
}

impl Clone for StrykeValue {
    fn clone(&self) -> Self {
        if nanbox::is_heap(self.0) {
            let arc = self.heap_arc();
            match &*arc {
                HeapObject::Array(v) => {
                    StrykeValue::from_heap(Arc::new(HeapObject::Array(v.clone())))
                }
                HeapObject::Hash(h) => {
                    StrykeValue::from_heap(Arc::new(HeapObject::Hash(h.clone())))
                }
                HeapObject::String(s) => {
                    StrykeValue::from_heap(Arc::new(HeapObject::String(s.clone())))
                }
                HeapObject::Integer(n) => StrykeValue::integer(*n),
                HeapObject::Float(f) => StrykeValue::float(*f),
                _ => StrykeValue::from_heap(Arc::clone(&arc)),
            }
        } else {
            StrykeValue(self.0)
        }
    }
}

impl StrykeValue {
    /// Stack duplicate (`Op::Dup`): share the outer heap [`Arc`] for arrays/hashes (COW on write),
    /// matching Perl temporaries; other heap payloads keep [`Clone`] semantics.
    #[inline]
    pub fn dup_stack(&self) -> Self {
        if nanbox::is_heap(self.0) {
            let arc = self.heap_arc();
            match &*arc {
                HeapObject::Array(_) | HeapObject::Hash(_) => {
                    StrykeValue::from_heap(Arc::clone(&arc))
                }
                _ => self.clone(),
            }
        } else {
            StrykeValue(self.0)
        }
    }

    /// Refcount-only clone: `Arc::clone` the heap pointer (no deep copy of the payload).
    ///
    /// Use this when producing a *second handle* to the same value that the caller
    /// will read-only or consume via [`Self::into_string`] / [`Arc::try_unwrap`]-style
    /// uniqueness checks. Cheap O(1) regardless of the payload size.
    ///
    /// The default [`Clone`] impl deep-copies `String`/`Array`/`Hash` payloads to
    /// preserve "clone = independent writable value" semantics for legacy callers;
    /// in hot RMW paths (`.=`, slot stash-and-return) that deep copy is O(N) and
    /// must be avoided — use this instead.
    #[inline]
    pub fn shallow_clone(&self) -> Self {
        if nanbox::is_heap(self.0) {
            StrykeValue::from_heap(self.heap_arc())
        } else {
            StrykeValue(self.0)
        }
    }
}

impl Drop for StrykeValue {
    fn drop(&mut self) {
        if nanbox::is_heap(self.0) {
            unsafe {
                let p = nanbox::decode_heap_ptr::<HeapObject>(self.0) as *mut HeapObject;
                drop(Arc::from_raw(p));
            }
        }
    }
}

impl fmt::Debug for StrykeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

/// Handle returned by `ppool(N)`; use `->submit(CODE, $topic?)` and `->collect()`.
/// One-arg `submit` copies the caller's `$_` into the worker (so postfix `for` works).
#[derive(Clone)]
pub struct PerlPpool(pub(crate) Arc<crate::ppool::PpoolInner>);

impl fmt::Debug for PerlPpool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PerlPpool")
    }
}

/// See [`crate::fib_like_tail::detect_fib_like_recursive_add`] — iterative fast path for
/// `return f($p-a)+f($p-b)` with a simple integer base case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FibLikeRecAddPattern {
    /// Scalar from `my $p = shift` (e.g. `n`).
    pub param: String,
    /// `n <= base_k` ⇒ return `n`.
    pub base_k: i64,
    /// Left call uses `$param - left_k`.
    pub left_k: i64,
    /// Right call uses `$param - right_k`.
    pub right_k: i64,
}
/// `StrykeSub` — see fields for layout.
#[derive(Debug, Clone)]
pub struct StrykeSub {
    /// `name` field.
    pub name: String,
    /// `params` field.
    pub params: Vec<SubSigParam>,
    /// `body` field.
    pub body: Block,
    /// Captured lexical scope (for closures)
    pub closure_env: Option<Vec<(String, StrykeValue)>>,
    /// Prototype string from `sub name (PROTO) { }`, or `None`.
    pub prototype: Option<String>,
    /// When set, [`Interpreter::call_sub`](crate::vm_helper::VMHelper::call_sub) may evaluate
    /// this sub with an explicit stack instead of recursive scope frames.
    pub fib_like: Option<FibLikeRecAddPattern>,
}

/// Operations queued on a [`StrykeValue::pipeline`](crate::value::StrykeValue::pipeline) value until `collect()`.
#[derive(Debug, Clone)]
pub enum PipelineOp {
    /// `Filter` variant.
    Filter(Arc<StrykeSub>),
    /// `Map` variant.
    Map(Arc<StrykeSub>),
    /// `tap` / `peek` — run block for side effects; `@_` is the current stage list; value unchanged.
    Tap(Arc<StrykeSub>),
    /// `Take` variant.
    Take(i64),
    /// Parallel map (`pmap`) — optional stderr progress bar (same as `pmap ..., progress => 1`).
    PMap { sub: Arc<StrykeSub>, progress: bool },
    /// Parallel grep (`pgrep`).
    PGrep { sub: Arc<StrykeSub>, progress: bool },
    /// Parallel foreach (`pfor`) — side effects only; stream order preserved.
    PFor { sub: Arc<StrykeSub>, progress: bool },
    /// `pmap_chunked N { }` — chunk size + block.
    PMapChunked {
        chunk: i64,
        sub: Arc<StrykeSub>,
        progress: bool,
    },
    /// `psort` / `psort { $a <=> $b }` — parallel sort.
    PSort {
        cmp: Option<Arc<StrykeSub>>,
        progress: bool,
    },
    /// `pcache { }` — parallel memoized map.
    PCache { sub: Arc<StrykeSub>, progress: bool },
    /// `preduce { }` — must be last before `collect()`; `collect()` returns a scalar.
    PReduce { sub: Arc<StrykeSub>, progress: bool },
    /// `preduce_init EXPR, { }` — scalar result; must be last before `collect()`.
    PReduceInit {
        init: StrykeValue,
        sub: Arc<StrykeSub>,
        progress: bool,
    },
    /// `pmap_reduce { } { }` — scalar result; must be last before `collect()`.
    PMapReduce {
        map: Arc<StrykeSub>,
        reduce: Arc<StrykeSub>,
        progress: bool,
    },
}
/// `PipelineInner` — see fields for layout.
#[derive(Debug)]
pub struct PipelineInner {
    /// `source` field.
    pub source: Vec<StrykeValue>,
    /// `ops` field.
    pub ops: Vec<PipelineOp>,
    /// Set after `preduce` / `preduce_init` / `pmap_reduce` — no further `->` ops allowed.
    pub has_scalar_terminal: bool,
    /// When true (from `par_pipeline(LIST)`), `->filter` / `->map` run in parallel with **input order preserved** on `collect()`.
    pub par_stream: bool,
    /// When true (from `par_pipeline_stream(LIST)`), `collect()` wires ops through bounded
    /// channels so items stream between stages concurrently (order **not** preserved).
    pub streaming: bool,
    /// Per-stage worker count for streaming mode (default: available parallelism).
    pub streaming_workers: usize,
    /// Bounded channel capacity for streaming mode (default: 256).
    pub streaming_buffer: usize,
}
/// `BlessedRef` — see fields for layout.
#[derive(Debug)]
pub struct BlessedRef {
    /// `class` field.
    pub class: String,
    /// `data` field.
    pub data: RwLock<StrykeValue>,
    /// When true, dropping does not enqueue `DESTROY` (temporary invocant built while running a destructor).
    pub(crate) suppress_destroy_queue: AtomicBool,
}

impl BlessedRef {
    pub(crate) fn new_blessed(class: String, data: StrykeValue) -> Self {
        Self {
            class,
            data: RwLock::new(data),
            suppress_destroy_queue: AtomicBool::new(false),
        }
    }

    /// Invocant for a running `DESTROY` — must not re-queue when dropped after the call.
    pub(crate) fn new_for_destroy_invocant(class: String, data: StrykeValue) -> Self {
        Self {
            class,
            data: RwLock::new(data),
            suppress_destroy_queue: AtomicBool::new(true),
        }
    }
}

impl Clone for BlessedRef {
    fn clone(&self) -> Self {
        Self {
            class: self.class.clone(),
            data: RwLock::new(self.data.read().clone()),
            suppress_destroy_queue: AtomicBool::new(false),
        }
    }
}

impl Drop for BlessedRef {
    fn drop(&mut self) {
        if self.suppress_destroy_queue.load(AtomicOrdering::Acquire) {
            return;
        }
        let inner = {
            let mut g = self.data.write();
            std::mem::take(&mut *g)
        };
        crate::pending_destroy::enqueue(self.class.clone(), inner);
    }
}

/// Instance of a `struct Name { ... }` definition; field access via `$obj->name`.
#[derive(Debug)]
pub struct StructInstance {
    /// `def` field.
    pub def: Arc<StructDef>,
    /// `values` field.
    pub values: RwLock<Vec<StrykeValue>>,
}

impl StructInstance {
    /// Create a new struct instance with the given definition and values.
    pub fn new(def: Arc<StructDef>, values: Vec<StrykeValue>) -> Self {
        Self {
            def,
            values: RwLock::new(values),
        }
    }

    /// Get a field value by index (clones the value).
    #[inline]
    pub fn get_field(&self, idx: usize) -> Option<StrykeValue> {
        self.values.read().get(idx).cloned()
    }

    /// Set a field value by index.
    #[inline]
    pub fn set_field(&self, idx: usize, val: StrykeValue) {
        if let Some(slot) = self.values.write().get_mut(idx) {
            *slot = val;
        }
    }

    /// Get all field values (clones the vector).
    #[inline]
    pub fn get_values(&self) -> Vec<StrykeValue> {
        self.values.read().clone()
    }
}

impl Clone for StructInstance {
    fn clone(&self) -> Self {
        Self {
            def: Arc::clone(&self.def),
            values: RwLock::new(self.values.read().clone()),
        }
    }
}

/// Instance of an `enum Name { Variant ... }` definition.
#[derive(Debug)]
pub struct EnumInstance {
    /// `def` field.
    pub def: Arc<EnumDef>,
    /// `variant_idx` field.
    pub variant_idx: usize,
    /// Data carried by this variant. For variants with no data, this is UNDEF.
    pub data: StrykeValue,
}

impl EnumInstance {
    /// `new` — see implementation.
    pub fn new(def: Arc<EnumDef>, variant_idx: usize, data: StrykeValue) -> Self {
        Self {
            def,
            variant_idx,
            data,
        }
    }
    /// `variant_name` — see implementation.
    pub fn variant_name(&self) -> &str {
        &self.def.variants[self.variant_idx].name
    }
}

impl Clone for EnumInstance {
    fn clone(&self) -> Self {
        Self {
            def: Arc::clone(&self.def),
            variant_idx: self.variant_idx,
            data: self.data.clone(),
        }
    }
}

/// Instance of a `class Name extends ... impl ... { ... }` definition.
#[derive(Debug)]
pub struct ClassInstance {
    /// `def` field.
    pub def: Arc<ClassDef>,
    /// `values` field.
    pub values: RwLock<Vec<StrykeValue>>,
    /// Full ISA chain for this class (all ancestors, computed at instantiation).
    pub isa_chain: Vec<String>,
}

impl ClassInstance {
    /// `new` — see implementation.
    pub fn new(def: Arc<ClassDef>, values: Vec<StrykeValue>) -> Self {
        Self {
            def,
            values: RwLock::new(values),
            isa_chain: Vec::new(),
        }
    }
    /// `new_with_isa` — see implementation.
    pub fn new_with_isa(
        def: Arc<ClassDef>,
        values: Vec<StrykeValue>,
        isa_chain: Vec<String>,
    ) -> Self {
        Self {
            def,
            values: RwLock::new(values),
            isa_chain,
        }
    }

    /// Check if this instance is-a given class name (direct or inherited).
    #[inline]
    pub fn isa(&self, name: &str) -> bool {
        self.def.name == name || self.isa_chain.contains(&name.to_string())
    }
    /// `get_field` — see implementation.
    #[inline]
    pub fn get_field(&self, idx: usize) -> Option<StrykeValue> {
        self.values.read().get(idx).cloned()
    }
    /// `set_field` — see implementation.
    #[inline]
    pub fn set_field(&self, idx: usize, val: StrykeValue) {
        if let Some(slot) = self.values.write().get_mut(idx) {
            *slot = val;
        }
    }
    /// `get_values` — see implementation.
    #[inline]
    pub fn get_values(&self) -> Vec<StrykeValue> {
        self.values.read().clone()
    }

    /// Get field value by name (searches through class and parent hierarchies).
    pub fn get_field_by_name(&self, name: &str) -> Option<StrykeValue> {
        self.def
            .field_index(name)
            .and_then(|idx| self.get_field(idx))
    }

    /// Set field value by name.
    pub fn set_field_by_name(&self, name: &str, val: StrykeValue) -> bool {
        if let Some(idx) = self.def.field_index(name) {
            self.set_field(idx, val);
            true
        } else {
            false
        }
    }
}

impl Clone for ClassInstance {
    fn clone(&self) -> Self {
        Self {
            def: Arc::clone(&self.def),
            values: RwLock::new(self.values.read().clone()),
            isa_chain: self.isa_chain.clone(),
        }
    }
}

impl StrykeValue {
    /// `UNDEF` constant.
    pub const UNDEF: StrykeValue = StrykeValue(nanbox::encode_imm_undef());

    #[inline]
    fn from_heap(arc: Arc<HeapObject>) -> StrykeValue {
        let ptr = Arc::into_raw(arc);
        StrykeValue(nanbox::encode_heap_ptr(ptr))
    }

    #[inline]
    pub(crate) fn heap_arc(&self) -> Arc<HeapObject> {
        debug_assert!(nanbox::is_heap(self.0));
        unsafe {
            let p = nanbox::decode_heap_ptr::<HeapObject>(self.0);
            Arc::increment_strong_count(p);
            Arc::from_raw(p as *mut HeapObject)
        }
    }

    /// Borrow the `Arc`-allocated [`HeapObject`] without refcount traffic (`Arc::clone` / `drop`).
    ///
    /// # Safety
    /// `nanbox::is_heap(self.0)` must hold (same invariant as [`Self::heap_arc`]).
    #[inline]
    pub(crate) unsafe fn heap_ref(&self) -> &HeapObject {
        &*nanbox::decode_heap_ptr::<HeapObject>(self.0)
    }

    #[inline]
    pub(crate) fn with_heap<R>(&self, f: impl FnOnce(&HeapObject) -> R) -> Option<R> {
        if !nanbox::is_heap(self.0) {
            return None;
        }
        // SAFETY: `is_heap` matches the contract of [`Self::heap_ref`].
        Some(f(unsafe { self.heap_ref() }))
    }

    /// Raw NaN-box bits for internal identity (e.g. [`crate::jit`] cache keys).
    #[inline]
    pub(crate) fn raw_bits(&self) -> u64 {
        self.0
    }

    /// Reconstruct from [`Self::raw_bits`] (e.g. block JIT returning a full [`StrykeValue`] encoding in `i64`).
    #[inline]
    pub(crate) fn from_raw_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// `typed : Int` — inline `i32` or heap `i64`.
    #[inline]
    pub fn is_integer_like(&self) -> bool {
        nanbox::as_imm_int32(self.0).is_some()
            || matches!(
                self.with_heap(|h| matches!(h, HeapObject::Integer(_) | HeapObject::BigInt(_))),
                Some(true)
            )
    }

    /// Raw `f64` bits or heap boxed float (NaN/Inf).
    #[inline]
    pub fn is_float_like(&self) -> bool {
        nanbox::is_raw_float_bits(self.0)
            || matches!(
                self.with_heap(|h| matches!(h, HeapObject::Float(_))),
                Some(true)
            )
    }

    /// Heap UTF-8 string only.
    #[inline]
    pub fn is_string_like(&self) -> bool {
        matches!(
            self.with_heap(|h| matches!(h, HeapObject::String(_))),
            Some(true)
        )
    }
    /// `integer` — see implementation.
    #[inline]
    pub fn integer(n: i64) -> Self {
        if n >= i32::MIN as i64 && n <= i32::MAX as i64 {
            StrykeValue(nanbox::encode_imm_int32(n as i32))
        } else {
            Self::from_heap(Arc::new(HeapObject::Integer(n)))
        }
    }

    /// Wrap a `BigInt`. If it fits in `i64`, demotes to a regular integer so
    /// downstream consumers don't have to special-case BigInt for small values.
    pub fn bigint(n: BigInt) -> Self {
        use num_traits::ToPrimitive;
        if let Some(i) = n.to_i64() {
            return Self::integer(i);
        }
        Self::from_heap(Arc::new(HeapObject::BigInt(Arc::new(n))))
    }

    /// Returns the inner `BigInt` as `Arc` (zero-copy) when this value is a
    /// boxed `BigInt`; `None` otherwise. Use [`Self::to_bigint`] to coerce
    /// from `i64`/`f64`/strings.
    pub fn as_bigint(&self) -> Option<Arc<BigInt>> {
        self.with_heap(|h| match h {
            HeapObject::BigInt(b) => Some(Arc::clone(b)),
            _ => None,
        })
        .flatten()
    }

    /// Coerce any numeric value into a `BigInt`. Floats truncate. Used by
    /// arithmetic promotion paths under `--compat` when one side overflowed.
    pub fn to_bigint(&self) -> BigInt {
        if let Some(b) = self.as_bigint() {
            return (*b).clone();
        }
        if let Some(i) = self.as_integer() {
            return BigInt::from(i);
        }
        BigInt::from(self.to_number() as i64)
    }
    /// `float` — see implementation.
    #[inline]
    pub fn float(f: f64) -> Self {
        if nanbox::float_needs_box(f) {
            Self::from_heap(Arc::new(HeapObject::Float(f)))
        } else {
            StrykeValue(f.to_bits())
        }
    }
    /// `string` — see implementation.
    #[inline]
    pub fn string(s: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::String(s)))
    }
    /// `bytes` — see implementation.
    #[inline]
    pub fn bytes(b: Arc<Vec<u8>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Bytes(b)))
    }
    /// `array` — see implementation.
    #[inline]
    pub fn array(v: Vec<StrykeValue>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Array(v)))
    }

    /// Wrap a lazy iterator as a StrykeValue.
    #[inline]
    pub fn iterator(it: Arc<dyn StrykeIterator>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Iterator(it)))
    }

    /// True when this value is a lazy iterator.
    #[inline]
    pub fn is_iterator(&self) -> bool {
        if !nanbox::is_heap(self.0) {
            return false;
        }
        matches!(unsafe { self.heap_ref() }, HeapObject::Iterator(_))
    }

    /// Extract the iterator Arc (panics if not an iterator).
    pub fn into_iterator(&self) -> Arc<dyn StrykeIterator> {
        if nanbox::is_heap(self.0) {
            if let HeapObject::Iterator(it) = &*self.heap_arc() {
                return Arc::clone(it);
            }
        }
        panic!("into_iterator on non-iterator value");
    }
    /// `hash` — see implementation.
    #[inline]
    pub fn hash(h: IndexMap<String, StrykeValue>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Hash(h)))
    }
    /// `array_ref` — see implementation.
    #[inline]
    pub fn array_ref(a: Arc<RwLock<Vec<StrykeValue>>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ArrayRef(a)))
    }
    /// `hash_ref` — see implementation.
    #[inline]
    pub fn hash_ref(h: Arc<RwLock<IndexMap<String, StrykeValue>>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::HashRef(h)))
    }
    /// `scalar_ref` — see implementation.
    #[inline]
    pub fn scalar_ref(r: Arc<RwLock<StrykeValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ScalarRef(r)))
    }
    /// `capture_cell` — see implementation.
    #[inline]
    pub fn capture_cell(r: Arc<RwLock<StrykeValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::CaptureCell(r)))
    }
    /// `scalar_binding_ref` — see implementation.
    #[inline]
    pub fn scalar_binding_ref(name: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::ScalarBindingRef(name)))
    }
    /// `array_binding_ref` — see implementation.
    #[inline]
    pub fn array_binding_ref(name: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::ArrayBindingRef(name)))
    }
    /// `hash_binding_ref` — see implementation.
    #[inline]
    pub fn hash_binding_ref(name: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::HashBindingRef(name)))
    }
    /// `code_ref` — see implementation.
    #[inline]
    pub fn code_ref(c: Arc<StrykeSub>) -> Self {
        Self::from_heap(Arc::new(HeapObject::CodeRef(c)))
    }
    /// `as_code_ref` — see implementation.
    #[inline]
    pub fn as_code_ref(&self) -> Option<Arc<StrykeSub>> {
        self.with_heap(|h| match h {
            HeapObject::CodeRef(sub) => Some(Arc::clone(sub)),
            _ => None,
        })
        .flatten()
    }
    /// `as_regex` — see implementation.
    #[inline]
    pub fn as_regex(&self) -> Option<Arc<PerlCompiledRegex>> {
        self.with_heap(|h| match h {
            HeapObject::Regex(re, _, _) => Some(Arc::clone(re)),
            _ => None,
        })
        .flatten()
    }
    /// `as_blessed_ref` — see implementation.
    #[inline]
    pub fn as_blessed_ref(&self) -> Option<Arc<BlessedRef>> {
        self.with_heap(|h| match h {
            HeapObject::Blessed(b) => Some(Arc::clone(b)),
            _ => None,
        })
        .flatten()
    }

    /// Hash lookup when this value is a plain `HeapObject::Hash` (not a ref).
    #[inline]
    pub fn hash_get(&self, key: &str) -> Option<StrykeValue> {
        self.with_heap(|h| match h {
            HeapObject::Hash(h) => h.get(key).cloned(),
            _ => None,
        })
        .flatten()
    }
    /// `is_undef` — see implementation.
    #[inline]
    pub fn is_undef(&self) -> bool {
        nanbox::is_imm_undef(self.0)
    }

    /// True for simple scalar values (integer, float, string, undef, bytes) that should be
    /// wrapped in ScalarRef for closure variable sharing. Complex heap objects like
    /// refs, blessed objects, code refs, etc. should NOT be wrapped because they already
    /// share state via Arc and wrapping breaks type detection.
    pub fn is_simple_scalar(&self) -> bool {
        if self.is_undef() {
            return true;
        }
        if !nanbox::is_heap(self.0) {
            return true; // immediate int32
        }
        matches!(
            unsafe { self.heap_ref() },
            HeapObject::Integer(_)
                | HeapObject::BigInt(_)
                | HeapObject::Float(_)
                | HeapObject::String(_)
                | HeapObject::Bytes(_)
        )
    }

    /// Immediate `int32` or heap `Integer` (not float / string).
    #[inline]
    pub fn as_integer(&self) -> Option<i64> {
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            return Some(n as i64);
        }
        if nanbox::is_raw_float_bits(self.0) {
            return None;
        }
        self.with_heap(|h| match h {
            HeapObject::Integer(n) => Some(*n),
            HeapObject::BigInt(b) => {
                use num_traits::ToPrimitive;
                b.to_i64()
            }
            _ => None,
        })
        .flatten()
    }
    /// `as_float` — see implementation.
    #[inline]
    pub fn as_float(&self) -> Option<f64> {
        if nanbox::is_raw_float_bits(self.0) {
            return Some(f64::from_bits(self.0));
        }
        self.with_heap(|h| match h {
            HeapObject::Float(f) => Some(*f),
            _ => None,
        })
        .flatten()
    }
    /// `as_array_vec` — see implementation.
    #[inline]
    pub fn as_array_vec(&self) -> Option<Vec<StrykeValue>> {
        self.with_heap(|h| match h {
            HeapObject::Array(v) => Some(v.clone()),
            _ => None,
        })
        .flatten()
    }

    /// Expand a `map` / `flat_map` / `pflat_map` block result into list elements. Plain arrays
    /// expand; when `peel_array_ref`, a single ARRAY ref is dereferenced one level (stryke
    /// `flat_map` / `pflat_map`; stock `map` uses `peel_array_ref == false`).
    pub fn map_flatten_outputs(&self, peel_array_ref: bool) -> Vec<StrykeValue> {
        if let Some(a) = self.as_array_vec() {
            return a;
        }
        if peel_array_ref {
            if let Some(r) = self.as_array_ref() {
                return r.read().clone();
            }
        }
        if self.is_iterator() {
            return self.into_iterator().collect_all();
        }
        vec![self.clone()]
    }
    /// `as_hash_map` — see implementation.
    #[inline]
    pub fn as_hash_map(&self) -> Option<IndexMap<String, StrykeValue>> {
        self.with_heap(|h| match h {
            HeapObject::Hash(h) => Some(h.clone()),
            _ => None,
        })
        .flatten()
    }
    /// `as_bytes_arc` — see implementation.
    #[inline]
    pub fn as_bytes_arc(&self) -> Option<Arc<Vec<u8>>> {
        self.with_heap(|h| match h {
            HeapObject::Bytes(b) => Some(Arc::clone(b)),
            _ => None,
        })
        .flatten()
    }
    /// `length` builtin semantics, factored out so the interpreter
    /// (`BuiltinId::Length`) and the fusevm JIT host helper
    /// (`fusevm_bridge::stryke_str_len_op`) compute an identical result: array
    /// element count, hash key count, raw-byte length, otherwise the stringified
    /// value's character count (when the `utf8` pragma is active) or byte length.
    pub fn length_value(&self, utf8: bool) -> i64 {
        if let Some(a) = self.as_array_vec() {
            a.len() as i64
        } else if let Some(h) = self.as_hash_map() {
            h.len() as i64
        } else if let Some(b) = self.as_bytes_arc() {
            b.len() as i64
        } else {
            let s = self.to_string();
            if utf8 {
                s.chars().count() as i64
            } else {
                s.len() as i64
            }
        }
    }

    /// `ord` builtin: Unicode codepoint of the stringified value's first char (0 if
    /// empty). Shared by the interpreter (`BuiltinId::Ord`) and the fusevm JIT host
    /// helper so both agree exactly.
    pub fn ord_value(&self) -> i64 {
        self.to_string().chars().next().map(|c| c as i64).unwrap_or(0)
    }

    /// `hex` builtin: parse the stringified value as hexadecimal (optional `0x`/`0X`
    /// prefix), 0 on failure. Shared by the interpreter and the fusevm JIT helper.
    pub fn hex_value(&self) -> i64 {
        let s = self.to_string();
        let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
        i64::from_str_radix(clean, 16).unwrap_or(0)
    }

    /// `oct` builtin: parse the stringified value per Perl `oct` (`0x`/`0X` hex,
    /// `0b`/`0B` binary, `0o`/`0O` or bare-leading-zero octal), 0 on failure. Shared
    /// by the interpreter and the fusevm JIT helper.
    pub fn oct_value(&self) -> i64 {
        let s = self.to_string();
        let s = s.trim();
        if s.starts_with("0x") || s.starts_with("0X") {
            i64::from_str_radix(&s[2..], 16).unwrap_or(0)
        } else if s.starts_with("0b") || s.starts_with("0B") {
            i64::from_str_radix(&s[2..], 2).unwrap_or(0)
        } else if s.starts_with("0o") || s.starts_with("0O") {
            i64::from_str_radix(&s[2..], 8).unwrap_or(0)
        } else {
            i64::from_str_radix(s.trim_start_matches('0'), 8).unwrap_or(0)
        }
    }
    /// `as_async_task` — see implementation.
    #[inline]
    pub fn as_async_task(&self) -> Option<Arc<StrykeAsyncTask>> {
        self.with_heap(|h| match h {
            HeapObject::AsyncTask(t) => Some(Arc::clone(t)),
            _ => None,
        })
        .flatten()
    }
    /// `as_generator` — see implementation.
    #[inline]
    pub fn as_generator(&self) -> Option<Arc<PerlGenerator>> {
        self.with_heap(|h| match h {
            HeapObject::Generator(g) => Some(Arc::clone(g)),
            _ => None,
        })
        .flatten()
    }
    /// `as_atomic_arc` — see implementation.
    #[inline]
    pub fn as_atomic_arc(&self) -> Option<Arc<Mutex<StrykeValue>>> {
        self.with_heap(|h| match h {
            HeapObject::Atomic(a) => Some(Arc::clone(a)),
            _ => None,
        })
        .flatten()
    }
    /// `as_io_handle_name` — see implementation.
    #[inline]
    pub fn as_io_handle_name(&self) -> Option<String> {
        self.with_heap(|h| match h {
            HeapObject::IOHandle(n) => Some(n.clone()),
            _ => None,
        })
        .flatten()
    }
    /// `as_sqlite_conn` — see implementation.
    #[inline]
    pub fn as_sqlite_conn(&self) -> Option<Arc<Mutex<rusqlite::Connection>>> {
        self.with_heap(|h| match h {
            HeapObject::SqliteConn(c) => Some(Arc::clone(c)),
            _ => None,
        })
        .flatten()
    }
    /// `as_struct_inst` — see implementation.
    #[inline]
    pub fn as_struct_inst(&self) -> Option<Arc<StructInstance>> {
        self.with_heap(|h| match h {
            HeapObject::StructInst(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `as_enum_inst` — see implementation.
    #[inline]
    pub fn as_enum_inst(&self) -> Option<Arc<EnumInstance>> {
        self.with_heap(|h| match h {
            HeapObject::EnumInst(e) => Some(Arc::clone(e)),
            _ => None,
        })
        .flatten()
    }
    /// `as_class_inst` — see implementation.
    #[inline]
    pub fn as_class_inst(&self) -> Option<Arc<ClassInstance>> {
        self.with_heap(|h| match h {
            HeapObject::ClassInst(c) => Some(Arc::clone(c)),
            _ => None,
        })
        .flatten()
    }
    /// `as_dataframe` — see implementation.
    #[inline]
    pub fn as_dataframe(&self) -> Option<Arc<Mutex<PerlDataFrame>>> {
        self.with_heap(|h| match h {
            HeapObject::DataFrame(d) => Some(Arc::clone(d)),
            _ => None,
        })
        .flatten()
    }
    /// `as_deque` — see implementation.
    #[inline]
    pub fn as_deque(&self) -> Option<Arc<Mutex<VecDeque<StrykeValue>>>> {
        self.with_heap(|h| match h {
            HeapObject::Deque(d) => Some(Arc::clone(d)),
            _ => None,
        })
        .flatten()
    }
    /// `as_heap_pq` — see implementation.
    #[inline]
    pub fn as_heap_pq(&self) -> Option<Arc<Mutex<PerlHeap>>> {
        self.with_heap(|h| match h {
            HeapObject::Heap(h) => Some(Arc::clone(h)),
            _ => None,
        })
        .flatten()
    }
    /// `as_pipeline` — see implementation.
    #[inline]
    pub fn as_pipeline(&self) -> Option<Arc<Mutex<PipelineInner>>> {
        self.with_heap(|h| match h {
            HeapObject::Pipeline(p) => Some(Arc::clone(p)),
            _ => None,
        })
        .flatten()
    }
    /// `as_capture` — see implementation.
    #[inline]
    pub fn as_capture(&self) -> Option<Arc<CaptureResult>> {
        self.with_heap(|h| match h {
            HeapObject::Capture(c) => Some(Arc::clone(c)),
            _ => None,
        })
        .flatten()
    }
    /// `as_ppool` — see implementation.
    #[inline]
    pub fn as_ppool(&self) -> Option<PerlPpool> {
        self.with_heap(|h| match h {
            HeapObject::Ppool(p) => Some(p.clone()),
            _ => None,
        })
        .flatten()
    }
    /// `as_remote_cluster` — see implementation.
    #[inline]
    pub fn as_remote_cluster(&self) -> Option<Arc<RemoteCluster>> {
        self.with_heap(|h| match h {
            HeapObject::RemoteCluster(c) => Some(Arc::clone(c)),
            _ => None,
        })
        .flatten()
    }
    /// `as_barrier` — see implementation.
    #[inline]
    pub fn as_barrier(&self) -> Option<PerlBarrier> {
        self.with_heap(|h| match h {
            HeapObject::Barrier(b) => Some(b.clone()),
            _ => None,
        })
        .flatten()
    }
    /// `as_channel_tx` — see implementation.
    #[inline]
    pub fn as_channel_tx(&self) -> Option<Arc<Sender<StrykeValue>>> {
        self.with_heap(|h| match h {
            HeapObject::ChannelTx(t) => Some(Arc::clone(t)),
            _ => None,
        })
        .flatten()
    }
    /// `as_channel_rx` — see implementation.
    #[inline]
    pub fn as_channel_rx(&self) -> Option<Arc<Receiver<StrykeValue>>> {
        self.with_heap(|h| match h {
            HeapObject::ChannelRx(r) => Some(Arc::clone(r)),
            _ => None,
        })
        .flatten()
    }
    /// `as_scalar_ref` — see implementation.
    #[inline]
    pub fn as_scalar_ref(&self) -> Option<Arc<RwLock<StrykeValue>>> {
        self.with_heap(|h| match h {
            HeapObject::ScalarRef(r) => Some(Arc::clone(r)),
            _ => None,
        })
        .flatten()
    }

    /// Returns the inner Arc if this is a [`HeapObject::CaptureCell`].
    #[inline]
    pub fn as_capture_cell(&self) -> Option<Arc<RwLock<StrykeValue>>> {
        self.with_heap(|h| match h {
            HeapObject::CaptureCell(r) => Some(Arc::clone(r)),
            _ => None,
        })
        .flatten()
    }

    /// Name of the scalar slot for [`HeapObject::ScalarBindingRef`], if any.
    #[inline]
    pub fn as_scalar_binding_name(&self) -> Option<String> {
        self.with_heap(|h| match h {
            HeapObject::ScalarBindingRef(s) => Some(s.clone()),
            _ => None,
        })
        .flatten()
    }

    /// Stash-qualified array name for [`HeapObject::ArrayBindingRef`], if any.
    #[inline]
    pub fn as_array_binding_name(&self) -> Option<String> {
        self.with_heap(|h| match h {
            HeapObject::ArrayBindingRef(s) => Some(s.clone()),
            _ => None,
        })
        .flatten()
    }

    /// Hash name for [`HeapObject::HashBindingRef`], if any.
    #[inline]
    pub fn as_hash_binding_name(&self) -> Option<String> {
        self.with_heap(|h| match h {
            HeapObject::HashBindingRef(s) => Some(s.clone()),
            _ => None,
        })
        .flatten()
    }
    /// `as_array_ref` — see implementation.
    #[inline]
    pub fn as_array_ref(&self) -> Option<Arc<RwLock<Vec<StrykeValue>>>> {
        self.with_heap(|h| match h {
            HeapObject::ArrayRef(r) => Some(Arc::clone(r)),
            _ => None,
        })
        .flatten()
    }
    /// `as_hash_ref` — see implementation.
    #[inline]
    pub fn as_hash_ref(&self) -> Option<Arc<RwLock<IndexMap<String, StrykeValue>>>> {
        self.with_heap(|h| match h {
            HeapObject::HashRef(r) => Some(Arc::clone(r)),
            _ => None,
        })
        .flatten()
    }

    /// `mysync`: `deque` / priority `heap` — already `Arc<Mutex<…>>`.
    #[inline]
    pub fn is_mysync_deque_or_heap(&self) -> bool {
        matches!(
            self.with_heap(|h| matches!(h, HeapObject::Deque(_) | HeapObject::Heap(_))),
            Some(true)
        )
    }
    /// `regex` — see implementation.
    #[inline]
    pub fn regex(rx: Arc<PerlCompiledRegex>, pattern_src: String, flags: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::Regex(rx, pattern_src, flags)))
    }

    /// Pattern and flag string stored with a compiled regex (for `=~` / [`Op::RegexMatchDyn`]).
    #[inline]
    pub fn regex_src_and_flags(&self) -> Option<(String, String)> {
        self.with_heap(|h| match h {
            HeapObject::Regex(_, pat, fl) => Some((pat.clone(), fl.clone())),
            _ => None,
        })
        .flatten()
    }
    /// `blessed` — see implementation.
    #[inline]
    pub fn blessed(b: Arc<BlessedRef>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Blessed(b)))
    }
    /// `io_handle` — see implementation.
    #[inline]
    pub fn io_handle(name: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::IOHandle(name)))
    }
    /// `atomic` — see implementation.
    #[inline]
    pub fn atomic(a: Arc<Mutex<StrykeValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Atomic(a)))
    }
    /// `set` — see implementation.
    #[inline]
    pub fn set(s: Arc<PerlSet>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Set(s)))
    }
    /// `channel_tx` — see implementation.
    #[inline]
    pub fn channel_tx(tx: Arc<Sender<StrykeValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ChannelTx(tx)))
    }
    /// `channel_rx` — see implementation.
    #[inline]
    pub fn channel_rx(rx: Arc<Receiver<StrykeValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ChannelRx(rx)))
    }
    /// `async_task` — see implementation.
    #[inline]
    pub fn async_task(t: Arc<StrykeAsyncTask>) -> Self {
        Self::from_heap(Arc::new(HeapObject::AsyncTask(t)))
    }
    /// `generator` — see implementation.
    #[inline]
    pub fn generator(g: Arc<PerlGenerator>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Generator(g)))
    }
    /// `deque` — see implementation.
    #[inline]
    pub fn deque(d: Arc<Mutex<VecDeque<StrykeValue>>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Deque(d)))
    }
    /// `heap` — see implementation.
    #[inline]
    pub fn heap(h: Arc<Mutex<PerlHeap>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Heap(h)))
    }

    /// Construct a fresh, unlocked [`HeapObject::Mutex`].
    #[inline]
    pub fn mutex() -> Self {
        Self::from_heap(Arc::new(HeapObject::Mutex(Arc::new(MutexHandle::new()))))
    }

    /// Construct a [`HeapObject::Semaphore`] with `n` permits (`n` is clamped
    /// to `>= 0` by the caller — see `builtins_sync::semaphore_new`).
    #[inline]
    pub fn semaphore(n: i64) -> Self {
        Self::from_heap(Arc::new(HeapObject::Semaphore(Arc::new(
            SemaphoreHandle::new(n),
        ))))
    }

    /// Borrow-the-inner-handle accessor for [`HeapObject::Mutex`] (returns
    /// the [`Arc`] so the handle outlives the temporary `StrykeValue`).
    #[inline]
    pub fn as_mutex(&self) -> Option<Arc<MutexHandle>> {
        self.with_heap(|h| match h {
            HeapObject::Mutex(m) => Some(Arc::clone(m)),
            _ => None,
        })
        .flatten()
    }

    /// Borrow-the-inner-handle accessor for [`HeapObject::Semaphore`].
    #[inline]
    pub fn as_semaphore(&self) -> Option<Arc<SemaphoreHandle>> {
        self.with_heap(|h| match h {
            HeapObject::Semaphore(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `bloom_filter` — see implementation.
    #[inline]
    pub fn bloom_filter(b: Arc<Mutex<crate::sketches::BloomFilter>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::BloomFilter(b)))
    }
    /// `as_bloom_filter` — see implementation.
    #[inline]
    pub fn as_bloom_filter(&self) -> Option<Arc<Mutex<crate::sketches::BloomFilter>>> {
        self.with_heap(|h| match h {
            HeapObject::BloomFilter(b) => Some(Arc::clone(b)),
            _ => None,
        })
        .flatten()
    }
    /// `hll_sketch` — see implementation.
    #[inline]
    pub fn hll_sketch(h: Arc<Mutex<crate::sketches::HllSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::HllSketch(h)))
    }
    /// `as_hll_sketch` — see implementation.
    #[inline]
    pub fn as_hll_sketch(&self) -> Option<Arc<Mutex<crate::sketches::HllSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::HllSketch(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `cms_sketch` — see implementation.
    #[inline]
    pub fn cms_sketch(c: Arc<Mutex<crate::sketches::CmsSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::CmsSketch(c)))
    }
    /// `as_cms_sketch` — see implementation.
    #[inline]
    pub fn as_cms_sketch(&self) -> Option<Arc<Mutex<crate::sketches::CmsSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::CmsSketch(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `topk_sketch` — see implementation.
    #[inline]
    pub fn topk_sketch(t: Arc<Mutex<crate::sketches::TopKSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::TopKSketch(t)))
    }
    /// `as_topk_sketch` — see implementation.
    #[inline]
    pub fn as_topk_sketch(&self) -> Option<Arc<Mutex<crate::sketches::TopKSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::TopKSketch(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `tdigest_sketch` — see implementation.
    #[inline]
    pub fn tdigest_sketch(t: Arc<Mutex<crate::sketches::TDigestSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::TDigestSketch(t)))
    }
    /// `as_tdigest_sketch` — see implementation.
    #[inline]
    pub fn as_tdigest_sketch(&self) -> Option<Arc<Mutex<crate::sketches::TDigestSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::TDigestSketch(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `roaring_bitmap` — see implementation.
    #[inline]
    pub fn roaring_bitmap(r: Arc<Mutex<crate::sketches::RoaringBitmapSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::RoaringBitmap(r)))
    }
    /// `as_roaring_bitmap` — see implementation.
    #[inline]
    pub fn as_roaring_bitmap(&self) -> Option<Arc<Mutex<crate::sketches::RoaringBitmapSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::RoaringBitmap(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `rate_limiter` — see implementation.
    #[inline]
    pub fn rate_limiter(r: Arc<Mutex<crate::sketches::RateLimiterSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::RateLimiter(r)))
    }
    /// `as_rate_limiter` — see implementation.
    #[inline]
    pub fn as_rate_limiter(&self) -> Option<Arc<Mutex<crate::sketches::RateLimiterSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::RateLimiter(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `hash_ring` — see implementation.
    #[inline]
    pub fn hash_ring(r: Arc<Mutex<crate::sketches::HashRingSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::HashRing(r)))
    }
    /// `as_hash_ring` — see implementation.
    #[inline]
    pub fn as_hash_ring(&self) -> Option<Arc<Mutex<crate::sketches::HashRingSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::HashRing(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `simhash` — see implementation.
    #[inline]
    pub fn simhash(s: Arc<Mutex<crate::sketches::SimHashSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::SimHash(s)))
    }
    /// `as_simhash` — see implementation.
    #[inline]
    pub fn as_simhash(&self) -> Option<Arc<Mutex<crate::sketches::SimHashSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::SimHash(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `minhash` — see implementation.
    #[inline]
    pub fn minhash(m: Arc<Mutex<crate::sketches::MinHashSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::MinHash(m)))
    }
    /// `as_minhash` — see implementation.
    #[inline]
    pub fn as_minhash(&self) -> Option<Arc<Mutex<crate::sketches::MinHashSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::MinHash(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `interval_tree` — see implementation.
    #[inline]
    pub fn interval_tree(t: Arc<Mutex<crate::sketches::IntervalTreeSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::IntervalTree(t)))
    }
    /// `as_interval_tree` — see implementation.
    #[inline]
    pub fn as_interval_tree(&self) -> Option<Arc<Mutex<crate::sketches::IntervalTreeSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::IntervalTree(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `bk_tree` — see implementation.
    #[inline]
    pub fn bk_tree(t: Arc<Mutex<crate::sketches::BkTreeSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::BkTree(t)))
    }
    /// `as_bk_tree` — see implementation.
    #[inline]
    pub fn as_bk_tree(&self) -> Option<Arc<Mutex<crate::sketches::BkTreeSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::BkTree(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `rope` — see implementation.
    #[inline]
    pub fn rope(r: Arc<Mutex<crate::sketches::RopeSketch>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Rope(r)))
    }
    /// `as_rope` — see implementation.
    #[inline]
    pub fn as_rope(&self) -> Option<Arc<Mutex<crate::sketches::RopeSketch>>> {
        self.with_heap(|h| match h {
            HeapObject::Rope(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `kv_store` — see implementation.
    #[inline]
    pub fn kv_store(k: Arc<Mutex<crate::kvstore::KvStore>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::KvStore(k)))
    }
    /// `as_kv_store` — see implementation.
    #[inline]
    pub fn as_kv_store(&self) -> Option<Arc<Mutex<crate::kvstore::KvStore>>> {
        self.with_heap(|h| match h {
            HeapObject::KvStore(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }
    /// `pipeline` — see implementation.
    #[inline]
    pub fn pipeline(p: Arc<Mutex<PipelineInner>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Pipeline(p)))
    }
    /// `capture` — see implementation.
    #[inline]
    pub fn capture(c: Arc<CaptureResult>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Capture(c)))
    }
    /// `ppool` — see implementation.
    #[inline]
    pub fn ppool(p: PerlPpool) -> Self {
        Self::from_heap(Arc::new(HeapObject::Ppool(p)))
    }
    /// `remote_cluster` — see implementation.
    #[inline]
    pub fn remote_cluster(c: Arc<RemoteCluster>) -> Self {
        Self::from_heap(Arc::new(HeapObject::RemoteCluster(c)))
    }
    /// `barrier` — see implementation.
    #[inline]
    pub fn barrier(b: PerlBarrier) -> Self {
        Self::from_heap(Arc::new(HeapObject::Barrier(b)))
    }
    /// `sqlite_conn` — see implementation.
    #[inline]
    pub fn sqlite_conn(c: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::SqliteConn(c)))
    }
    /// `struct_inst` — see implementation.
    #[inline]
    pub fn struct_inst(s: Arc<StructInstance>) -> Self {
        Self::from_heap(Arc::new(HeapObject::StructInst(s)))
    }
    /// `enum_inst` — see implementation.
    #[inline]
    pub fn enum_inst(e: Arc<EnumInstance>) -> Self {
        Self::from_heap(Arc::new(HeapObject::EnumInst(e)))
    }
    /// `class_inst` — see implementation.
    #[inline]
    pub fn class_inst(c: Arc<ClassInstance>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ClassInst(c)))
    }
    /// `dataframe` — see implementation.
    #[inline]
    pub fn dataframe(df: Arc<Mutex<PerlDataFrame>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::DataFrame(df)))
    }

    /// OS errno dualvar (`$!`) or eval-error dualvar (`$@`): `to_int`/`to_number` use `code`; string context uses `msg`.
    #[inline]
    pub fn errno_dual(code: i32, msg: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::ErrnoDual { code, msg }))
    }

    /// If this value is a numeric/string dualvar (`$!` / `$@`), return `(code, msg)`.
    #[inline]
    pub(crate) fn errno_dual_parts(&self) -> Option<(i32, String)> {
        if !nanbox::is_heap(self.0) {
            return None;
        }
        match unsafe { self.heap_ref() } {
            HeapObject::ErrnoDual { code, msg } => Some((*code, msg.clone())),
            _ => None,
        }
    }

    /// Heap string payload, if any (allocates).
    #[inline]
    pub fn as_str(&self) -> Option<String> {
        if !nanbox::is_heap(self.0) {
            return None;
        }
        match unsafe { self.heap_ref() } {
            HeapObject::String(s) => Some(s.clone()),
            _ => None,
        }
    }
    /// `append_to` — see implementation.
    #[inline]
    pub fn append_to(&self, buf: &mut String) {
        if nanbox::is_imm_undef(self.0) {
            return;
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            let mut b = itoa::Buffer::new();
            buf.push_str(b.format(n));
            return;
        }
        if nanbox::is_raw_float_bits(self.0) {
            buf.push_str(&format_float(f64::from_bits(self.0)));
            return;
        }
        match unsafe { self.heap_ref() } {
            HeapObject::String(s) => buf.push_str(s),
            HeapObject::ErrnoDual { msg, .. } => buf.push_str(msg),
            HeapObject::Bytes(b) => buf.push_str(&decode_utf8_or_latin1(b)),
            HeapObject::Atomic(arc) => arc.lock().append_to(buf),
            HeapObject::Set(s) => {
                buf.push('{');
                let mut first = true;
                for v in s.values() {
                    if !first {
                        buf.push(',');
                    }
                    first = false;
                    v.append_to(buf);
                }
                buf.push('}');
            }
            HeapObject::ChannelTx(_) => buf.push_str("PCHANNEL::Tx"),
            HeapObject::ChannelRx(_) => buf.push_str("PCHANNEL::Rx"),
            HeapObject::AsyncTask(_) => buf.push_str("AsyncTask"),
            HeapObject::Generator(_) => buf.push_str("Generator"),
            HeapObject::Pipeline(_) => buf.push_str("Pipeline"),
            HeapObject::DataFrame(d) => {
                let g = d.lock();
                buf.push_str(&format!("DataFrame({}x{})", g.nrows(), g.ncols()));
            }
            HeapObject::Capture(_) => buf.push_str("Capture"),
            HeapObject::Ppool(_) => buf.push_str("Ppool"),
            HeapObject::RemoteCluster(_) => buf.push_str("Cluster"),
            HeapObject::Barrier(_) => buf.push_str("Barrier"),
            HeapObject::SqliteConn(_) => buf.push_str("SqliteConn"),
            HeapObject::StructInst(s) => buf.push_str(&s.def.name),
            _ => buf.push_str(&self.to_string()),
        }
    }
    /// `unwrap_atomic` — see implementation.
    #[inline]
    pub fn unwrap_atomic(&self) -> StrykeValue {
        if !nanbox::is_heap(self.0) {
            return self.clone();
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Atomic(a) => a.lock().clone(),
            _ => self.clone(),
        }
    }
    /// `is_atomic` — see implementation.
    #[inline]
    pub fn is_atomic(&self) -> bool {
        if !nanbox::is_heap(self.0) {
            return false;
        }
        matches!(unsafe { self.heap_ref() }, HeapObject::Atomic(_))
    }
    /// `is_true` — see implementation.
    #[inline]
    pub fn is_true(&self) -> bool {
        if nanbox::is_imm_undef(self.0) {
            return false;
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            return n != 0;
        }
        if nanbox::is_raw_float_bits(self.0) {
            return f64::from_bits(self.0) != 0.0;
        }
        match unsafe { self.heap_ref() } {
            HeapObject::ErrnoDual { code, msg } => *code != 0 || !msg.is_empty(),
            HeapObject::String(s) => !s.is_empty() && s != "0",
            HeapObject::Bytes(b) => !b.is_empty(),
            HeapObject::BigInt(b) => {
                use num_traits::Zero;
                !b.is_zero()
            }
            HeapObject::Array(a) => !a.is_empty(),
            HeapObject::Hash(h) => !h.is_empty(),
            HeapObject::Atomic(arc) => arc.lock().is_true(),
            HeapObject::Set(s) => !s.is_empty(),
            HeapObject::Deque(d) => !d.lock().is_empty(),
            HeapObject::Heap(h) => !h.lock().items.is_empty(),
            HeapObject::Mutex(m) => *m.held.lock(),
            HeapObject::Semaphore(s) => *s.permits.lock() > 0,
            HeapObject::DataFrame(d) => d.lock().nrows() > 0,
            HeapObject::Pipeline(_) | HeapObject::Capture(_) => true,
            _ => true,
        }
    }

    /// String concat with owned LHS: moves out a uniquely held heap string when possible
    /// ([`Self::into_string`]), then appends `rhs`. Used for `.=` and VM concat-append ops.
    #[inline]
    pub(crate) fn concat_append_owned(self, rhs: &StrykeValue) -> StrykeValue {
        let mut s = self.into_string();
        rhs.append_to(&mut s);
        StrykeValue::string(s)
    }

    /// In-place repeated `.=` for the fused counted-loop superinstruction:
    /// append `rhs` exactly `n` times to the sole-owned heap `String` behind
    /// `self`, reserving once. Returns `false` (leaving `self` untouched) when
    /// the value is not a uniquely-held `HeapObject::String` — the VM then
    /// falls back to the per-iteration slow path.
    #[inline]
    pub(crate) fn try_concat_repeat_inplace(&mut self, rhs: &str, n: usize) -> bool {
        if !nanbox::is_heap(self.0) || n == 0 {
            // n==0 is trivially "done" in the caller's sense — nothing to append.
            return n == 0 && nanbox::is_heap(self.0);
        }
        unsafe {
            if !matches!(self.heap_ref(), HeapObject::String(_)) {
                return false;
            }
            let raw = nanbox::decode_heap_ptr::<HeapObject>(self.0) as *mut HeapObject
                as *const HeapObject;
            let mut arc: Arc<HeapObject> = Arc::from_raw(raw);
            let did = if let Some(HeapObject::String(s)) = Arc::get_mut(&mut arc) {
                if !rhs.is_empty() {
                    s.reserve(rhs.len().saturating_mul(n));
                    for _ in 0..n {
                        s.push_str(rhs);
                    }
                }
                true
            } else {
                false
            };
            let restored = Arc::into_raw(arc);
            self.0 = nanbox::encode_heap_ptr(restored);
            did
        }
    }

    /// In-place `.=` fast path: when `self` is the **sole owner** of a heap
    /// `HeapObject::String`, append `rhs` straight into the existing `String`
    /// buffer — no `Arc` allocation, no unwrap/rewrap churn, `String::push_str`
    /// reuses spare capacity and only reallocates on growth.
    ///
    /// Returns `true` if the in-place path ran (no further work for the caller),
    /// `false` when the value was not a heap String or the `Arc` was shared —
    /// the caller must then fall back to [`Self::concat_append_owned`] so that a
    /// second handle to the same `Arc` never observes a torn midway write.
    #[inline]
    pub(crate) fn try_concat_append_inplace(&mut self, rhs: &StrykeValue) -> bool {
        if !nanbox::is_heap(self.0) {
            return false;
        }
        // Peek without bumping the refcount to bail early on non-String payloads.
        // SAFETY: nanbox::is_heap holds (checked above), so the payload is a live
        // `Arc<HeapObject>` whose pointer we decode below.
        unsafe {
            if !matches!(self.heap_ref(), HeapObject::String(_)) {
                return false;
            }
            // Reconstitute the Arc to consult its strong count; `Arc::get_mut`
            // returns `Some` iff both strong and weak counts are 1.
            let raw = nanbox::decode_heap_ptr::<HeapObject>(self.0) as *mut HeapObject
                as *const HeapObject;
            let mut arc: Arc<HeapObject> = Arc::from_raw(raw);
            let did_append = if let Some(HeapObject::String(s)) = Arc::get_mut(&mut arc) {
                rhs.append_to(s);
                true
            } else {
                false
            };
            // Either way, hand the Arc back to the nanbox slot — we only ever
            // borrowed the single strong reference we started with.
            let restored = Arc::into_raw(arc);
            self.0 = nanbox::encode_heap_ptr(restored);
            did_append
        }
    }
    /// `into_string` — see implementation.
    #[inline]
    pub fn into_string(self) -> String {
        let bits = self.0;
        std::mem::forget(self);
        if nanbox::is_imm_undef(bits) {
            return String::new();
        }
        if let Some(n) = nanbox::as_imm_int32(bits) {
            let mut buf = itoa::Buffer::new();
            return buf.format(n).to_owned();
        }
        if nanbox::is_raw_float_bits(bits) {
            return format_float(f64::from_bits(bits));
        }
        if nanbox::is_heap(bits) {
            unsafe {
                let arc =
                    Arc::from_raw(nanbox::decode_heap_ptr::<HeapObject>(bits) as *mut HeapObject);
                match Arc::try_unwrap(arc) {
                    Ok(HeapObject::String(s)) => return s,
                    Ok(o) => return StrykeValue::from_heap(Arc::new(o)).to_string(),
                    Err(arc) => {
                        return match &*arc {
                            HeapObject::String(s) => s.clone(),
                            _ => StrykeValue::from_heap(Arc::clone(&arc)).to_string(),
                        };
                    }
                }
            }
        }
        String::new()
    }
    /// `as_str_or_empty` — see implementation.
    #[inline]
    pub fn as_str_or_empty(&self) -> String {
        if !nanbox::is_heap(self.0) {
            return String::new();
        }
        match unsafe { self.heap_ref() } {
            HeapObject::String(s) => s.clone(),
            HeapObject::ErrnoDual { msg, .. } => msg.clone(),
            _ => String::new(),
        }
    }
    /// `to_number` — see implementation.
    #[inline]
    pub fn to_number(&self) -> f64 {
        if nanbox::is_imm_undef(self.0) {
            return 0.0;
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            return n as f64;
        }
        if nanbox::is_raw_float_bits(self.0) {
            return f64::from_bits(self.0);
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Integer(n) => *n as f64,
            HeapObject::BigInt(b) => {
                use num_traits::ToPrimitive;
                b.to_f64().unwrap_or(f64::INFINITY)
            }
            HeapObject::Float(f) => *f,
            HeapObject::ErrnoDual { code, .. } => *code as f64,
            HeapObject::String(s) => parse_number(s),
            HeapObject::Bytes(b) => b.len() as f64,
            HeapObject::Array(a) => a.len() as f64,
            HeapObject::Atomic(arc) => arc.lock().to_number(),
            HeapObject::Set(s) => s.len() as f64,
            HeapObject::ChannelTx(_)
            | HeapObject::ChannelRx(_)
            | HeapObject::AsyncTask(_)
            | HeapObject::Generator(_) => 1.0,
            HeapObject::Deque(d) => d.lock().len() as f64,
            HeapObject::Heap(h) => h.lock().items.len() as f64,
            HeapObject::Mutex(m) => i64::from(*m.held.lock()) as f64,
            HeapObject::Semaphore(s) => *s.permits.lock() as f64,
            HeapObject::Pipeline(p) => p.lock().source.len() as f64,
            HeapObject::DataFrame(d) => d.lock().nrows() as f64,
            HeapObject::Capture(_)
            | HeapObject::Ppool(_)
            | HeapObject::RemoteCluster(_)
            | HeapObject::Barrier(_)
            | HeapObject::SqliteConn(_)
            | HeapObject::StructInst(_)
            | HeapObject::IOHandle(_) => 1.0,
            _ => 0.0,
        }
    }
    /// `to_int` — see implementation.
    #[inline]
    pub fn to_int(&self) -> i64 {
        if nanbox::is_imm_undef(self.0) {
            return 0;
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            return n as i64;
        }
        if nanbox::is_raw_float_bits(self.0) {
            return f64::from_bits(self.0) as i64;
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Integer(n) => *n,
            HeapObject::BigInt(b) => {
                use num_traits::ToPrimitive;
                b.to_i64().unwrap_or(i64::MAX)
            }
            HeapObject::Float(f) => *f as i64,
            HeapObject::ErrnoDual { code, .. } => *code as i64,
            HeapObject::String(s) => parse_number(s) as i64,
            HeapObject::Bytes(b) => b.len() as i64,
            HeapObject::Array(a) => a.len() as i64,
            HeapObject::Atomic(arc) => arc.lock().to_int(),
            HeapObject::Set(s) => s.len() as i64,
            HeapObject::ChannelTx(_)
            | HeapObject::ChannelRx(_)
            | HeapObject::AsyncTask(_)
            | HeapObject::Generator(_) => 1,
            HeapObject::Deque(d) => d.lock().len() as i64,
            HeapObject::Heap(h) => h.lock().items.len() as i64,
            HeapObject::Mutex(m) => i64::from(*m.held.lock()),
            HeapObject::Semaphore(s) => *s.permits.lock(),
            HeapObject::Pipeline(p) => p.lock().source.len() as i64,
            HeapObject::DataFrame(d) => d.lock().nrows() as i64,
            HeapObject::Capture(_)
            | HeapObject::Ppool(_)
            | HeapObject::RemoteCluster(_)
            | HeapObject::Barrier(_)
            | HeapObject::SqliteConn(_)
            | HeapObject::StructInst(_)
            | HeapObject::IOHandle(_) => 1,
            _ => 0,
        }
    }
    /// `type_name` — see implementation.
    pub fn type_name(&self) -> String {
        if nanbox::is_imm_undef(self.0) {
            return "undef".to_string();
        }
        if nanbox::as_imm_int32(self.0).is_some() {
            return "INTEGER".to_string();
        }
        if nanbox::is_raw_float_bits(self.0) {
            return "FLOAT".to_string();
        }
        match unsafe { self.heap_ref() } {
            HeapObject::String(_) => "STRING".to_string(),
            HeapObject::Bytes(_) => "BYTES".to_string(),
            HeapObject::Array(_) => "ARRAY".to_string(),
            HeapObject::Hash(_) => "HASH".to_string(),
            HeapObject::ArrayRef(_) | HeapObject::ArrayBindingRef(_) => "ARRAY".to_string(),
            HeapObject::HashRef(_) | HeapObject::HashBindingRef(_) => "HASH".to_string(),
            HeapObject::ScalarRef(_)
            | HeapObject::ScalarBindingRef(_)
            | HeapObject::CaptureCell(_) => "SCALAR".to_string(),
            HeapObject::CodeRef(_) => "CODE".to_string(),
            HeapObject::Regex(_, _, _) => "Regexp".to_string(),
            HeapObject::Blessed(b) => b.class.clone(),
            HeapObject::IOHandle(_) => "GLOB".to_string(),
            HeapObject::Atomic(_) => "ATOMIC".to_string(),
            HeapObject::Set(_) => "Set".to_string(),
            HeapObject::ChannelTx(_) => "PCHANNEL::Tx".to_string(),
            HeapObject::ChannelRx(_) => "PCHANNEL::Rx".to_string(),
            HeapObject::AsyncTask(_) => "ASYNCTASK".to_string(),
            HeapObject::Generator(_) => "Generator".to_string(),
            HeapObject::Deque(_) => "Deque".to_string(),
            HeapObject::Heap(_) => "Heap".to_string(),
            HeapObject::Mutex(_) => "Mutex".to_string(),
            HeapObject::Semaphore(_) => "Semaphore".to_string(),
            HeapObject::BloomFilter(_) => "BloomFilter".to_string(),
            HeapObject::HllSketch(_) => "HllSketch".to_string(),
            HeapObject::CmsSketch(_) => "CmsSketch".to_string(),
            HeapObject::TopKSketch(_) => "TopKSketch".to_string(),
            HeapObject::TDigestSketch(_) => "TDigestSketch".to_string(),
            HeapObject::RoaringBitmap(_) => "RoaringBitmap".to_string(),
            HeapObject::RateLimiter(_) => "RateLimiter".to_string(),
            HeapObject::HashRing(_) => "HashRing".to_string(),
            HeapObject::SimHash(_) => "SimHash".to_string(),
            HeapObject::MinHash(_) => "MinHash".to_string(),
            HeapObject::IntervalTree(_) => "IntervalTree".to_string(),
            HeapObject::BkTree(_) => "BkTree".to_string(),
            HeapObject::Rope(_) => "Rope".to_string(),
            HeapObject::KvStore(_) => "KvStore".to_string(),
            HeapObject::Pipeline(_) => "Pipeline".to_string(),
            HeapObject::DataFrame(_) => "DataFrame".to_string(),
            HeapObject::Capture(_) => "Capture".to_string(),
            HeapObject::Ppool(_) => "Ppool".to_string(),
            HeapObject::RemoteCluster(_) => "Cluster".to_string(),
            HeapObject::Barrier(_) => "Barrier".to_string(),
            HeapObject::SqliteConn(_) => "SqliteConn".to_string(),
            HeapObject::StructInst(s) => s.def.name.to_string(),
            HeapObject::EnumInst(e) => e.def.name.to_string(),
            HeapObject::ClassInst(c) => c.def.name.to_string(),
            HeapObject::Iterator(_) => "Iterator".to_string(),
            HeapObject::ErrnoDual { .. } => "Errno".to_string(),
            HeapObject::Integer(_) => "INTEGER".to_string(),
            HeapObject::BigInt(_) => "INTEGER".to_string(),
            HeapObject::Float(_) => "FLOAT".to_string(),
        }
    }
    /// `ref_type` — see implementation.
    pub fn ref_type(&self) -> StrykeValue {
        if !nanbox::is_heap(self.0) {
            return StrykeValue::string(String::new());
        }
        match unsafe { self.heap_ref() } {
            HeapObject::ArrayRef(_) | HeapObject::ArrayBindingRef(_) => {
                StrykeValue::string("ARRAY".into())
            }
            HeapObject::HashRef(_) | HeapObject::HashBindingRef(_) => {
                StrykeValue::string("HASH".into())
            }
            HeapObject::ScalarRef(_) | HeapObject::ScalarBindingRef(_) => {
                StrykeValue::string("SCALAR".into())
            }
            HeapObject::CodeRef(_) => StrykeValue::string("CODE".into()),
            HeapObject::Regex(_, _, _) => StrykeValue::string("Regexp".into()),
            HeapObject::Atomic(_) => StrykeValue::string("ATOMIC".into()),
            HeapObject::Set(_) => StrykeValue::string("Set".into()),
            HeapObject::ChannelTx(_) => StrykeValue::string("PCHANNEL::Tx".into()),
            HeapObject::ChannelRx(_) => StrykeValue::string("PCHANNEL::Rx".into()),
            HeapObject::AsyncTask(_) => StrykeValue::string("ASYNCTASK".into()),
            HeapObject::Generator(_) => StrykeValue::string("Generator".into()),
            HeapObject::Deque(_) => StrykeValue::string("Deque".into()),
            HeapObject::Heap(_) => StrykeValue::string("Heap".into()),
            HeapObject::Mutex(_) => StrykeValue::string("Mutex".into()),
            HeapObject::Semaphore(_) => StrykeValue::string("Semaphore".into()),
            HeapObject::BloomFilter(_) => StrykeValue::string("BloomFilter".into()),
            HeapObject::HllSketch(_) => StrykeValue::string("HllSketch".into()),
            HeapObject::CmsSketch(_) => StrykeValue::string("CmsSketch".into()),
            HeapObject::TopKSketch(_) => StrykeValue::string("TopKSketch".into()),
            HeapObject::TDigestSketch(_) => StrykeValue::string("TDigestSketch".into()),
            HeapObject::RoaringBitmap(_) => StrykeValue::string("RoaringBitmap".into()),
            HeapObject::RateLimiter(_) => StrykeValue::string("RateLimiter".into()),
            HeapObject::HashRing(_) => StrykeValue::string("HashRing".into()),
            HeapObject::SimHash(_) => StrykeValue::string("SimHash".into()),
            HeapObject::MinHash(_) => StrykeValue::string("MinHash".into()),
            HeapObject::IntervalTree(_) => StrykeValue::string("IntervalTree".into()),
            HeapObject::BkTree(_) => StrykeValue::string("BkTree".into()),
            HeapObject::Rope(_) => StrykeValue::string("Rope".into()),
            HeapObject::KvStore(_) => StrykeValue::string("KvStore".into()),
            HeapObject::Pipeline(_) => StrykeValue::string("Pipeline".into()),
            HeapObject::DataFrame(_) => StrykeValue::string("DataFrame".into()),
            HeapObject::Capture(_) => StrykeValue::string("Capture".into()),
            HeapObject::Ppool(_) => StrykeValue::string("Ppool".into()),
            HeapObject::RemoteCluster(_) => StrykeValue::string("Cluster".into()),
            HeapObject::Barrier(_) => StrykeValue::string("Barrier".into()),
            HeapObject::SqliteConn(_) => StrykeValue::string("SqliteConn".into()),
            HeapObject::StructInst(s) => StrykeValue::string(s.def.name.clone()),
            HeapObject::EnumInst(e) => StrykeValue::string(e.def.name.clone()),
            HeapObject::ClassInst(c) => StrykeValue::string(c.def.name.clone()),
            HeapObject::Bytes(_) => StrykeValue::string("BYTES".into()),
            HeapObject::Blessed(b) => StrykeValue::string(b.class.clone()),
            _ => StrykeValue::string(String::new()),
        }
    }
    /// `num_cmp` — see implementation.
    pub fn num_cmp(&self, other: &StrykeValue) -> Ordering {
        let a = self.to_number();
        let b = other.to_number();
        a.partial_cmp(&b).unwrap_or(Ordering::Equal)
    }

    /// String equality for `eq` / `cmp` without allocating when both sides are heap strings.
    #[inline]
    pub fn str_eq(&self, other: &StrykeValue) -> bool {
        if nanbox::is_heap(self.0) && nanbox::is_heap(other.0) {
            if let (HeapObject::String(a), HeapObject::String(b)) =
                unsafe { (self.heap_ref(), other.heap_ref()) }
            {
                return a == b;
            }
        }
        self.to_string() == other.to_string()
    }
    /// `str_cmp` — see implementation.
    pub fn str_cmp(&self, other: &StrykeValue) -> Ordering {
        if nanbox::is_heap(self.0) && nanbox::is_heap(other.0) {
            if let (HeapObject::String(a), HeapObject::String(b)) =
                unsafe { (self.heap_ref(), other.heap_ref()) }
            {
                return a.cmp(b);
            }
        }
        self.to_string().cmp(&other.to_string())
    }

    /// Deep equality for struct fields (recursive).
    pub fn struct_field_eq(&self, other: &StrykeValue) -> bool {
        if nanbox::is_imm_undef(self.0) && nanbox::is_imm_undef(other.0) {
            return true;
        }
        if let (Some(a), Some(b)) = (nanbox::as_imm_int32(self.0), nanbox::as_imm_int32(other.0)) {
            return a == b;
        }
        if nanbox::is_raw_float_bits(self.0) && nanbox::is_raw_float_bits(other.0) {
            return f64::from_bits(self.0) == f64::from_bits(other.0);
        }
        if !nanbox::is_heap(self.0) || !nanbox::is_heap(other.0) {
            return self.to_number() == other.to_number();
        }
        match (unsafe { self.heap_ref() }, unsafe { other.heap_ref() }) {
            (HeapObject::String(a), HeapObject::String(b)) => a == b,
            (HeapObject::Integer(a), HeapObject::Integer(b)) => a == b,
            (HeapObject::BigInt(a), HeapObject::BigInt(b)) => a == b,
            (HeapObject::BigInt(a), HeapObject::Integer(b))
            | (HeapObject::Integer(b), HeapObject::BigInt(a)) => a.as_ref() == &BigInt::from(*b),
            (HeapObject::Float(a), HeapObject::Float(b)) => a == b,
            (HeapObject::Array(a), HeapObject::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.struct_field_eq(y))
            }
            (HeapObject::ArrayRef(a), HeapObject::ArrayRef(b)) => {
                let ag = a.read();
                let bg = b.read();
                ag.len() == bg.len() && ag.iter().zip(bg.iter()).all(|(x, y)| x.struct_field_eq(y))
            }
            (HeapObject::Hash(a), HeapObject::Hash(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v)| b.get(k).is_some_and(|bv| v.struct_field_eq(bv)))
            }
            (HeapObject::HashRef(a), HeapObject::HashRef(b)) => {
                let ag = a.read();
                let bg = b.read();
                ag.len() == bg.len()
                    && ag
                        .iter()
                        .all(|(k, v)| bg.get(k).is_some_and(|bv| v.struct_field_eq(bv)))
            }
            (HeapObject::StructInst(a), HeapObject::StructInst(b)) => {
                if a.def.name != b.def.name {
                    false
                } else {
                    let av = a.get_values();
                    let bv = b.get_values();
                    av.len() == bv.len()
                        && av.iter().zip(bv.iter()).all(|(x, y)| x.struct_field_eq(y))
                }
            }
            _ => self.to_string() == other.to_string(),
        }
    }

    /// Deep clone a value (used for struct clone).
    pub fn deep_clone(&self) -> StrykeValue {
        if !nanbox::is_heap(self.0) {
            return self.clone();
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Array(a) => StrykeValue::array(a.iter().map(|v| v.deep_clone()).collect()),
            HeapObject::ArrayRef(a) => {
                let cloned: Vec<StrykeValue> = a.read().iter().map(|v| v.deep_clone()).collect();
                StrykeValue::array_ref(Arc::new(RwLock::new(cloned)))
            }
            HeapObject::Hash(h) => {
                let mut cloned = IndexMap::new();
                for (k, v) in h.iter() {
                    cloned.insert(k.clone(), v.deep_clone());
                }
                StrykeValue::hash(cloned)
            }
            HeapObject::HashRef(h) => {
                let mut cloned = IndexMap::new();
                for (k, v) in h.read().iter() {
                    cloned.insert(k.clone(), v.deep_clone());
                }
                StrykeValue::hash_ref(Arc::new(RwLock::new(cloned)))
            }
            HeapObject::StructInst(s) => {
                let new_values = s.get_values().iter().map(|v| v.deep_clone()).collect();
                StrykeValue::struct_inst(Arc::new(StructInstance::new(
                    Arc::clone(&s.def),
                    new_values,
                )))
            }
            _ => self.clone(),
        }
    }
    /// `to_list` — see implementation.
    pub fn to_list(&self) -> Vec<StrykeValue> {
        if nanbox::is_imm_undef(self.0) {
            return vec![];
        }
        if !nanbox::is_heap(self.0) {
            return vec![self.clone()];
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Array(a) => a.clone(),
            HeapObject::Hash(h) => h
                .iter()
                .flat_map(|(k, v)| vec![StrykeValue::string(k.clone()), v.clone()])
                .collect(),
            HeapObject::Atomic(arc) => arc.lock().to_list(),
            HeapObject::Set(s) => s.values().cloned().collect(),
            HeapObject::Deque(d) => d.lock().iter().cloned().collect(),
            HeapObject::Iterator(it) => {
                let mut out = Vec::new();
                while let Some(v) = it.next_item() {
                    out.push(v);
                }
                out
            }
            _ => vec![self.clone()],
        }
    }
    /// `scalar_context` — see implementation.
    pub fn scalar_context(&self) -> StrykeValue {
        if !nanbox::is_heap(self.0) {
            return self.clone();
        }
        if let Some(arc) = self.as_atomic_arc() {
            return arc.lock().scalar_context();
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Array(a) => StrykeValue::integer(a.len() as i64),
            HeapObject::Hash(h) => {
                if h.is_empty() {
                    StrykeValue::integer(0)
                } else {
                    StrykeValue::string(format!("{}/{}", h.len(), h.capacity()))
                }
            }
            HeapObject::Set(s) => StrykeValue::integer(s.len() as i64),
            HeapObject::Deque(d) => StrykeValue::integer(d.lock().len() as i64),
            HeapObject::Heap(h) => StrykeValue::integer(h.lock().items.len() as i64),
            HeapObject::Mutex(m) => StrykeValue::integer(i64::from(*m.held.lock())),
            HeapObject::Semaphore(s) => StrykeValue::integer(*s.permits.lock()),
            HeapObject::Pipeline(p) => StrykeValue::integer(p.lock().source.len() as i64),
            HeapObject::Capture(_)
            | HeapObject::Ppool(_)
            | HeapObject::RemoteCluster(_)
            | HeapObject::Barrier(_) => StrykeValue::integer(1),
            HeapObject::Generator(_) => StrykeValue::integer(1),
            _ => self.clone(),
        }
    }
}

impl fmt::Display for StrykeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if nanbox::is_imm_undef(self.0) {
            return Ok(());
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            return write!(f, "{n}");
        }
        if nanbox::is_raw_float_bits(self.0) {
            return write!(f, "{}", format_float(f64::from_bits(self.0)));
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Integer(n) => write!(f, "{n}"),
            HeapObject::BigInt(b) => write!(f, "{b}"),
            HeapObject::Float(val) => write!(f, "{}", format_float(*val)),
            HeapObject::ErrnoDual { msg, .. } => f.write_str(msg),
            HeapObject::String(s) => f.write_str(s),
            HeapObject::Bytes(b) => f.write_str(&decode_utf8_or_latin1(b)),
            HeapObject::Array(a) => {
                for v in a {
                    write!(f, "{v}")?;
                }
                Ok(())
            }
            HeapObject::Hash(h) => write!(f, "{}/{}", h.len(), h.capacity()),
            HeapObject::ArrayRef(_) | HeapObject::ArrayBindingRef(_) => f.write_str("ARRAY(0x...)"),
            HeapObject::HashRef(_) | HeapObject::HashBindingRef(_) => f.write_str("HASH(0x...)"),
            HeapObject::ScalarRef(_)
            | HeapObject::ScalarBindingRef(_)
            | HeapObject::CaptureCell(_) => f.write_str("SCALAR(0x...)"),
            HeapObject::CodeRef(sub) => {
                // Match Perl's `CODE(0x<hexaddr>)` so distinct closures
                // stringify to distinct values and string comparison can
                // tell them apart. The Arc pointer is stable for the
                // lifetime of the closure instance and unique across
                // simultaneous instances (BUG-245).
                let addr = Arc::as_ptr(sub) as usize;
                write!(f, "CODE(0x{:x})", addr)
            }
            HeapObject::Regex(_, src, _) => write!(f, "(?:{src})"),
            HeapObject::Blessed(b) => write!(f, "{}=HASH(0x...)", b.class),
            HeapObject::IOHandle(name) => f.write_str(name),
            HeapObject::Atomic(arc) => write!(f, "{}", arc.lock()),
            HeapObject::Set(s) => {
                f.write_str("{")?;
                if !s.is_empty() {
                    let mut iter = s.values();
                    if let Some(v) = iter.next() {
                        write!(f, "{v}")?;
                    }
                    for v in iter {
                        write!(f, ",{v}")?;
                    }
                }
                f.write_str("}")
            }
            HeapObject::ChannelTx(_) => f.write_str("PCHANNEL::Tx"),
            HeapObject::ChannelRx(_) => f.write_str("PCHANNEL::Rx"),
            HeapObject::AsyncTask(_) => f.write_str("AsyncTask"),
            HeapObject::Generator(g) => write!(f, "Generator({} stmts)", g.block.len()),
            HeapObject::Deque(d) => write!(f, "Deque({})", d.lock().len()),
            HeapObject::Heap(h) => write!(f, "Heap({})", h.lock().items.len()),
            HeapObject::Mutex(m) => write!(f, "Mutex({})", *m.held.lock()),
            HeapObject::Semaphore(s) => {
                write!(f, "Semaphore({}/{})", *s.permits.lock(), s.limit)
            }
            HeapObject::BloomFilter(b) => {
                let g = b.lock();
                write!(
                    f,
                    "BloomFilter(n={}, bits={}, k={})",
                    g.inserted(),
                    g.bit_count(),
                    g.k()
                )
            }
            HeapObject::HllSketch(s) => {
                let g = s.lock();
                write!(f, "HllSketch(p={}, m={})", g.precision(), g.registers_len())
            }
            HeapObject::CmsSketch(s) => {
                let g = s.lock();
                write!(f, "CmsSketch(w={}, d={})", g.width(), g.depth())
            }
            HeapObject::TopKSketch(s) => {
                let g = s.lock();
                write!(f, "TopKSketch(k={}, n={})", g.k(), g.size())
            }
            HeapObject::TDigestSketch(s) => {
                let g = s.lock();
                write!(f, "TDigestSketch(compression={})", g.compression())
            }
            HeapObject::RoaringBitmap(s) => {
                let g = s.lock();
                write!(f, "RoaringBitmap(n={})", g.len())
            }
            HeapObject::RateLimiter(s) => {
                let g = s.lock();
                let kind = if g.leaky { "leaky" } else { "token" };
                write!(
                    f,
                    "RateLimiter({}, cap={}, rate={}/s)",
                    kind, g.capacity, g.rate_per_sec
                )
            }
            HeapObject::HashRing(s) => {
                let g = s.lock();
                write!(
                    f,
                    "HashRing(nodes={}, vnodes={})",
                    g.node_count(),
                    g.vnodes_per_node
                )
            }
            HeapObject::SimHash(s) => {
                let g = s.lock();
                write!(f, "SimHash(features={})", g.feature_count())
            }
            HeapObject::MinHash(s) => {
                let g = s.lock();
                write!(f, "MinHash(k={})", g.k())
            }
            HeapObject::IntervalTree(s) => {
                let g = s.lock();
                write!(f, "IntervalTree(n={})", g.len())
            }
            HeapObject::BkTree(s) => {
                let g = s.lock();
                write!(f, "BkTree(n={})", g.len())
            }
            HeapObject::Rope(s) => {
                let g = s.lock();
                write!(f, "Rope(len={}, bytes={})", g.len(), g.byte_len())
            }
            HeapObject::Pipeline(p) => {
                let g = p.lock();
                write!(f, "Pipeline({} ops)", g.ops.len())
            }
            HeapObject::Capture(c) => write!(f, "Capture(exit={})", c.exitcode),
            HeapObject::Ppool(_) => f.write_str("Ppool"),
            HeapObject::RemoteCluster(c) => write!(f, "Cluster({} slots)", c.slots.len()),
            HeapObject::Barrier(_) => f.write_str("Barrier"),
            HeapObject::SqliteConn(_) => f.write_str("SqliteConn"),
            HeapObject::StructInst(s) => {
                // Smart stringify: Point(x => 1.5, y => 2.0)
                write!(f, "{}(", s.def.name)?;
                let values = s.values.read();
                for (i, field) in s.def.fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(
                        f,
                        "{} => {}",
                        field.name,
                        values.get(i).cloned().unwrap_or(StrykeValue::UNDEF)
                    )?;
                }
                f.write_str(")")
            }
            HeapObject::EnumInst(e) => {
                // Smart stringify: Color::Red or Maybe::Some(value)
                write!(f, "{}::{}", e.def.name, e.variant_name())?;
                if e.def.variants[e.variant_idx].ty.is_some() {
                    write!(f, "({})", e.data)?;
                }
                Ok(())
            }
            HeapObject::ClassInst(c) => {
                // Smart stringify: Dog(name => "Rex", age => 5)
                write!(f, "{}(", c.def.name)?;
                let values = c.values.read();
                for (i, field) in c.def.fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(
                        f,
                        "{} => {}",
                        field.name,
                        values.get(i).cloned().unwrap_or(StrykeValue::UNDEF)
                    )?;
                }
                f.write_str(")")
            }
            HeapObject::DataFrame(d) => {
                let g = d.lock();
                write!(f, "DataFrame({} rows)", g.nrows())
            }
            HeapObject::Iterator(_) => f.write_str("Iterator"),
            HeapObject::KvStore(s) => {
                let g = s.lock();
                write!(f, "KvStore({} entries)", g.len())
            }
        }
    }
}

/// Stable key for set membership (dedup of `StrykeValue` in this runtime).
pub fn set_member_key(v: &StrykeValue) -> String {
    if nanbox::is_imm_undef(v.0) {
        return "u:".to_string();
    }
    if let Some(n) = nanbox::as_imm_int32(v.0) {
        return format!("i:{n}");
    }
    if nanbox::is_raw_float_bits(v.0) {
        return format!("f:{}", f64::from_bits(v.0).to_bits());
    }
    match unsafe { v.heap_ref() } {
        HeapObject::String(s) => format!("s:{s}"),
        HeapObject::Bytes(b) => {
            use std::fmt::Write as _;
            let mut h = String::with_capacity(b.len() * 2);
            for &x in b.iter() {
                let _ = write!(&mut h, "{:02x}", x);
            }
            format!("by:{h}")
        }
        HeapObject::Array(a) => {
            let parts: Vec<_> = a.iter().map(set_member_key).collect();
            format!("a:{}", parts.join(","))
        }
        HeapObject::Hash(h) => {
            let mut keys: Vec<_> = h.keys().cloned().collect();
            keys.sort();
            let parts: Vec<_> = keys
                .iter()
                .map(|k| format!("{}={}", k, set_member_key(h.get(k).unwrap())))
                .collect();
            format!("h:{}", parts.join(","))
        }
        HeapObject::Set(inner) => {
            let mut keys: Vec<_> = inner.keys().cloned().collect();
            keys.sort();
            format!("S:{}", keys.join(","))
        }
        HeapObject::ArrayRef(a) => {
            let g = a.read();
            let parts: Vec<_> = g.iter().map(set_member_key).collect();
            format!("ar:{}", parts.join(","))
        }
        HeapObject::HashRef(h) => {
            let g = h.read();
            let mut keys: Vec<_> = g.keys().cloned().collect();
            keys.sort();
            let parts: Vec<_> = keys
                .iter()
                .map(|k| format!("{}={}", k, set_member_key(g.get(k).unwrap())))
                .collect();
            format!("hr:{}", parts.join(","))
        }
        HeapObject::Blessed(b) => {
            let d = b.data.read();
            format!("b:{}:{}", b.class, set_member_key(&d))
        }
        HeapObject::ScalarRef(_) | HeapObject::ScalarBindingRef(_) | HeapObject::CaptureCell(_) => {
            format!("sr:{v}")
        }
        HeapObject::ArrayBindingRef(n) => format!("abind:{n}"),
        HeapObject::HashBindingRef(n) => format!("hbind:{n}"),
        HeapObject::CodeRef(_) => format!("c:{v}"),
        HeapObject::Regex(_, src, _) => format!("r:{src}"),
        HeapObject::IOHandle(s) => format!("io:{s}"),
        HeapObject::Atomic(arc) => format!("at:{}", set_member_key(&arc.lock())),
        HeapObject::ChannelTx(tx) => format!("chtx:{:p}", Arc::as_ptr(tx)),
        HeapObject::ChannelRx(rx) => format!("chrx:{:p}", Arc::as_ptr(rx)),
        HeapObject::AsyncTask(t) => format!("async:{:p}", Arc::as_ptr(t)),
        HeapObject::Generator(g) => format!("gen:{:p}", Arc::as_ptr(g)),
        HeapObject::Deque(d) => format!("dq:{:p}", Arc::as_ptr(d)),
        HeapObject::Heap(h) => format!("hp:{:p}", Arc::as_ptr(h)),
        HeapObject::Mutex(m) => format!("mu:{:p}", Arc::as_ptr(m)),
        HeapObject::Semaphore(s) => format!("se:{:p}", Arc::as_ptr(s)),
        HeapObject::BloomFilter(b) => format!("bf:{:p}", Arc::as_ptr(b)),
        HeapObject::HllSketch(s) => format!("hll:{:p}", Arc::as_ptr(s)),
        HeapObject::CmsSketch(s) => format!("cms:{:p}", Arc::as_ptr(s)),
        HeapObject::TopKSketch(s) => format!("topk:{:p}", Arc::as_ptr(s)),
        HeapObject::TDigestSketch(s) => format!("td:{:p}", Arc::as_ptr(s)),
        HeapObject::RoaringBitmap(s) => format!("rb:{:p}", Arc::as_ptr(s)),
        HeapObject::RateLimiter(s) => format!("rl:{:p}", Arc::as_ptr(s)),
        HeapObject::HashRing(s) => format!("hr:{:p}", Arc::as_ptr(s)),
        HeapObject::SimHash(s) => format!("sh:{:p}", Arc::as_ptr(s)),
        HeapObject::MinHash(s) => format!("mh:{:p}", Arc::as_ptr(s)),
        HeapObject::IntervalTree(s) => format!("it:{:p}", Arc::as_ptr(s)),
        HeapObject::BkTree(s) => format!("bk:{:p}", Arc::as_ptr(s)),
        HeapObject::Rope(s) => format!("rp:{:p}", Arc::as_ptr(s)),
        HeapObject::Pipeline(p) => format!("pl:{:p}", Arc::as_ptr(p)),
        HeapObject::Capture(c) => format!("cap:{:p}", Arc::as_ptr(c)),
        HeapObject::Ppool(p) => format!("pp:{:p}", Arc::as_ptr(&p.0)),
        HeapObject::RemoteCluster(c) => format!("rcl:{:p}", Arc::as_ptr(c)),
        HeapObject::Barrier(b) => format!("br:{:p}", Arc::as_ptr(&b.0)),
        HeapObject::SqliteConn(c) => format!("sql:{:p}", Arc::as_ptr(c)),
        HeapObject::StructInst(s) => format!("st:{}:{:?}", s.def.name, s.values),
        HeapObject::EnumInst(e) => {
            format!("en:{}::{}:{}", e.def.name, e.variant_name(), e.data)
        }
        HeapObject::ClassInst(c) => format!("cl:{}:{:?}", c.def.name, c.values),
        HeapObject::DataFrame(d) => format!("df:{:p}", Arc::as_ptr(d)),
        HeapObject::KvStore(s) => format!("kv:{:p}", Arc::as_ptr(s)),
        HeapObject::Iterator(_) => "iter".to_string(),
        HeapObject::ErrnoDual { code, msg } => format!("e:{code}:{msg}"),
        HeapObject::Integer(n) => format!("i:{n}"),
        HeapObject::BigInt(b) => format!("bi:{b}"),
        HeapObject::Float(fl) => format!("f:{}", fl.to_bits()),
    }
}

/// Perl-style integer modulo: floored division, so the result has the
/// sign of the divisor (or is zero). Defined for all `b != 0`. Rust's
/// `%` operator returns the sign of the dividend, which differs whenever
/// the operands have opposite signs.
///
/// Examples (matching Perl 5.42):
///   `perl_mod_i64(-7, 3) =  2`
///   `perl_mod_i64( 7,-3) = -2`
///   `perl_mod_i64(-7,-3) = -1`
///   `perl_mod_i64( 7, 3) =  1`
#[inline]
pub fn perl_mod_i64(a: i64, b: i64) -> i64 {
    debug_assert_ne!(b, 0);
    let r = a.wrapping_rem(b);
    // Sign mismatch between r and b, and r is non-zero → snap toward
    // the divisor's sign by adding b (won't overflow since |r| < |b|).
    if r != 0 && (r ^ b) < 0 {
        r + b
    } else {
        r
    }
}

/// Perl-compatible `<<` on a 64-bit signed integer. Shift amounts of `>= 64`
/// or `< 0` yield `0` instead of Rust's checked-shift panic. Bits shifted past
/// position 63 wrap (matches Perl's two's-complement IV behavior).
#[inline]
pub fn perl_shl_i64(a: i64, b: i64) -> i64 {
    if !(0..64).contains(&b) {
        0
    } else {
        ((a as u64).wrapping_shl(b as u32)) as i64
    }
}

/// Perl-compatible `>>` on a 64-bit signed integer. Shift amounts of `>= 64`
/// fully shift out the value (returning `0` for non-negative inputs and `-1`
/// for negative inputs under arithmetic shift); negative shift amounts yield
/// `0` instead of Rust's checked-shift panic.
#[inline]
pub fn perl_shr_i64(a: i64, b: i64) -> i64 {
    if b < 0 {
        0
    } else if b >= 64 {
        if a < 0 {
            -1
        } else {
            0
        }
    } else {
        a >> b
    }
}

/// `--compat`-aware integer multiply. In compat mode, promotes to `BigInt` on
/// overflow. In native mode, wraps (preserves current behavior). Either side
/// already being a `BigInt` forces the BigInt path.
#[inline]
pub fn compat_mul(a: &StrykeValue, b: &StrykeValue) -> StrykeValue {
    if a.as_bigint().is_some() || b.as_bigint().is_some() {
        return StrykeValue::bigint(a.to_bigint() * b.to_bigint());
    }
    let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) else {
        return StrykeValue::float(a.to_number() * b.to_number());
    };
    if crate::compat_mode() || crate::bigint_pragma() {
        match x.checked_mul(y) {
            Some(r) => StrykeValue::integer(r),
            None => StrykeValue::bigint(BigInt::from(x) * BigInt::from(y)),
        }
    } else {
        StrykeValue::integer(x.wrapping_mul(y))
    }
}
/// `compat_add` — see implementation.
#[inline]
pub fn compat_add(a: &StrykeValue, b: &StrykeValue) -> StrykeValue {
    if a.as_bigint().is_some() || b.as_bigint().is_some() {
        return StrykeValue::bigint(a.to_bigint() + b.to_bigint());
    }
    let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) else {
        return StrykeValue::float(a.to_number() + b.to_number());
    };
    if crate::compat_mode() || crate::bigint_pragma() {
        match x.checked_add(y) {
            Some(r) => StrykeValue::integer(r),
            None => StrykeValue::bigint(BigInt::from(x) + BigInt::from(y)),
        }
    } else {
        StrykeValue::integer(x.wrapping_add(y))
    }
}
/// `compat_sub` — see implementation.
#[inline]
pub fn compat_sub(a: &StrykeValue, b: &StrykeValue) -> StrykeValue {
    if a.as_bigint().is_some() || b.as_bigint().is_some() {
        return StrykeValue::bigint(a.to_bigint() - b.to_bigint());
    }
    let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) else {
        return StrykeValue::float(a.to_number() - b.to_number());
    };
    if crate::compat_mode() || crate::bigint_pragma() {
        match x.checked_sub(y) {
            Some(r) => StrykeValue::integer(r),
            None => StrykeValue::bigint(BigInt::from(x) - BigInt::from(y)),
        }
    } else {
        StrykeValue::integer(x.wrapping_sub(y))
    }
}

/// `**` (exponentiation) — under `--compat` or `use bigint;`, uses `BigInt`
/// directly when the exponent is a non-negative integer so `2 ** 100`
/// works. Falls through to `f64::powf` for negative or non-integer
/// exponents (matches Perl's behavior).
#[inline]
pub fn compat_pow(a: &StrykeValue, b: &StrykeValue) -> StrykeValue {
    let (Some(base), Some(exp)) = (a.as_integer(), b.as_integer()) else {
        return StrykeValue::float(a.to_number().powf(b.to_number()));
    };
    let bigint_active = crate::compat_mode() || crate::bigint_pragma();
    if !bigint_active {
        // Native: do whatever the existing path does — fall back to float
        // (matches Perl's default i64-overflow-to-NV behavior).
        return StrykeValue::float((base as f64).powf(exp as f64));
    }
    if exp < 0 {
        return StrykeValue::float((base as f64).powf(exp as f64));
    }
    use num_traits::Pow;
    let result = BigInt::from(base).pow(exp as u32);
    StrykeValue::bigint(result)
}
/// `set_from_elements` — see implementation.
pub fn set_from_elements<I: IntoIterator<Item = StrykeValue>>(items: I) -> StrykeValue {
    let mut map = PerlSet::new();
    for v in items {
        let k = set_member_key(&v);
        map.insert(k, v);
    }
    StrykeValue::set(Arc::new(map))
}

/// Underlying set for union/intersection, including `mysync $s` (`Atomic` wrapping `Set`).
#[inline]
pub fn set_payload(v: &StrykeValue) -> Option<Arc<PerlSet>> {
    if !nanbox::is_heap(v.0) {
        return None;
    }
    match unsafe { v.heap_ref() } {
        HeapObject::Set(s) => Some(Arc::clone(s)),
        HeapObject::Atomic(a) => set_payload(&a.lock()),
        _ => None,
    }
}
/// `set_union` — see implementation.
pub fn set_union(a: &StrykeValue, b: &StrykeValue) -> Option<StrykeValue> {
    let ia = set_payload(a)?;
    let ib = set_payload(b)?;
    let mut m = (*ia).clone();
    for (k, v) in ib.iter() {
        m.entry(k.clone()).or_insert_with(|| v.clone());
    }
    Some(StrykeValue::set(Arc::new(m)))
}
/// `set_intersection` — see implementation.
pub fn set_intersection(a: &StrykeValue, b: &StrykeValue) -> Option<StrykeValue> {
    let ia = set_payload(a)?;
    let ib = set_payload(b)?;
    let mut m = PerlSet::new();
    for (k, v) in ia.iter() {
        if ib.contains_key(k) {
            m.insert(k.clone(), v.clone());
        }
    }
    Some(StrykeValue::set(Arc::new(m)))
}
fn parse_number(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    // Perl 5.22+ recognizes "Inf" / "Infinity" / "NaN" (case-insensitive,
    // optional leading sign) as float specials. We accept the same forms.
    {
        let bytes = s.as_bytes();
        let (sign, rest) = match bytes.first() {
            Some(b'+') => (1.0_f64, &s[1..]),
            Some(b'-') => (-1.0_f64, &s[1..]),
            _ => (1.0_f64, s),
        };
        if rest.eq_ignore_ascii_case("inf") || rest.eq_ignore_ascii_case("infinity") {
            return sign * f64::INFINITY;
        }
        if rest.eq_ignore_ascii_case("nan") {
            // Perl's sign on NaN is preserved through arithmetic; here we
            // just return the canonical NaN bit pattern. Sign on NaN is
            // not observable via `==` anyway.
            return f64::NAN;
        }
    }
    // Perl extracts leading numeric portion
    let mut end = 0;
    let bytes = s.as_bytes();
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
        end += 1;
        if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
            end += 1;
        }
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end == 0 {
        return 0.0;
    }
    s[..end].parse::<f64>().unwrap_or(0.0)
}

fn format_float(f: f64) -> String {
    // Perl prints float specials as "Inf" / "-Inf" / "NaN".
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f.is_sign_negative() {
            "-Inf".to_string()
        } else {
            "Inf".to_string()
        };
    }
    if f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{}", f as i64)
    } else {
        // Perl uses Gconvert which is sprintf("%.15g", f) on most platforms.
        let mut buf = [0u8; 64];
        unsafe {
            libc::snprintf(
                buf.as_mut_ptr() as *mut libc::c_char,
                buf.len(),
                c"%.15g".as_ptr(),
                f,
            );
            std::ffi::CStr::from_ptr(buf.as_ptr() as *const libc::c_char)
                .to_string_lossy()
                .into_owned()
        }
    }
}

/// Result of one magical string increment step in a list-context `..` range (Perl `sv_inc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PerlListRangeIncOutcome {
    Continue,
    /// Perl upgraded the scalar to a numeric form (`SvNIOKp`); list range stops after this step.
    BecameNumeric,
}

/// Perl `looks_like_number` / `grok_number` subset: `s` must be **entirely** a numeric string
/// (after trim), with no trailing garbage. Used for `RANGE_IS_NUMERIC` in `pp_flop`.
fn perl_str_looks_like_number_for_range(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return s.is_empty();
    }
    let b = t.as_bytes();
    let mut i = 0usize;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    if i >= b.len() {
        return false;
    }
    let mut saw_digit = false;
    while i < b.len() && b[i].is_ascii_digit() {
        saw_digit = true;
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            saw_digit = true;
            i += 1;
        }
    }
    if !saw_digit {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp0 = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp0 {
            return false;
        }
    }
    i == b.len()
}

/// Whether list-context `..` uses Perl's **numeric** counting (`pp_flop` `RANGE_IS_NUMERIC`).
pub(crate) fn perl_list_range_pair_is_numeric(left: &StrykeValue, right: &StrykeValue) -> bool {
    if left.is_integer_like() || left.is_float_like() {
        return true;
    }
    if !left.is_undef() && !left.is_string_like() {
        return true;
    }
    if right.is_integer_like() || right.is_float_like() {
        return true;
    }
    if !right.is_undef() && !right.is_string_like() {
        return true;
    }

    let left_ok = !left.is_undef();
    let right_ok = !right.is_undef();
    let left_pok = left.is_string_like();
    let left_pv = left.as_str_or_empty();
    let right_pv = right.as_str_or_empty();

    let left_n = perl_str_looks_like_number_for_range(&left_pv);
    let right_n = perl_str_looks_like_number_for_range(&right_pv);

    let left_zero_prefix =
        left_pok && left_pv.len() > 1 && left_pv.as_bytes().first() == Some(&b'0');

    let clause5_left =
        (!left_ok && right_ok) || ((!left_ok || left_n) && left_pok && !left_zero_prefix);
    clause5_left && (!right_ok || right_n)
}

/// Magical string `++` for ASCII letter/digit runs (Perl `sv_inc_nomg`, non-EBCDIC).
pub(crate) fn perl_magic_string_increment_for_range(s: &mut String) -> PerlListRangeIncOutcome {
    if s.is_empty() {
        return PerlListRangeIncOutcome::BecameNumeric;
    }
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() {
        let n = parse_number(s) + 1.0;
        *s = format_float(n);
        return PerlListRangeIncOutcome::BecameNumeric;
    }

    let bytes = unsafe { s.as_mut_vec() };
    let mut idx = bytes.len() - 1;
    loop {
        if bytes[idx].is_ascii_digit() {
            bytes[idx] += 1;
            if bytes[idx] <= b'9' {
                return PerlListRangeIncOutcome::Continue;
            }
            bytes[idx] = b'0';
            if idx == 0 {
                bytes.insert(0, b'1');
                return PerlListRangeIncOutcome::Continue;
            }
            idx -= 1;
        } else {
            bytes[idx] = bytes[idx].wrapping_add(1);
            if bytes[idx].is_ascii_alphabetic() {
                return PerlListRangeIncOutcome::Continue;
            }
            bytes[idx] = bytes[idx].wrapping_sub(b'z' - b'a' + 1);
            if idx == 0 {
                let c = bytes[0];
                bytes.insert(0, if c.is_ascii_digit() { b'1' } else { c });
                return PerlListRangeIncOutcome::Continue;
            }
            idx -= 1;
        }
    }
}

/// Magical string `--` for ASCII letter/digit runs (stryke extension — Perl doesn't have this).
/// Returns `None` if we've hit the floor (e.g., "a" can't decrement, "aa" → "z").
pub(crate) fn perl_magic_string_decrement_for_range(s: &mut String) -> Option<()> {
    if s.is_empty() {
        return None;
    }
    // Validate: must be all alpha then all digit (like increment)
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() {
        return None; // Not a pure alpha/digit string
    }

    let bytes = unsafe { s.as_mut_vec() };
    let mut idx = bytes.len() - 1;
    loop {
        if bytes[idx].is_ascii_digit() {
            if bytes[idx] > b'0' {
                bytes[idx] -= 1;
                return Some(());
            }
            // Borrow: '0' becomes '9', continue to next position
            bytes[idx] = b'9';
            if idx == 0 {
                // "0" → can't go lower, or "00" → "9" (shrink)
                if bytes.len() == 1 {
                    bytes[0] = b'0'; // restore, signal floor
                    return None;
                }
                bytes.remove(0);
                return Some(());
            }
            idx -= 1;
        } else if bytes[idx].is_ascii_lowercase() {
            if bytes[idx] > b'a' {
                bytes[idx] -= 1;
                return Some(());
            }
            // Borrow: 'a' becomes 'z', continue to next position
            bytes[idx] = b'z';
            if idx == 0 {
                // "a" can't decrement, "aa" → "z"
                if bytes.len() == 1 {
                    bytes[0] = b'a'; // restore
                    return None;
                }
                bytes.remove(0);
                return Some(());
            }
            idx -= 1;
        } else if bytes[idx].is_ascii_uppercase() {
            if bytes[idx] > b'A' {
                bytes[idx] -= 1;
                return Some(());
            }
            // Borrow: 'A' becomes 'Z', continue to next position
            bytes[idx] = b'Z';
            if idx == 0 {
                if bytes.len() == 1 {
                    bytes[0] = b'A'; // restore
                    return None;
                }
                bytes.remove(0);
                return Some(());
            }
            idx -= 1;
        } else {
            return None;
        }
    }
}

fn perl_list_range_max_bound(right: &str) -> usize {
    if right.is_ascii() {
        right.len()
    } else {
        right.chars().count()
    }
}

fn perl_list_range_cur_bound(cur: &str, right_is_ascii: bool) -> usize {
    if right_is_ascii {
        cur.len()
    } else {
        cur.chars().count()
    }
}

fn perl_list_range_expand_string_magic(from: StrykeValue, to: StrykeValue) -> Vec<StrykeValue> {
    let mut cur = from.into_string();
    let right = to.into_string();
    let right_ascii = right.is_ascii();
    let max_bound = perl_list_range_max_bound(&right);
    let mut out = Vec::new();
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 50_000_000 {
            break;
        }
        let cur_bound = perl_list_range_cur_bound(&cur, right_ascii);
        if cur_bound > max_bound {
            break;
        }
        out.push(StrykeValue::string(cur.clone()));
        if cur == right {
            break;
        }
        match perl_magic_string_increment_for_range(&mut cur) {
            PerlListRangeIncOutcome::Continue => {}
            PerlListRangeIncOutcome::BecameNumeric => break,
        }
    }
    out
}

/// Perl list-context `..` (`pp_flop`): numeric counting or magical string sequence.
pub(crate) fn perl_list_range_expand(from: StrykeValue, to: StrykeValue) -> Vec<StrykeValue> {
    if perl_list_range_pair_is_numeric(&from, &to) {
        let i = from.to_int();
        let j = to.to_int();
        if j >= i {
            (i..=j).map(StrykeValue::integer).collect()
        } else {
            Vec::new()
        }
    } else {
        perl_list_range_expand_string_magic(from, to)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Polymorphic range types — stryke extension (world first!)
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if string is a valid Roman numeral.
fn is_roman_numeral(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let upper = s.to_ascii_uppercase();
    upper
        .chars()
        .all(|c| matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
}

/// Check if string is an IPv4 address.
fn is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok())
}

/// Parse IPv4 to u32.
fn ipv4_to_u32(s: &str) -> Option<u32> {
    let parts: Vec<u8> = s.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 4 {
        return None;
    }
    Some(
        ((parts[0] as u32) << 24)
            | ((parts[1] as u32) << 16)
            | ((parts[2] as u32) << 8)
            | (parts[3] as u32),
    )
}

/// Convert u32 to IPv4 string.
fn u32_to_ipv4(n: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (n >> 24) & 0xFF,
        (n >> 16) & 0xFF,
        (n >> 8) & 0xFF,
        n & 0xFF
    )
}

/// IPv4 range with step.
fn ipv4_range_stepped(from: &str, to: &str, step: i64) -> Vec<StrykeValue> {
    let Some(start) = ipv4_to_u32(from) else {
        return vec![];
    };
    let Some(end) = ipv4_to_u32(to) else {
        return vec![];
    };
    let mut out = Vec::new();
    if step > 0 {
        let mut cur = start as i64;
        while cur <= end as i64 {
            out.push(StrykeValue::string(u32_to_ipv4(cur as u32)));
            cur += step;
        }
    } else {
        let mut cur = start as i64;
        while cur >= end as i64 {
            out.push(StrykeValue::string(u32_to_ipv4(cur as u32)));
            cur += step;
        }
    }
    out
}

/// Check if string is a valid IPv6 address. Uses Rust's parser so all
/// compressed (`::`), full (8-group), and IPv4-mapped forms are accepted.
fn is_ipv6(s: &str) -> bool {
    s.parse::<std::net::Ipv6Addr>().is_ok()
}

/// Check if string is a `0x…` / `0X…` hex literal in source-form. Used by
/// the range op to keep `0x00:0xFF:1` iterating as hex strings instead of
/// decimal. Returns true only when the prefix is present and the body is
/// non-empty hex digits.
fn is_hex_source_literal(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() > 2
        && bytes[0] == b'0'
        && (bytes[1] == b'x' || bytes[1] == b'X')
        && bytes[2..].iter().all(|b| b.is_ascii_hexdigit())
}

/// Iterate a hex range with step. Output values preserve:
/// - The `0x` / `0X` prefix from the FROM endpoint.
/// - The minimum digit width to fit either endpoint (zero-padded to that).
/// - Uppercase iff EITHER endpoint had any uppercase letter — once the user
///   types `0xFF` we keep the case for every value in the range, even when
///   the FROM endpoint (`0x00`) had no letters of its own to disambiguate.
fn hex_range_stepped(from: &str, to: &str, step: i64) -> Vec<StrykeValue> {
    let from_body = &from[2..];
    let to_body = &to[2..];
    let Ok(start) = i64::from_str_radix(from_body, 16) else {
        return vec![];
    };
    let Ok(end) = i64::from_str_radix(to_body, 16) else {
        return vec![];
    };
    let prefix = &from[..2];
    let width = from_body.len().max(to_body.len());
    let upper = from_body.bytes().any(|b| b.is_ascii_uppercase())
        || to_body.bytes().any(|b| b.is_ascii_uppercase());
    let mut out = Vec::new();
    let format_one = |n: i64, width: usize, upper: bool, prefix: &str| -> String {
        if upper {
            format!("{}{:0>w$X}", prefix, n, w = width)
        } else {
            format!("{}{:0>w$x}", prefix, n, w = width)
        }
    };
    if step > 0 {
        if start > end {
            return out;
        }
        let mut cur = start;
        while cur <= end {
            out.push(StrykeValue::string(format_one(cur, width, upper, prefix)));
            if (end - cur) < step {
                break;
            }
            cur += step;
        }
    } else if step < 0 {
        if start < end {
            return out;
        }
        let mut cur = start;
        while cur >= end {
            out.push(StrykeValue::string(format_one(cur, width, upper, prefix)));
            if (cur - end) < (-step) {
                break;
            }
            cur += step;
        }
    }
    out
}

fn ipv6_range_stepped(from: &str, to: &str, step: i64) -> Vec<StrykeValue> {
    let Ok(start) = from.parse::<std::net::Ipv6Addr>() else {
        return vec![];
    };
    let Ok(end) = to.parse::<std::net::Ipv6Addr>() else {
        return vec![];
    };
    let s = u128::from(start);
    let e = u128::from(end);
    let mut out = Vec::new();
    if step > 0 {
        if s > e {
            return out; // start past end with positive step → empty
        }
        let step = step as u128;
        let mut cur = s;
        loop {
            out.push(StrykeValue::string(
                std::net::Ipv6Addr::from(cur).to_string(),
            ));
            if cur == e || e.saturating_sub(cur) < step {
                break;
            }
            cur += step;
        }
    } else if step < 0 {
        if s < e {
            return out; // start before end with negative step → empty
        }
        let step = (-step) as u128;
        let mut cur = s;
        loop {
            out.push(StrykeValue::string(
                std::net::Ipv6Addr::from(cur).to_string(),
            ));
            if cur == e || cur.saturating_sub(e) < step {
                break;
            }
            cur -= step;
        }
    }
    out
}

/// Check if string is ISO date YYYY-MM-DD.
fn is_iso_date(s: &str) -> bool {
    if s.len() != 10 {
        return false;
    }
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 3
        && parts[0].len() == 4
        && parts[0].parse::<u16>().is_ok()
        && parts[1].len() == 2
        && parts[1]
            .parse::<u8>()
            .map(|m| (1..=12).contains(&m))
            .unwrap_or(false)
        && parts[2].len() == 2
        && parts[2]
            .parse::<u8>()
            .map(|d| (1..=31).contains(&d))
            .unwrap_or(false)
}

/// Check if string is YYYY-MM (month range).
fn is_year_month(s: &str) -> bool {
    if s.len() != 7 {
        return false;
    }
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 2
        && parts[0].len() == 4
        && parts[0].parse::<u16>().is_ok()
        && parts[1].len() == 2
        && parts[1]
            .parse::<u8>()
            .map(|m| (1..=12).contains(&m))
            .unwrap_or(false)
}

/// Parse ISO date to (year, month, day).
fn parse_iso_date(s: &str) -> Option<(i32, u32, u32)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// Parse YYYY-MM to (year, month).
fn parse_year_month(s: &str) -> Option<(i32, u32)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0].parse().ok()?, parts[1].parse().ok()?))
}

/// Days in month (handles leap years).
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Add days to a date, returning new (year, month, day).
fn add_days(mut year: i32, mut month: u32, mut day: u32, mut delta: i64) -> (i32, u32, u32) {
    if delta > 0 {
        while delta > 0 {
            let dim = days_in_month(year, month);
            let remaining = dim - day;
            if delta <= remaining as i64 {
                day += delta as u32;
                break;
            }
            delta -= (remaining + 1) as i64;
            day = 1;
            month += 1;
            if month > 12 {
                month = 1;
                year += 1;
            }
        }
    } else {
        while delta < 0 {
            if (-delta) < day as i64 {
                day = (day as i64 + delta) as u32;
                break;
            }
            delta += day as i64;
            month -= 1;
            if month == 0 {
                month = 12;
                year -= 1;
            }
            day = days_in_month(year, month);
        }
    }
    (year, month, day)
}

/// ISO date range with step (step = days).
fn iso_date_range_stepped(from: &str, to: &str, step: i64) -> Vec<StrykeValue> {
    let Some((mut y, mut m, mut d)) = parse_iso_date(from) else {
        return vec![];
    };
    let Some((ey, em, ed)) = parse_iso_date(to) else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut guard = 0;
    if step > 0 {
        while (y, m, d) <= (ey, em, ed) && guard < 50_000 {
            out.push(StrykeValue::string(format!("{:04}-{:02}-{:02}", y, m, d)));
            (y, m, d) = add_days(y, m, d, step);
            guard += 1;
        }
    } else {
        while (y, m, d) >= (ey, em, ed) && guard < 50_000 {
            out.push(StrykeValue::string(format!("{:04}-{:02}-{:02}", y, m, d)));
            (y, m, d) = add_days(y, m, d, step);
            guard += 1;
        }
    }
    out
}

/// Add months to (year, month).
fn add_months(mut year: i32, mut month: u32, delta: i64) -> (i32, u32) {
    let total = (year as i64 * 12 + month as i64 - 1) + delta;
    year = (total / 12) as i32;
    month = ((total % 12) + 1) as u32;
    if month == 0 {
        month = 12;
        year -= 1;
    }
    (year, month)
}

/// YYYY-MM range with step (step = months).
fn year_month_range_stepped(from: &str, to: &str, step: i64) -> Vec<StrykeValue> {
    let Some((mut y, mut m)) = parse_year_month(from) else {
        return vec![];
    };
    let Some((ey, em)) = parse_year_month(to) else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut guard = 0;
    if step > 0 {
        while (y, m) <= (ey, em) && guard < 50_000 {
            out.push(StrykeValue::string(format!("{:04}-{:02}", y, m)));
            (y, m) = add_months(y, m, step);
            guard += 1;
        }
    } else {
        while (y, m) >= (ey, em) && guard < 50_000 {
            out.push(StrykeValue::string(format!("{:04}-{:02}", y, m)));
            (y, m) = add_months(y, m, step);
            guard += 1;
        }
    }
    out
}

/// Check if string looks like HH:MM time.
fn is_time_hhmm(s: &str) -> bool {
    if s.len() != 5 {
        return false;
    }
    let parts: Vec<&str> = s.split(':').collect();
    parts.len() == 2
        && parts[0].len() == 2
        && parts[0].parse::<u8>().map(|h| h < 24).unwrap_or(false)
        && parts[1].len() == 2
        && parts[1].parse::<u8>().map(|m| m < 60).unwrap_or(false)
}

/// Parse HH:MM to minutes since midnight.
fn parse_time_hhmm(s: &str) -> Option<i32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h: i32 = parts[0].parse().ok()?;
    let m: i32 = parts[1].parse().ok()?;
    Some(h * 60 + m)
}

/// Minutes to HH:MM string.
fn minutes_to_hhmm(mins: i32) -> String {
    let h = (mins / 60) % 24;
    let m = mins % 60;
    format!("{:02}:{:02}", h, m)
}

/// HH:MM time range with step (step = minutes).
fn time_range_stepped(from: &str, to: &str, step: i64) -> Vec<StrykeValue> {
    let Some(start) = parse_time_hhmm(from) else {
        return vec![];
    };
    let Some(end) = parse_time_hhmm(to) else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut guard = 0;
    if step > 0 {
        let mut cur = start;
        while cur <= end && guard < 50_000 {
            out.push(StrykeValue::string(minutes_to_hhmm(cur)));
            cur += step as i32;
            guard += 1;
        }
    } else {
        let mut cur = start;
        while cur >= end && guard < 50_000 {
            out.push(StrykeValue::string(minutes_to_hhmm(cur)));
            cur += step as i32;
            guard += 1;
        }
    }
    out
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_FULL: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Check if string is a weekday name.
fn weekday_index(s: &str) -> Option<usize> {
    let lower = s.to_ascii_lowercase();
    for (i, &d) in WEEKDAYS.iter().enumerate() {
        if d.to_ascii_lowercase() == lower {
            return Some(i);
        }
    }
    for (i, &d) in WEEKDAYS_FULL.iter().enumerate() {
        if d.to_ascii_lowercase() == lower {
            return Some(i);
        }
    }
    None
}

/// Weekday range with step.
fn weekday_range_stepped(from: &str, to: &str, step: i64) -> Vec<StrykeValue> {
    let Some(start) = weekday_index(from) else {
        return vec![];
    };
    let Some(end) = weekday_index(to) else {
        return vec![];
    };
    let full = from.len() > 3;
    let names = if full { &WEEKDAYS_FULL } else { &WEEKDAYS };
    let mut out = Vec::new();
    if step > 0 {
        let mut cur = start as i64;
        let target = if end >= start {
            end as i64
        } else {
            end as i64 + 7
        };
        while cur <= target {
            out.push(StrykeValue::string(names[(cur % 7) as usize].to_string()));
            cur += step;
        }
    } else {
        let mut cur = start as i64;
        let target = if end <= start {
            end as i64
        } else {
            end as i64 - 7
        };
        while cur >= target {
            out.push(StrykeValue::string(
                names[((cur % 7 + 7) % 7) as usize].to_string(),
            ));
            cur += step;
        }
    }
    out
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Check if string is a month name.
fn month_name_index(s: &str) -> Option<usize> {
    let lower = s.to_ascii_lowercase();
    for (i, &m) in MONTHS.iter().enumerate() {
        if m.to_ascii_lowercase() == lower {
            return Some(i);
        }
    }
    for (i, &m) in MONTHS_FULL.iter().enumerate() {
        if m.to_ascii_lowercase() == lower {
            return Some(i);
        }
    }
    None
}

/// Month name range with step.
fn month_name_range_stepped(from: &str, to: &str, step: i64) -> Vec<StrykeValue> {
    let Some(start) = month_name_index(from) else {
        return vec![];
    };
    let Some(end) = month_name_index(to) else {
        return vec![];
    };
    let full = from.len() > 3;
    let names = if full { &MONTHS_FULL } else { &MONTHS };
    let mut out = Vec::new();
    if step > 0 {
        let mut cur = start as i64;
        let target = if end >= start {
            end as i64
        } else {
            end as i64 + 12
        };
        while cur <= target {
            out.push(StrykeValue::string(names[(cur % 12) as usize].to_string()));
            cur += step;
        }
    } else {
        let mut cur = start as i64;
        let target = if end <= start {
            end as i64
        } else {
            end as i64 - 12
        };
        while cur >= target {
            out.push(StrykeValue::string(
                names[((cur % 12 + 12) % 12) as usize].to_string(),
            ));
            cur += step;
        }
    }
    out
}

/// Check if both operands are float-like (contain decimal point, not date/time/IP).
fn is_float_pair(from: &str, to: &str) -> bool {
    fn is_float(s: &str) -> bool {
        s.contains('.')
            && !s.contains(':')
            && s.matches('.').count() == 1
            && s.parse::<f64>().is_ok()
    }
    is_float(from) && is_float(to)
}

/// Float range with step.
fn float_range_stepped(from: &str, to: &str, step: f64) -> Vec<StrykeValue> {
    let Ok(start) = from.parse::<f64>() else {
        return vec![];
    };
    let Ok(end) = to.parse::<f64>() else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut guard = 0;
    // Use integer counting to avoid floating point accumulation errors
    if step > 0.0 {
        let mut i = 0i64;
        loop {
            let cur = start + (i as f64) * step;
            if cur > end + step.abs() * f64::EPSILON * 10.0 || guard >= 50_000 {
                break;
            }
            // Round to avoid floating point noise
            let rounded = (cur * 1e12).round() / 1e12;
            out.push(StrykeValue::float(rounded));
            i += 1;
            guard += 1;
        }
    } else if step < 0.0 {
        let mut i = 0i64;
        loop {
            let cur = start + (i as f64) * step;
            if cur < end - step.abs() * f64::EPSILON * 10.0 || guard >= 50_000 {
                break;
            }
            let rounded = (cur * 1e12).round() / 1e12;
            out.push(StrykeValue::float(rounded));
            i += 1;
            guard += 1;
        }
    }
    out
}

/// Convert Roman numeral string to integer.
fn roman_to_int(s: &str) -> Option<i64> {
    let upper = s.to_ascii_uppercase();
    let mut result = 0i64;
    let mut prev = 0i64;
    for c in upper.chars().rev() {
        let val = match c {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            'D' => 500,
            'M' => 1000,
            _ => return None,
        };
        if val < prev {
            result -= val;
        } else {
            result += val;
        }
        prev = val;
    }
    if result > 0 {
        Some(result)
    } else {
        None
    }
}

/// Convert integer to Roman numeral string.
fn int_to_roman(mut n: i64, lowercase: bool) -> Option<String> {
    if n <= 0 || n > 3999 {
        return None;
    }
    let numerals = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut result = String::new();
    for (val, sym) in numerals {
        while n >= val {
            result.push_str(sym);
            n -= val;
        }
    }
    if lowercase {
        Some(result.to_ascii_lowercase())
    } else {
        Some(result)
    }
}

/// Expand a Roman numeral range with step.
fn roman_range_stepped(from: &str, to: &str, step: i64) -> Vec<StrykeValue> {
    let Some(start) = roman_to_int(from) else {
        return vec![];
    };
    let Some(end) = roman_to_int(to) else {
        return vec![];
    };
    let lowercase = from
        .chars()
        .next()
        .map(|c| c.is_ascii_lowercase())
        .unwrap_or(false);

    let mut out = Vec::new();
    if step > 0 {
        let mut cur = start;
        while cur <= end {
            if let Some(r) = int_to_roman(cur, lowercase) {
                out.push(StrykeValue::string(r));
            }
            cur += step;
        }
    } else {
        let mut cur = start;
        while cur >= end {
            if let Some(r) = int_to_roman(cur, lowercase) {
                out.push(StrykeValue::string(r));
            }
            cur += step; // step is negative
        }
    }
    out
}

/// Stepped range expansion — polymorphic across many types (stryke world first!).
/// Supports: integers, floats, strings, Roman numerals, dates, times, weekdays, months, IPv4.
pub(crate) fn perl_list_range_expand_stepped(
    from: StrykeValue,
    to: StrykeValue,
    step_val: StrykeValue,
) -> Vec<StrykeValue> {
    let from_str = from.to_string();
    let to_str = to.to_string();

    // Check if this is a float range (operands have decimal points)
    let is_float_range = is_float_pair(&from_str, &to_str);

    // Get step as float or int depending on context
    let step_float = step_val.as_float().unwrap_or(step_val.to_int() as f64);
    let step_int = step_val.to_int();

    if step_int == 0 && step_float == 0.0 {
        return vec![];
    }

    // Float ranges use float step
    if is_float_range {
        return float_range_stepped(&from_str, &to_str, step_float);
    }

    // Pure numeric integers
    if perl_list_range_pair_is_numeric(&from, &to) {
        let i = from.to_int();
        let j = to.to_int();
        if step_int > 0 {
            (i..=j)
                .step_by(step_int as usize)
                .map(StrykeValue::integer)
                .collect()
        } else {
            std::iter::successors(Some(i), |&x| {
                let next = x + step_int;
                if next >= j {
                    Some(next)
                } else {
                    None
                }
            })
            .map(StrykeValue::integer)
            .collect()
        }
    } else {
        // Check special types in order of specificity

        // Hex literals — must check before IPv4 because `0xFF` chars include
        // hex digits that aren't dotted-quad anyway, but keeping ordering
        // tight prevents future ambiguity. Preserves `0x` prefix, width,
        // and case from the source form.
        if is_hex_source_literal(&from_str) && is_hex_source_literal(&to_str) {
            return hex_range_stepped(&from_str, &to_str, step_int);
        }

        // IPv4 addresses (must check before floats due to dots)
        if is_ipv4(&from_str) && is_ipv4(&to_str) {
            return ipv4_range_stepped(&from_str, &to_str, step_int);
        }

        // IPv6 addresses — full or `::`-compressed. Uses the dedicated `!!!`
        // range separator so the IPv6's own colons don't collide with the
        // standard `:` range op.
        if is_ipv6(&from_str) && is_ipv6(&to_str) {
            return ipv6_range_stepped(&from_str, &to_str, step_int);
        }

        // ISO dates YYYY-MM-DD (step = days)
        if is_iso_date(&from_str) && is_iso_date(&to_str) {
            return iso_date_range_stepped(&from_str, &to_str, step_int);
        }

        // Year-month YYYY-MM (step = months)
        if is_year_month(&from_str) && is_year_month(&to_str) {
            return year_month_range_stepped(&from_str, &to_str, step_int);
        }

        // Time HH:MM (step = minutes)
        if is_time_hhmm(&from_str) && is_time_hhmm(&to_str) {
            return time_range_stepped(&from_str, &to_str, step_int);
        }

        // Weekday names
        if weekday_index(&from_str).is_some() && weekday_index(&to_str).is_some() {
            return weekday_range_stepped(&from_str, &to_str, step_int);
        }

        // Month names
        if month_name_index(&from_str).is_some() && month_name_index(&to_str).is_some() {
            return month_name_range_stepped(&from_str, &to_str, step_int);
        }

        // Roman numerals
        if is_roman_numeral(&from_str) && is_roman_numeral(&to_str) {
            return roman_range_stepped(&from_str, &to_str, step_int);
        }

        // Fall back to magic string increment/decrement
        perl_list_range_expand_string_magic_stepped(from, to, step_int)
    }
}

/// Coerce a slice endpoint to a strict integer. Used by [`Op::ArraySliceRange`] —
/// non-numeric strings, fractional floats, refs, and other non-integer types die.
/// `where_` is the diagnostic context (`"start"`, `"stop"`, `"step"`).
pub(crate) fn perl_slice_endpoint_to_strict_int(
    v: &StrykeValue,
    where_: &str,
) -> Result<i64, String> {
    if let Some(n) = v.as_integer() {
        return Ok(n);
    }
    if let Some(f) = v.as_float() {
        if f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
            return Ok(f as i64);
        }
        return Err(format!(
            "array slice {}: non-integer float endpoint {}",
            where_, f
        ));
    }
    let s = v.as_str_or_empty();
    if !s.is_empty() {
        if let Ok(n) = s.trim().parse::<i64>() {
            return Ok(n);
        }
        return Err(format!(
            "array slice {}: non-integer string endpoint {:?}",
            where_, s
        ));
    }
    Err(format!(
        "array slice {}: endpoint must be an integer (got non-numeric value)",
        where_
    ))
}

/// Resolve `from`/`to`/`step` for `@arr[FROM:TO:STEP]` (and open-ended forms) into the
/// concrete list of array indices. Closed inclusive on both ends. `Undef` endpoints
/// (the omitted-endpoint sentinel emitted by the compiler) default to:
/// - `step` → `1`
/// - `from` → `0` (positive step) or `arr_len-1` (negative step)
/// - `to`   → `arr_len-1` (positive step) or `0` (negative step)
///
/// Negative explicit indices count from the end (Perl semantics: `-1` = last element).
/// Returns `Err(msg)` for non-integer endpoints or zero step — caller dies with that.
pub(crate) fn compute_array_slice_indices(
    arr_len: i64,
    from: &StrykeValue,
    to: &StrykeValue,
    step: &StrykeValue,
) -> Result<Vec<i64>, String> {
    let step_i = if step.is_undef() {
        1i64
    } else {
        perl_slice_endpoint_to_strict_int(step, "step")?
    };
    if step_i == 0 {
        return Err("array slice step cannot be 0".into());
    }

    let normalize = |i: i64| -> i64 {
        if i < 0 {
            i + arr_len
        } else {
            i
        }
    };

    // Open-ended slice (`@a[..3]`, `@a[-3..]`) is a stryke extension where each
    // explicit endpoint wraps once from the end. Closed `Range` slices
    // (`@a[0..-1]`, `@a[3..-1]`, `@a[-3..-1]`) follow Perl's raw-integer range
    // semantics: `0..-1` is empty, `-3..-1` is `(-3, -2, -1)`, and each
    // generated integer wraps individually when looked up.
    let any_undef = from.is_undef() || to.is_undef();

    let from_raw = if from.is_undef() {
        if step_i > 0 {
            0
        } else {
            arr_len - 1
        }
    } else {
        perl_slice_endpoint_to_strict_int(from, "start")?
    };

    let to_raw = if to.is_undef() {
        if step_i > 0 {
            arr_len - 1
        } else {
            0
        }
    } else {
        perl_slice_endpoint_to_strict_int(to, "stop")?
    };

    let mut out = Vec::new();
    if arr_len == 0 {
        return Ok(out);
    }

    let (from_i, to_i) = if any_undef {
        (normalize(from_raw), normalize(to_raw))
    } else {
        (from_raw, to_raw)
    };

    if step_i > 0 {
        let mut i = from_i;
        while i <= to_i {
            out.push(if any_undef { i } else { normalize(i) });
            i += step_i;
        }
    } else {
        let mut i = from_i;
        while i >= to_i {
            out.push(if any_undef { i } else { normalize(i) });
            i += step_i; // step_i is negative
        }
    }
    Ok(out)
}

/// Resolve `from`/`to`/`step` for `@h{FROM:TO:STEP}` into the concrete list of hash keys.
/// Both endpoints must be present (open-ended forms are nonsense for unordered hashes
/// and die). Endpoints stringify to keys; expansion uses the polymorphic stepped-range
/// machinery (numeric, magic-string-increment, Roman, etc.).
pub(crate) fn compute_hash_slice_keys(
    from: &StrykeValue,
    to: &StrykeValue,
    step: &StrykeValue,
) -> Result<Vec<String>, String> {
    if from.is_undef() || to.is_undef() {
        return Err(
            "hash slice range requires both endpoints (open-ended forms not allowed)".into(),
        );
    }
    let step_val = if step.is_undef() {
        StrykeValue::integer(1)
    } else {
        step.clone()
    };
    let expanded = perl_list_range_expand_stepped(from.clone(), to.clone(), step_val);
    Ok(expanded.into_iter().map(|v| v.to_string()).collect())
}

fn perl_list_range_expand_string_magic_stepped(
    from: StrykeValue,
    to: StrykeValue,
    step: i64,
) -> Vec<StrykeValue> {
    if step == 0 {
        return vec![];
    }
    let mut cur = from.into_string();
    let right = to.into_string();

    if step > 0 {
        // Forward iteration
        let step = step as usize;
        let right_ascii = right.is_ascii();
        let max_bound = perl_list_range_max_bound(&right);
        let mut out = Vec::new();
        let mut guard = 0usize;
        let mut idx = 0usize;
        loop {
            guard += 1;
            if guard > 50_000_000 {
                break;
            }
            let cur_bound = perl_list_range_cur_bound(&cur, right_ascii);
            if cur_bound > max_bound {
                break;
            }
            if idx.is_multiple_of(step) {
                out.push(StrykeValue::string(cur.clone()));
            }
            if cur == right {
                break;
            }
            match perl_magic_string_increment_for_range(&mut cur) {
                PerlListRangeIncOutcome::Continue => {}
                PerlListRangeIncOutcome::BecameNumeric => break,
            }
            idx += 1;
        }
        out
    } else {
        // Reverse iteration (stryke extension)
        let step = (-step) as usize;
        let mut out = Vec::new();
        let mut guard = 0usize;
        let mut idx = 0usize;
        loop {
            guard += 1;
            if guard > 50_000_000 {
                break;
            }
            if idx.is_multiple_of(step) {
                out.push(StrykeValue::string(cur.clone()));
            }
            if cur == right {
                break;
            }
            // Check if we've gone past the target (cur < right lexicographically)
            if cur < right {
                break;
            }
            match perl_magic_string_decrement_for_range(&mut cur) {
                Some(()) => {}
                None => break, // Hit floor
            }
            idx += 1;
        }
        out
    }
}

impl PerlDataFrame {
    /// One row as a hashref (`$_` in `filter`).
    pub fn row_hashref(&self, row: usize) -> StrykeValue {
        let mut m = IndexMap::new();
        for (i, col) in self.columns.iter().enumerate() {
            m.insert(
                col.clone(),
                self.cols[i].get(row).cloned().unwrap_or(StrykeValue::UNDEF),
            );
        }
        StrykeValue::hash_ref(Arc::new(RwLock::new(m)))
    }
}

#[cfg(test)]
mod tests {
    use super::StrykeValue;
    use crate::perl_regex::PerlCompiledRegex;
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::cmp::Ordering;
    use std::sync::Arc;

    #[test]
    fn undef_is_false() {
        assert!(!StrykeValue::UNDEF.is_true());
    }

    #[test]
    fn string_zero_is_false() {
        assert!(!StrykeValue::string("0".into()).is_true());
        assert!(StrykeValue::string("00".into()).is_true());
    }

    #[test]
    fn empty_string_is_false() {
        assert!(!StrykeValue::string(String::new()).is_true());
    }

    #[test]
    fn integer_zero_is_false_nonzero_true() {
        assert!(!StrykeValue::integer(0).is_true());
        assert!(StrykeValue::integer(-1).is_true());
    }

    #[test]
    fn float_zero_is_false_nonzero_true() {
        assert!(!StrykeValue::float(0.0).is_true());
        assert!(StrykeValue::float(0.1).is_true());
    }

    #[test]
    fn num_cmp_orders_float_against_integer() {
        assert_eq!(
            StrykeValue::float(2.5).num_cmp(&StrykeValue::integer(3)),
            Ordering::Less
        );
    }

    #[test]
    fn to_int_parses_leading_number_from_string() {
        assert_eq!(StrykeValue::string("42xyz".into()).to_int(), 42);
        assert_eq!(StrykeValue::string("  -3.7foo".into()).to_int(), -3);
    }

    #[test]
    fn num_cmp_orders_as_numeric() {
        assert_eq!(
            StrykeValue::integer(2).num_cmp(&StrykeValue::integer(11)),
            Ordering::Less
        );
        assert_eq!(
            StrykeValue::string("2foo".into()).num_cmp(&StrykeValue::string("11".into())),
            Ordering::Less
        );
    }

    #[test]
    fn str_cmp_orders_as_strings() {
        assert_eq!(
            StrykeValue::string("2".into()).str_cmp(&StrykeValue::string("11".into())),
            Ordering::Greater
        );
    }

    #[test]
    fn str_eq_heap_strings_fast_path() {
        let a = StrykeValue::string("hello".into());
        let b = StrykeValue::string("hello".into());
        assert!(a.str_eq(&b));
        assert!(!a.str_eq(&StrykeValue::string("hell".into())));
    }

    #[test]
    fn str_eq_fallback_matches_stringified_equality() {
        let n = StrykeValue::integer(42);
        let s = StrykeValue::string("42".into());
        assert!(n.str_eq(&s));
        assert!(!StrykeValue::integer(1).str_eq(&StrykeValue::string("2".into())));
    }

    #[test]
    fn str_cmp_heap_strings_fast_path() {
        assert_eq!(
            StrykeValue::string("a".into()).str_cmp(&StrykeValue::string("b".into())),
            Ordering::Less
        );
    }

    #[test]
    fn scalar_context_array_and_hash() {
        let a = StrykeValue::array(vec![StrykeValue::integer(1), StrykeValue::integer(2)])
            .scalar_context();
        assert_eq!(a.to_int(), 2);
        let mut h = IndexMap::new();
        h.insert("a".into(), StrykeValue::integer(1));
        let sc = StrykeValue::hash(h).scalar_context();
        assert!(sc.is_string_like());
    }

    #[test]
    fn to_list_array_hash_and_scalar() {
        assert_eq!(
            StrykeValue::array(vec![StrykeValue::integer(7)])
                .to_list()
                .len(),
            1
        );
        let mut h = IndexMap::new();
        h.insert("k".into(), StrykeValue::integer(1));
        let list = StrykeValue::hash(h).to_list();
        assert_eq!(list.len(), 2);
        let one = StrykeValue::integer(99).to_list();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].to_int(), 99);
    }

    #[test]
    fn type_name_and_ref_type_for_core_kinds() {
        assert_eq!(StrykeValue::integer(0).type_name(), "INTEGER");
        assert_eq!(StrykeValue::UNDEF.ref_type().to_string(), "");
        assert_eq!(
            StrykeValue::array_ref(Arc::new(RwLock::new(vec![])))
                .ref_type()
                .to_string(),
            "ARRAY"
        );
    }

    #[test]
    fn display_undef_is_empty_integer_is_decimal() {
        assert_eq!(StrykeValue::UNDEF.to_string(), "");
        assert_eq!(StrykeValue::integer(-7).to_string(), "-7");
    }

    #[test]
    fn empty_array_is_false_nonempty_is_true() {
        assert!(!StrykeValue::array(vec![]).is_true());
        assert!(StrykeValue::array(vec![StrykeValue::integer(0)]).is_true());
    }

    #[test]
    fn to_number_undef_and_non_numeric_refs_are_zero() {
        use super::StrykeSub;

        assert_eq!(StrykeValue::UNDEF.to_number(), 0.0);
        assert_eq!(
            StrykeValue::code_ref(Arc::new(StrykeSub {
                name: "f".into(),
                params: vec![],
                body: vec![],
                closure_env: None,
                prototype: None,
                fib_like: None,
            }))
            .to_number(),
            0.0
        );
    }

    #[test]
    fn append_to_builds_string_without_extra_alloc_for_int_and_string() {
        let mut buf = String::new();
        StrykeValue::integer(-12).append_to(&mut buf);
        StrykeValue::string("ab".into()).append_to(&mut buf);
        assert_eq!(buf, "-12ab");
        let mut u = String::new();
        StrykeValue::UNDEF.append_to(&mut u);
        assert!(u.is_empty());
    }

    #[test]
    fn append_to_atomic_delegates_to_inner() {
        use parking_lot::Mutex;
        let a = StrykeValue::atomic(Arc::new(Mutex::new(StrykeValue::string("z".into()))));
        let mut buf = String::new();
        a.append_to(&mut buf);
        assert_eq!(buf, "z");
    }

    #[test]
    fn unwrap_atomic_reads_inner_other_variants_clone() {
        use parking_lot::Mutex;
        let a = StrykeValue::atomic(Arc::new(Mutex::new(StrykeValue::integer(9))));
        assert_eq!(a.unwrap_atomic().to_int(), 9);
        assert_eq!(StrykeValue::integer(3).unwrap_atomic().to_int(), 3);
    }

    #[test]
    fn is_atomic_only_true_for_atomic_variant() {
        use parking_lot::Mutex;
        assert!(StrykeValue::atomic(Arc::new(Mutex::new(StrykeValue::UNDEF))).is_atomic());
        assert!(!StrykeValue::integer(0).is_atomic());
    }

    #[test]
    fn as_str_only_on_string_variant() {
        assert_eq!(
            StrykeValue::string("x".into()).as_str(),
            Some("x".to_string())
        );
        assert_eq!(StrykeValue::integer(1).as_str(), None);
    }

    #[test]
    fn as_str_or_empty_defaults_non_string() {
        assert_eq!(StrykeValue::string("z".into()).as_str_or_empty(), "z");
        assert_eq!(StrykeValue::integer(1).as_str_or_empty(), "");
    }

    #[test]
    fn to_int_truncates_float_toward_zero() {
        assert_eq!(StrykeValue::float(3.9).to_int(), 3);
        assert_eq!(StrykeValue::float(-2.1).to_int(), -2);
    }

    #[test]
    fn to_number_array_is_length() {
        assert_eq!(
            StrykeValue::array(vec![StrykeValue::integer(1), StrykeValue::integer(2)]).to_number(),
            2.0
        );
    }

    #[test]
    fn scalar_context_empty_hash_is_zero() {
        let h = IndexMap::new();
        assert_eq!(StrykeValue::hash(h).scalar_context().to_int(), 0);
    }

    #[test]
    fn scalar_context_nonhash_nonarray_clones() {
        let v = StrykeValue::integer(8);
        assert_eq!(v.scalar_context().to_int(), 8);
    }

    #[test]
    fn display_float_integer_like_omits_decimal() {
        assert_eq!(StrykeValue::float(4.0).to_string(), "4");
    }

    #[test]
    fn display_array_concatenates_element_displays() {
        let a = StrykeValue::array(vec![
            StrykeValue::integer(1),
            StrykeValue::string("b".into()),
        ]);
        assert_eq!(a.to_string(), "1b");
    }

    #[test]
    fn display_code_ref_is_perl_style_hex_address() {
        // Per BUG-245, coderefs stringify as `CODE(0x<hexaddr>)` so distinct
        // closures produce distinct strings (matches Perl's documented form).
        use super::StrykeSub;
        let c = StrykeValue::code_ref(Arc::new(StrykeSub {
            name: "foo".into(),
            params: vec![],
            body: vec![],
            closure_env: None,
            prototype: None,
            fib_like: None,
        }));
        let s = c.to_string();
        assert!(s.starts_with("CODE(0x"), "got {:?}", s);
        assert!(s.ends_with(')'), "got {:?}", s);
    }

    #[test]
    fn display_regex_shows_non_capturing_prefix() {
        let r = StrykeValue::regex(
            PerlCompiledRegex::compile("x+").unwrap(),
            "x+".into(),
            "".into(),
        );
        assert_eq!(r.to_string(), "(?:x+)");
    }

    #[test]
    fn display_iohandle_is_name() {
        assert_eq!(
            StrykeValue::io_handle("STDOUT".into()).to_string(),
            "STDOUT"
        );
    }

    #[test]
    fn ref_type_blessed_uses_class_name() {
        let b = StrykeValue::blessed(Arc::new(super::BlessedRef::new_blessed(
            "Pkg".into(),
            StrykeValue::UNDEF,
        )));
        assert_eq!(b.ref_type().to_string(), "Pkg");
    }

    #[test]
    fn blessed_drop_enqueues_pending_destroy() {
        let v = StrykeValue::blessed(Arc::new(super::BlessedRef::new_blessed(
            "Z".into(),
            StrykeValue::integer(7),
        )));
        drop(v);
        let q = crate::pending_destroy::take_queue();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].0, "Z");
        assert_eq!(q[0].1.to_int(), 7);
    }

    #[test]
    fn type_name_iohandle_is_glob() {
        assert_eq!(StrykeValue::io_handle("FH".into()).type_name(), "GLOB");
    }

    #[test]
    fn empty_hash_is_false() {
        assert!(!StrykeValue::hash(IndexMap::new()).is_true());
    }

    #[test]
    fn hash_nonempty_is_true() {
        let mut h = IndexMap::new();
        h.insert("k".into(), StrykeValue::UNDEF);
        assert!(StrykeValue::hash(h).is_true());
    }

    #[test]
    fn num_cmp_equal_integers() {
        assert_eq!(
            StrykeValue::integer(5).num_cmp(&StrykeValue::integer(5)),
            Ordering::Equal
        );
    }

    #[test]
    fn str_cmp_compares_lexicographic_string_forms() {
        // Display forms "2" and "10" — string order differs from numeric order.
        assert_eq!(
            StrykeValue::integer(2).str_cmp(&StrykeValue::integer(10)),
            Ordering::Greater
        );
    }

    #[test]
    fn to_list_undef_empty() {
        assert!(StrykeValue::UNDEF.to_list().is_empty());
    }

    #[test]
    fn unwrap_atomic_nested_atomic() {
        use parking_lot::Mutex;
        let inner = StrykeValue::atomic(Arc::new(Mutex::new(StrykeValue::integer(2))));
        let outer = StrykeValue::atomic(Arc::new(Mutex::new(inner)));
        assert_eq!(outer.unwrap_atomic().to_int(), 2);
    }

    #[test]
    fn errno_dual_parts_extracts_code_and_message() {
        let v = StrykeValue::errno_dual(-2, "oops".into());
        assert_eq!(v.errno_dual_parts(), Some((-2, "oops".into())));
    }

    #[test]
    fn errno_dual_parts_none_for_plain_string() {
        assert!(StrykeValue::string("hi".into())
            .errno_dual_parts()
            .is_none());
    }

    #[test]
    fn errno_dual_parts_none_for_integer() {
        assert!(StrykeValue::integer(1).errno_dual_parts().is_none());
    }

    #[test]
    fn errno_dual_numeric_context_uses_code_string_uses_msg() {
        let v = StrykeValue::errno_dual(5, "five".into());
        assert_eq!(v.to_int(), 5);
        assert_eq!(v.to_string(), "five");
    }

    #[test]
    fn list_range_alpha_joins_like_perl() {
        use super::perl_list_range_expand;
        let v = perl_list_range_expand(
            StrykeValue::string("a".into()),
            StrykeValue::string("z".into()),
        );
        let s: String = v.iter().map(|x| x.to_string()).collect();
        assert_eq!(s, "abcdefghijklmnopqrstuvwxyz");
    }

    #[test]
    fn list_range_numeric_string_endpoints() {
        use super::perl_list_range_expand;
        let v = perl_list_range_expand(
            StrykeValue::string("9".into()),
            StrykeValue::string("11".into()),
        );
        assert_eq!(v.len(), 3);
        assert_eq!(
            v.iter().map(|x| x.to_int()).collect::<Vec<_>>(),
            vec![9, 10, 11]
        );
    }

    #[test]
    fn list_range_leading_zero_is_string_mode() {
        use super::perl_list_range_expand;
        let v = perl_list_range_expand(
            StrykeValue::string("01".into()),
            StrykeValue::string("05".into()),
        );
        assert_eq!(v.len(), 5);
        assert_eq!(
            v.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
            vec!["01", "02", "03", "04", "05"]
        );
    }

    #[test]
    fn list_range_empty_to_letter_one_element() {
        use super::perl_list_range_expand;
        let v = perl_list_range_expand(
            StrykeValue::string(String::new()),
            StrykeValue::string("c".into()),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].to_string(), "");
    }

    #[test]
    fn magic_string_inc_z_wraps_aa() {
        use super::{perl_magic_string_increment_for_range, PerlListRangeIncOutcome};
        let mut s = "z".to_string();
        assert_eq!(
            perl_magic_string_increment_for_range(&mut s),
            PerlListRangeIncOutcome::Continue
        );
        assert_eq!(s, "aa");
    }

    #[test]
    fn test_boxed_numeric_stringification() {
        // Large integer outside i32 range
        let large_int = 10_000_000_000i64;
        let v_int = StrykeValue::integer(large_int);
        assert_eq!(v_int.to_string(), "10000000000");

        // Float that needs boxing (e.g. Infinity); Perl prints "Inf".
        let v_inf = StrykeValue::float(f64::INFINITY);
        assert_eq!(v_inf.to_string(), "Inf");
    }

    #[test]
    fn magic_string_inc_nine_to_ten() {
        use super::{perl_magic_string_increment_for_range, PerlListRangeIncOutcome};
        let mut s = "9".to_string();
        assert_eq!(
            perl_magic_string_increment_for_range(&mut s),
            PerlListRangeIncOutcome::Continue
        );
        assert_eq!(s, "10");
    }
}
