use crossbeam::channel::{Receiver, Sender};
use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::sync::Barrier;

use crate::ast::{Block, ClassDef, EnumDef, StructDef, SubSigParam};
use crate::error::PerlResult;
use crate::nanbox;
use crate::perl_decode::decode_utf8_or_latin1;
use crate::perl_regex::PerlCompiledRegex;

/// Handle returned by `async { ... }` / `spawn { ... }`; join with `await`.
#[derive(Debug)]
pub struct PerlAsyncTask {
    pub(crate) result: Arc<Mutex<Option<PerlResult<PerlValue>>>>,
    pub(crate) join: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl Clone for PerlAsyncTask {
    fn clone(&self) -> Self {
        Self {
            result: self.result.clone(),
            join: self.join.clone(),
        }
    }
}

impl PerlAsyncTask {
    /// Join the worker thread (once) and return the block's value or error.
    pub fn await_result(&self) -> PerlResult<PerlValue> {
        if let Some(h) = self.join.lock().take() {
            let _ = h.join();
        }
        self.result
            .lock()
            .clone()
            .unwrap_or_else(|| Ok(PerlValue::UNDEF))
    }
}

// ── Lazy iterator protocol (`|>` streaming) ─────────────────────────────────

/// Pull-based lazy iterator.  Sources (`frs`, `drs`) produce one; transform
/// stages (`rev`) wrap one; terminals (`e`/`fore`) consume one item at a time.
pub trait PerlIterator: Send + Sync {
    /// Return the next item, or `None` when exhausted.
    fn next_item(&self) -> Option<PerlValue>;

    /// Collect all remaining items into a `Vec`.
    fn collect_all(&self) -> Vec<PerlValue> {
        let mut out = Vec::new();
        while let Some(v) = self.next_item() {
            out.push(v);
        }
        out
    }
}

impl fmt::Debug for dyn PerlIterator {
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
    files_only: bool,
}

impl FsWalkIterator {
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

impl PerlIterator for FsWalkIterator {
    fn next_item(&self) -> Option<PerlValue> {
        loop {
            {
                let mut buf = self.buf.lock();
                if let Some((path, _)) = buf.pop() {
                    return Some(PerlValue::string(path));
                }
            }
            if !self.refill() {
                return None;
            }
        }
    }
}

/// Wraps a source iterator, applying `scalar reverse` (char-reverse) to each string.
pub struct RevIterator {
    source: Arc<dyn PerlIterator>,
}

impl RevIterator {
    pub fn new(source: Arc<dyn PerlIterator>) -> Self {
        Self { source }
    }
}

impl PerlIterator for RevIterator {
    fn next_item(&self) -> Option<PerlValue> {
        let item = self.source.next_item()?;
        let s = item.to_string();
        Some(PerlValue::string(s.chars().rev().collect()))
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
pub type PerlSet = IndexMap<String, PerlValue>;

/// Min-heap ordered by a Perl comparator (`$a` / `$b` in scope, like `sort { }`).
#[derive(Debug, Clone)]
pub struct PerlHeap {
    pub items: Vec<PerlValue>,
    pub cmp: Arc<PerlSub>,
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

    fn s(v: &str) -> PerlValue {
        PerlValue::string(v.to_string())
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
        h.insert("slots".to_string(), PerlValue::integer(2));
        h.insert("stryke".to_string(), s("/opt/stryke"));
        let c = RemoteCluster::from_list_args(&[PerlValue::hash(h)]).expect("parse");
        assert_eq!(c.slots.len(), 2);
        assert_eq!(c.slots[0].host, "data1");
        assert_eq!(c.slots[0].pe_path, "/opt/stryke");
    }

    #[test]
    fn parses_trailing_tunables_hashref() {
        let mut tun = indexmap::IndexMap::new();
        tun.insert("timeout".to_string(), PerlValue::integer(30));
        tun.insert("retries".to_string(), PerlValue::integer(2));
        tun.insert("connect_timeout".to_string(), PerlValue::integer(5));
        let c = RemoteCluster::from_list_args(&[s("h1:1"), PerlValue::hash(tun)]).expect("parse");
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
    pub slots: Vec<RemoteSlot>,
    pub job_timeout_ms: u64,
    pub max_attempts: u32,
    pub connect_timeout_ms: u64,
}

impl RemoteCluster {
    pub const DEFAULT_JOB_TIMEOUT_MS: u64 = 60_000;
    pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
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
    pub fn from_list_args(items: &[PerlValue]) -> Result<Self, String> {
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
    pub stdout: String,
    pub stderr: String,
    pub exitcode: i64,
}

/// Columnar table from `dataframe(path)`; chain `filter`, `group_by`, `sum`, `nrow`.
#[derive(Debug, Clone)]
pub struct PerlDataFrame {
    pub columns: Vec<String>,
    pub cols: Vec<Vec<PerlValue>>,
    /// When set, `sum(col)` aggregates rows by this column.
    pub group_by: Option<String>,
}

impl PerlDataFrame {
    #[inline]
    pub fn nrows(&self) -> usize {
        self.cols.first().map(|c| c.len()).unwrap_or(0)
    }

    #[inline]
    pub fn ncols(&self) -> usize {
        self.columns.len()
    }

    #[inline]
    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }
}

/// Heap payload when [`PerlValue`] is not an immediate or raw [`f64`] bits.
#[derive(Debug, Clone)]
pub(crate) enum HeapObject {
    Integer(i64),
    Float(f64),
    String(String),
    Bytes(Arc<Vec<u8>>),
    Array(Vec<PerlValue>),
    Hash(IndexMap<String, PerlValue>),
    ArrayRef(Arc<RwLock<Vec<PerlValue>>>),
    HashRef(Arc<RwLock<IndexMap<String, PerlValue>>>),
    ScalarRef(Arc<RwLock<PerlValue>>),
    /// `\\$name` when `name` is a plain scalar variable — aliases that binding (Perl ref to lexical).
    ScalarBindingRef(String),
    /// `\\@name` — aliases the live array in [`crate::scope::Scope`] (same stash key as [`Op::GetArray`]).
    ArrayBindingRef(String),
    /// `\\%name` — aliases the live hash in scope.
    HashBindingRef(String),
    CodeRef(Arc<PerlSub>),
    /// Compiled regex: pattern source and flag chars (e.g. `"i"`, `"g"`) for re-match without re-parse.
    Regex(Arc<PerlCompiledRegex>, String, String),
    Blessed(Arc<BlessedRef>),
    IOHandle(String),
    Atomic(Arc<Mutex<PerlValue>>),
    Set(Arc<PerlSet>),
    ChannelTx(Arc<Sender<PerlValue>>),
    ChannelRx(Arc<Receiver<PerlValue>>),
    AsyncTask(Arc<PerlAsyncTask>),
    Generator(Arc<PerlGenerator>),
    Deque(Arc<Mutex<VecDeque<PerlValue>>>),
    Heap(Arc<Mutex<PerlHeap>>),
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
    Iterator(Arc<dyn PerlIterator>),
    /// Numeric/string dualvar: **`$!`** (errno + message) and **`$@`** (numeric flag or code + message).
    ErrnoDual {
        code: i32,
        msg: String,
    },
}

/// NaN-boxed value: one `u64` (immediates, raw float bits, or tagged heap pointer).
#[repr(transparent)]
pub struct PerlValue(pub(crate) u64);

impl Default for PerlValue {
    fn default() -> Self {
        Self::UNDEF
    }
}

impl Clone for PerlValue {
    fn clone(&self) -> Self {
        if nanbox::is_heap(self.0) {
            let arc = self.heap_arc();
            match &*arc {
                HeapObject::Array(v) => {
                    PerlValue::from_heap(Arc::new(HeapObject::Array(v.clone())))
                }
                HeapObject::Hash(h) => PerlValue::from_heap(Arc::new(HeapObject::Hash(h.clone()))),
                HeapObject::String(s) => {
                    PerlValue::from_heap(Arc::new(HeapObject::String(s.clone())))
                }
                HeapObject::Integer(n) => PerlValue::integer(*n),
                HeapObject::Float(f) => PerlValue::float(*f),
                _ => PerlValue::from_heap(Arc::clone(&arc)),
            }
        } else {
            PerlValue(self.0)
        }
    }
}

impl PerlValue {
    /// Stack duplicate (`Op::Dup`): share the outer heap [`Arc`] for arrays/hashes (COW on write),
    /// matching Perl temporaries; other heap payloads keep [`Clone`] semantics.
    #[inline]
    pub fn dup_stack(&self) -> Self {
        if nanbox::is_heap(self.0) {
            let arc = self.heap_arc();
            match &*arc {
                HeapObject::Array(_) | HeapObject::Hash(_) => {
                    PerlValue::from_heap(Arc::clone(&arc))
                }
                _ => self.clone(),
            }
        } else {
            PerlValue(self.0)
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
            PerlValue::from_heap(self.heap_arc())
        } else {
            PerlValue(self.0)
        }
    }
}

impl Drop for PerlValue {
    fn drop(&mut self) {
        if nanbox::is_heap(self.0) {
            unsafe {
                let p = nanbox::decode_heap_ptr::<HeapObject>(self.0) as *mut HeapObject;
                drop(Arc::from_raw(p));
            }
        }
    }
}

impl fmt::Debug for PerlValue {
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

#[derive(Debug, Clone)]
pub struct PerlSub {
    pub name: String,
    pub params: Vec<SubSigParam>,
    pub body: Block,
    /// Captured lexical scope (for closures)
    pub closure_env: Option<Vec<(String, PerlValue)>>,
    /// Prototype string from `sub name (PROTO) { }`, or `None`.
    pub prototype: Option<String>,
    /// When set, [`Interpreter::call_sub`](crate::interpreter::Interpreter::call_sub) may evaluate
    /// this sub with an explicit stack instead of recursive scope frames.
    pub fib_like: Option<FibLikeRecAddPattern>,
}

/// Operations queued on a [`PerlValue::pipeline`](crate::value::PerlValue::pipeline) value until `collect()`.
#[derive(Debug, Clone)]
pub enum PipelineOp {
    Filter(Arc<PerlSub>),
    Map(Arc<PerlSub>),
    /// `tap` / `peek` — run block for side effects; `@_` is the current stage list; value unchanged.
    Tap(Arc<PerlSub>),
    Take(i64),
    /// Parallel map (`pmap`) — optional stderr progress bar (same as `pmap ..., progress => 1`).
    PMap {
        sub: Arc<PerlSub>,
        progress: bool,
    },
    /// Parallel grep (`pgrep`).
    PGrep {
        sub: Arc<PerlSub>,
        progress: bool,
    },
    /// Parallel foreach (`pfor`) — side effects only; stream order preserved.
    PFor {
        sub: Arc<PerlSub>,
        progress: bool,
    },
    /// `pmap_chunked N { }` — chunk size + block.
    PMapChunked {
        chunk: i64,
        sub: Arc<PerlSub>,
        progress: bool,
    },
    /// `psort` / `psort { $a <=> $b }` — parallel sort.
    PSort {
        cmp: Option<Arc<PerlSub>>,
        progress: bool,
    },
    /// `pcache { }` — parallel memoized map.
    PCache {
        sub: Arc<PerlSub>,
        progress: bool,
    },
    /// `preduce { }` — must be last before `collect()`; `collect()` returns a scalar.
    PReduce {
        sub: Arc<PerlSub>,
        progress: bool,
    },
    /// `preduce_init EXPR, { }` — scalar result; must be last before `collect()`.
    PReduceInit {
        init: PerlValue,
        sub: Arc<PerlSub>,
        progress: bool,
    },
    /// `pmap_reduce { } { }` — scalar result; must be last before `collect()`.
    PMapReduce {
        map: Arc<PerlSub>,
        reduce: Arc<PerlSub>,
        progress: bool,
    },
}

#[derive(Debug)]
pub struct PipelineInner {
    pub source: Vec<PerlValue>,
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

#[derive(Debug)]
pub struct BlessedRef {
    pub class: String,
    pub data: RwLock<PerlValue>,
    /// When true, dropping does not enqueue `DESTROY` (temporary invocant built while running a destructor).
    pub(crate) suppress_destroy_queue: AtomicBool,
}

impl BlessedRef {
    pub(crate) fn new_blessed(class: String, data: PerlValue) -> Self {
        Self {
            class,
            data: RwLock::new(data),
            suppress_destroy_queue: AtomicBool::new(false),
        }
    }

    /// Invocant for a running `DESTROY` — must not re-queue when dropped after the call.
    pub(crate) fn new_for_destroy_invocant(class: String, data: PerlValue) -> Self {
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
    pub def: Arc<StructDef>,
    pub values: RwLock<Vec<PerlValue>>,
}

impl StructInstance {
    /// Create a new struct instance with the given definition and values.
    pub fn new(def: Arc<StructDef>, values: Vec<PerlValue>) -> Self {
        Self {
            def,
            values: RwLock::new(values),
        }
    }

    /// Get a field value by index (clones the value).
    #[inline]
    pub fn get_field(&self, idx: usize) -> Option<PerlValue> {
        self.values.read().get(idx).cloned()
    }

    /// Set a field value by index.
    #[inline]
    pub fn set_field(&self, idx: usize, val: PerlValue) {
        if let Some(slot) = self.values.write().get_mut(idx) {
            *slot = val;
        }
    }

    /// Get all field values (clones the vector).
    #[inline]
    pub fn get_values(&self) -> Vec<PerlValue> {
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
    pub def: Arc<EnumDef>,
    pub variant_idx: usize,
    /// Data carried by this variant. For variants with no data, this is UNDEF.
    pub data: PerlValue,
}

impl EnumInstance {
    pub fn new(def: Arc<EnumDef>, variant_idx: usize, data: PerlValue) -> Self {
        Self {
            def,
            variant_idx,
            data,
        }
    }

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
    pub def: Arc<ClassDef>,
    pub values: RwLock<Vec<PerlValue>>,
    /// Full ISA chain for this class (all ancestors, computed at instantiation).
    pub isa_chain: Vec<String>,
}

impl ClassInstance {
    pub fn new(def: Arc<ClassDef>, values: Vec<PerlValue>) -> Self {
        Self {
            def,
            values: RwLock::new(values),
            isa_chain: Vec::new(),
        }
    }

    pub fn new_with_isa(
        def: Arc<ClassDef>,
        values: Vec<PerlValue>,
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

    #[inline]
    pub fn get_field(&self, idx: usize) -> Option<PerlValue> {
        self.values.read().get(idx).cloned()
    }

    #[inline]
    pub fn set_field(&self, idx: usize, val: PerlValue) {
        if let Some(slot) = self.values.write().get_mut(idx) {
            *slot = val;
        }
    }

    #[inline]
    pub fn get_values(&self) -> Vec<PerlValue> {
        self.values.read().clone()
    }

    /// Get field value by name (searches through class and parent hierarchies).
    pub fn get_field_by_name(&self, name: &str) -> Option<PerlValue> {
        self.def
            .field_index(name)
            .and_then(|idx| self.get_field(idx))
    }

    /// Set field value by name.
    pub fn set_field_by_name(&self, name: &str, val: PerlValue) -> bool {
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

impl PerlValue {
    pub const UNDEF: PerlValue = PerlValue(nanbox::encode_imm_undef());

    #[inline]
    fn from_heap(arc: Arc<HeapObject>) -> PerlValue {
        let ptr = Arc::into_raw(arc);
        PerlValue(nanbox::encode_heap_ptr(ptr))
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

    /// Reconstruct from [`Self::raw_bits`] (e.g. block JIT returning a full [`PerlValue`] encoding in `i64`).
    #[inline]
    pub(crate) fn from_raw_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// `typed : Int` — inline `i32` or heap `i64`.
    #[inline]
    pub fn is_integer_like(&self) -> bool {
        nanbox::as_imm_int32(self.0).is_some()
            || matches!(
                self.with_heap(|h| matches!(h, HeapObject::Integer(_))),
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

    #[inline]
    pub fn integer(n: i64) -> Self {
        if n >= i32::MIN as i64 && n <= i32::MAX as i64 {
            PerlValue(nanbox::encode_imm_int32(n as i32))
        } else {
            Self::from_heap(Arc::new(HeapObject::Integer(n)))
        }
    }

    #[inline]
    pub fn float(f: f64) -> Self {
        if nanbox::float_needs_box(f) {
            Self::from_heap(Arc::new(HeapObject::Float(f)))
        } else {
            PerlValue(f.to_bits())
        }
    }

    #[inline]
    pub fn string(s: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::String(s)))
    }

    #[inline]
    pub fn bytes(b: Arc<Vec<u8>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Bytes(b)))
    }

    #[inline]
    pub fn array(v: Vec<PerlValue>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Array(v)))
    }

    /// Wrap a lazy iterator as a PerlValue.
    #[inline]
    pub fn iterator(it: Arc<dyn PerlIterator>) -> Self {
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
    pub fn into_iterator(&self) -> Arc<dyn PerlIterator> {
        if nanbox::is_heap(self.0) {
            if let HeapObject::Iterator(it) = &*self.heap_arc() {
                return Arc::clone(it);
            }
        }
        panic!("into_iterator on non-iterator value");
    }

    #[inline]
    pub fn hash(h: IndexMap<String, PerlValue>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Hash(h)))
    }

    #[inline]
    pub fn array_ref(a: Arc<RwLock<Vec<PerlValue>>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ArrayRef(a)))
    }

    #[inline]
    pub fn hash_ref(h: Arc<RwLock<IndexMap<String, PerlValue>>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::HashRef(h)))
    }

    #[inline]
    pub fn scalar_ref(r: Arc<RwLock<PerlValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ScalarRef(r)))
    }

    #[inline]
    pub fn scalar_binding_ref(name: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::ScalarBindingRef(name)))
    }

    #[inline]
    pub fn array_binding_ref(name: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::ArrayBindingRef(name)))
    }

    #[inline]
    pub fn hash_binding_ref(name: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::HashBindingRef(name)))
    }

    #[inline]
    pub fn code_ref(c: Arc<PerlSub>) -> Self {
        Self::from_heap(Arc::new(HeapObject::CodeRef(c)))
    }

    #[inline]
    pub fn as_code_ref(&self) -> Option<Arc<PerlSub>> {
        self.with_heap(|h| match h {
            HeapObject::CodeRef(sub) => Some(Arc::clone(sub)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_regex(&self) -> Option<Arc<PerlCompiledRegex>> {
        self.with_heap(|h| match h {
            HeapObject::Regex(re, _, _) => Some(Arc::clone(re)),
            _ => None,
        })
        .flatten()
    }

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
    pub fn hash_get(&self, key: &str) -> Option<PerlValue> {
        self.with_heap(|h| match h {
            HeapObject::Hash(h) => h.get(key).cloned(),
            _ => None,
        })
        .flatten()
    }

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
            _ => None,
        })
        .flatten()
    }

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

    #[inline]
    pub fn as_array_vec(&self) -> Option<Vec<PerlValue>> {
        self.with_heap(|h| match h {
            HeapObject::Array(v) => Some(v.clone()),
            _ => None,
        })
        .flatten()
    }

    /// Expand a `map` / `flat_map` / `pflat_map` block result into list elements. Plain arrays
    /// expand; when `peel_array_ref`, a single ARRAY ref is dereferenced one level (stryke
    /// `flat_map` / `pflat_map`; stock `map` uses `peel_array_ref == false`).
    pub fn map_flatten_outputs(&self, peel_array_ref: bool) -> Vec<PerlValue> {
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

    #[inline]
    pub fn as_hash_map(&self) -> Option<IndexMap<String, PerlValue>> {
        self.with_heap(|h| match h {
            HeapObject::Hash(h) => Some(h.clone()),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_bytes_arc(&self) -> Option<Arc<Vec<u8>>> {
        self.with_heap(|h| match h {
            HeapObject::Bytes(b) => Some(Arc::clone(b)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_async_task(&self) -> Option<Arc<PerlAsyncTask>> {
        self.with_heap(|h| match h {
            HeapObject::AsyncTask(t) => Some(Arc::clone(t)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_generator(&self) -> Option<Arc<PerlGenerator>> {
        self.with_heap(|h| match h {
            HeapObject::Generator(g) => Some(Arc::clone(g)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_atomic_arc(&self) -> Option<Arc<Mutex<PerlValue>>> {
        self.with_heap(|h| match h {
            HeapObject::Atomic(a) => Some(Arc::clone(a)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_io_handle_name(&self) -> Option<String> {
        self.with_heap(|h| match h {
            HeapObject::IOHandle(n) => Some(n.clone()),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_sqlite_conn(&self) -> Option<Arc<Mutex<rusqlite::Connection>>> {
        self.with_heap(|h| match h {
            HeapObject::SqliteConn(c) => Some(Arc::clone(c)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_struct_inst(&self) -> Option<Arc<StructInstance>> {
        self.with_heap(|h| match h {
            HeapObject::StructInst(s) => Some(Arc::clone(s)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_enum_inst(&self) -> Option<Arc<EnumInstance>> {
        self.with_heap(|h| match h {
            HeapObject::EnumInst(e) => Some(Arc::clone(e)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_class_inst(&self) -> Option<Arc<ClassInstance>> {
        self.with_heap(|h| match h {
            HeapObject::ClassInst(c) => Some(Arc::clone(c)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_dataframe(&self) -> Option<Arc<Mutex<PerlDataFrame>>> {
        self.with_heap(|h| match h {
            HeapObject::DataFrame(d) => Some(Arc::clone(d)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_deque(&self) -> Option<Arc<Mutex<VecDeque<PerlValue>>>> {
        self.with_heap(|h| match h {
            HeapObject::Deque(d) => Some(Arc::clone(d)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_heap_pq(&self) -> Option<Arc<Mutex<PerlHeap>>> {
        self.with_heap(|h| match h {
            HeapObject::Heap(h) => Some(Arc::clone(h)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_pipeline(&self) -> Option<Arc<Mutex<PipelineInner>>> {
        self.with_heap(|h| match h {
            HeapObject::Pipeline(p) => Some(Arc::clone(p)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_capture(&self) -> Option<Arc<CaptureResult>> {
        self.with_heap(|h| match h {
            HeapObject::Capture(c) => Some(Arc::clone(c)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_ppool(&self) -> Option<PerlPpool> {
        self.with_heap(|h| match h {
            HeapObject::Ppool(p) => Some(p.clone()),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_remote_cluster(&self) -> Option<Arc<RemoteCluster>> {
        self.with_heap(|h| match h {
            HeapObject::RemoteCluster(c) => Some(Arc::clone(c)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_barrier(&self) -> Option<PerlBarrier> {
        self.with_heap(|h| match h {
            HeapObject::Barrier(b) => Some(b.clone()),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_channel_tx(&self) -> Option<Arc<Sender<PerlValue>>> {
        self.with_heap(|h| match h {
            HeapObject::ChannelTx(t) => Some(Arc::clone(t)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_channel_rx(&self) -> Option<Arc<Receiver<PerlValue>>> {
        self.with_heap(|h| match h {
            HeapObject::ChannelRx(r) => Some(Arc::clone(r)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_scalar_ref(&self) -> Option<Arc<RwLock<PerlValue>>> {
        self.with_heap(|h| match h {
            HeapObject::ScalarRef(r) => Some(Arc::clone(r)),
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

    #[inline]
    pub fn as_array_ref(&self) -> Option<Arc<RwLock<Vec<PerlValue>>>> {
        self.with_heap(|h| match h {
            HeapObject::ArrayRef(r) => Some(Arc::clone(r)),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn as_hash_ref(&self) -> Option<Arc<RwLock<IndexMap<String, PerlValue>>>> {
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

    #[inline]
    pub fn blessed(b: Arc<BlessedRef>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Blessed(b)))
    }

    #[inline]
    pub fn io_handle(name: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::IOHandle(name)))
    }

    #[inline]
    pub fn atomic(a: Arc<Mutex<PerlValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Atomic(a)))
    }

    #[inline]
    pub fn set(s: Arc<PerlSet>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Set(s)))
    }

    #[inline]
    pub fn channel_tx(tx: Arc<Sender<PerlValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ChannelTx(tx)))
    }

    #[inline]
    pub fn channel_rx(rx: Arc<Receiver<PerlValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ChannelRx(rx)))
    }

    #[inline]
    pub fn async_task(t: Arc<PerlAsyncTask>) -> Self {
        Self::from_heap(Arc::new(HeapObject::AsyncTask(t)))
    }

    #[inline]
    pub fn generator(g: Arc<PerlGenerator>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Generator(g)))
    }

    #[inline]
    pub fn deque(d: Arc<Mutex<VecDeque<PerlValue>>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Deque(d)))
    }

    #[inline]
    pub fn heap(h: Arc<Mutex<PerlHeap>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Heap(h)))
    }

    #[inline]
    pub fn pipeline(p: Arc<Mutex<PipelineInner>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Pipeline(p)))
    }

    #[inline]
    pub fn capture(c: Arc<CaptureResult>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Capture(c)))
    }

    #[inline]
    pub fn ppool(p: PerlPpool) -> Self {
        Self::from_heap(Arc::new(HeapObject::Ppool(p)))
    }

    #[inline]
    pub fn remote_cluster(c: Arc<RemoteCluster>) -> Self {
        Self::from_heap(Arc::new(HeapObject::RemoteCluster(c)))
    }

    #[inline]
    pub fn barrier(b: PerlBarrier) -> Self {
        Self::from_heap(Arc::new(HeapObject::Barrier(b)))
    }

    #[inline]
    pub fn sqlite_conn(c: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::SqliteConn(c)))
    }

    #[inline]
    pub fn struct_inst(s: Arc<StructInstance>) -> Self {
        Self::from_heap(Arc::new(HeapObject::StructInst(s)))
    }

    #[inline]
    pub fn enum_inst(e: Arc<EnumInstance>) -> Self {
        Self::from_heap(Arc::new(HeapObject::EnumInst(e)))
    }

    #[inline]
    pub fn class_inst(c: Arc<ClassInstance>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ClassInst(c)))
    }

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

    #[inline]
    pub fn unwrap_atomic(&self) -> PerlValue {
        if !nanbox::is_heap(self.0) {
            return self.clone();
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Atomic(a) => a.lock().clone(),
            _ => self.clone(),
        }
    }

    #[inline]
    pub fn is_atomic(&self) -> bool {
        if !nanbox::is_heap(self.0) {
            return false;
        }
        matches!(unsafe { self.heap_ref() }, HeapObject::Atomic(_))
    }

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
            HeapObject::Array(a) => !a.is_empty(),
            HeapObject::Hash(h) => !h.is_empty(),
            HeapObject::Atomic(arc) => arc.lock().is_true(),
            HeapObject::Set(s) => !s.is_empty(),
            HeapObject::Deque(d) => !d.lock().is_empty(),
            HeapObject::Heap(h) => !h.lock().items.is_empty(),
            HeapObject::DataFrame(d) => d.lock().nrows() > 0,
            HeapObject::Pipeline(_) | HeapObject::Capture(_) => true,
            _ => true,
        }
    }

    /// String concat with owned LHS: moves out a uniquely held heap string when possible
    /// ([`Self::into_string`]), then appends `rhs`. Used for `.=` and VM concat-append ops.
    #[inline]
    pub(crate) fn concat_append_owned(self, rhs: &PerlValue) -> PerlValue {
        let mut s = self.into_string();
        rhs.append_to(&mut s);
        PerlValue::string(s)
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
    pub(crate) fn try_concat_append_inplace(&mut self, rhs: &PerlValue) -> bool {
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
                    Ok(o) => return PerlValue::from_heap(Arc::new(o)).to_string(),
                    Err(arc) => {
                        return match &*arc {
                            HeapObject::String(s) => s.clone(),
                            _ => PerlValue::from_heap(Arc::clone(&arc)).to_string(),
                        };
                    }
                }
            }
        }
        String::new()
    }

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
            HeapObject::ScalarRef(_) | HeapObject::ScalarBindingRef(_) => "SCALAR".to_string(),
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
            HeapObject::Float(_) => "FLOAT".to_string(),
        }
    }

    pub fn ref_type(&self) -> PerlValue {
        if !nanbox::is_heap(self.0) {
            return PerlValue::string(String::new());
        }
        match unsafe { self.heap_ref() } {
            HeapObject::ArrayRef(_) | HeapObject::ArrayBindingRef(_) => {
                PerlValue::string("ARRAY".into())
            }
            HeapObject::HashRef(_) | HeapObject::HashBindingRef(_) => {
                PerlValue::string("HASH".into())
            }
            HeapObject::ScalarRef(_) | HeapObject::ScalarBindingRef(_) => {
                PerlValue::string("SCALAR".into())
            }
            HeapObject::CodeRef(_) => PerlValue::string("CODE".into()),
            HeapObject::Regex(_, _, _) => PerlValue::string("Regexp".into()),
            HeapObject::Atomic(_) => PerlValue::string("ATOMIC".into()),
            HeapObject::Set(_) => PerlValue::string("Set".into()),
            HeapObject::ChannelTx(_) => PerlValue::string("PCHANNEL::Tx".into()),
            HeapObject::ChannelRx(_) => PerlValue::string("PCHANNEL::Rx".into()),
            HeapObject::AsyncTask(_) => PerlValue::string("ASYNCTASK".into()),
            HeapObject::Generator(_) => PerlValue::string("Generator".into()),
            HeapObject::Deque(_) => PerlValue::string("Deque".into()),
            HeapObject::Heap(_) => PerlValue::string("Heap".into()),
            HeapObject::Pipeline(_) => PerlValue::string("Pipeline".into()),
            HeapObject::DataFrame(_) => PerlValue::string("DataFrame".into()),
            HeapObject::Capture(_) => PerlValue::string("Capture".into()),
            HeapObject::Ppool(_) => PerlValue::string("Ppool".into()),
            HeapObject::RemoteCluster(_) => PerlValue::string("Cluster".into()),
            HeapObject::Barrier(_) => PerlValue::string("Barrier".into()),
            HeapObject::SqliteConn(_) => PerlValue::string("SqliteConn".into()),
            HeapObject::StructInst(s) => PerlValue::string(s.def.name.clone()),
            HeapObject::EnumInst(e) => PerlValue::string(e.def.name.clone()),
            HeapObject::Bytes(_) => PerlValue::string("BYTES".into()),
            HeapObject::Blessed(b) => PerlValue::string(b.class.clone()),
            _ => PerlValue::string(String::new()),
        }
    }

    pub fn num_cmp(&self, other: &PerlValue) -> Ordering {
        let a = self.to_number();
        let b = other.to_number();
        a.partial_cmp(&b).unwrap_or(Ordering::Equal)
    }

    /// String equality for `eq` / `cmp` without allocating when both sides are heap strings.
    #[inline]
    pub fn str_eq(&self, other: &PerlValue) -> bool {
        if nanbox::is_heap(self.0) && nanbox::is_heap(other.0) {
            if let (HeapObject::String(a), HeapObject::String(b)) =
                unsafe { (self.heap_ref(), other.heap_ref()) }
            {
                return a == b;
            }
        }
        self.to_string() == other.to_string()
    }

    pub fn str_cmp(&self, other: &PerlValue) -> Ordering {
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
    pub fn struct_field_eq(&self, other: &PerlValue) -> bool {
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
    pub fn deep_clone(&self) -> PerlValue {
        if !nanbox::is_heap(self.0) {
            return self.clone();
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Array(a) => PerlValue::array(a.iter().map(|v| v.deep_clone()).collect()),
            HeapObject::ArrayRef(a) => {
                let cloned: Vec<PerlValue> = a.read().iter().map(|v| v.deep_clone()).collect();
                PerlValue::array_ref(Arc::new(RwLock::new(cloned)))
            }
            HeapObject::Hash(h) => {
                let mut cloned = IndexMap::new();
                for (k, v) in h.iter() {
                    cloned.insert(k.clone(), v.deep_clone());
                }
                PerlValue::hash(cloned)
            }
            HeapObject::HashRef(h) => {
                let mut cloned = IndexMap::new();
                for (k, v) in h.read().iter() {
                    cloned.insert(k.clone(), v.deep_clone());
                }
                PerlValue::hash_ref(Arc::new(RwLock::new(cloned)))
            }
            HeapObject::StructInst(s) => {
                let new_values = s.get_values().iter().map(|v| v.deep_clone()).collect();
                PerlValue::struct_inst(Arc::new(StructInstance::new(
                    Arc::clone(&s.def),
                    new_values,
                )))
            }
            _ => self.clone(),
        }
    }

    pub fn to_list(&self) -> Vec<PerlValue> {
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
                .flat_map(|(k, v)| vec![PerlValue::string(k.clone()), v.clone()])
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

    pub fn scalar_context(&self) -> PerlValue {
        if !nanbox::is_heap(self.0) {
            return self.clone();
        }
        if let Some(arc) = self.as_atomic_arc() {
            return arc.lock().scalar_context();
        }
        match unsafe { self.heap_ref() } {
            HeapObject::Array(a) => PerlValue::integer(a.len() as i64),
            HeapObject::Hash(h) => {
                if h.is_empty() {
                    PerlValue::integer(0)
                } else {
                    PerlValue::string(format!("{}/{}", h.len(), h.capacity()))
                }
            }
            HeapObject::Set(s) => PerlValue::integer(s.len() as i64),
            HeapObject::Deque(d) => PerlValue::integer(d.lock().len() as i64),
            HeapObject::Heap(h) => PerlValue::integer(h.lock().items.len() as i64),
            HeapObject::Pipeline(p) => PerlValue::integer(p.lock().source.len() as i64),
            HeapObject::Capture(_)
            | HeapObject::Ppool(_)
            | HeapObject::RemoteCluster(_)
            | HeapObject::Barrier(_) => PerlValue::integer(1),
            HeapObject::Generator(_) => PerlValue::integer(1),
            _ => self.clone(),
        }
    }
}

impl fmt::Display for PerlValue {
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
            HeapObject::ScalarRef(_) | HeapObject::ScalarBindingRef(_) => {
                f.write_str("SCALAR(0x...)")
            }
            HeapObject::CodeRef(sub) => write!(f, "CODE({})", sub.name),
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
                        values.get(i).cloned().unwrap_or(PerlValue::UNDEF)
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
                        values.get(i).cloned().unwrap_or(PerlValue::UNDEF)
                    )?;
                }
                f.write_str(")")
            }
            HeapObject::DataFrame(d) => {
                let g = d.lock();
                write!(f, "DataFrame({} rows)", g.nrows())
            }
            HeapObject::Iterator(_) => f.write_str("Iterator"),
        }
    }
}

/// Stable key for set membership (dedup of `PerlValue` in this runtime).
pub fn set_member_key(v: &PerlValue) -> String {
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
        HeapObject::ScalarRef(_) | HeapObject::ScalarBindingRef(_) => format!("sr:{v}"),
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
        HeapObject::Iterator(_) => "iter".to_string(),
        HeapObject::ErrnoDual { code, msg } => format!("e:{code}:{msg}"),
        HeapObject::Integer(n) => format!("i:{n}"),
        HeapObject::Float(fl) => format!("f:{}", fl.to_bits()),
    }
}

pub fn set_from_elements<I: IntoIterator<Item = PerlValue>>(items: I) -> PerlValue {
    let mut map = PerlSet::new();
    for v in items {
        let k = set_member_key(&v);
        map.insert(k, v);
    }
    PerlValue::set(Arc::new(map))
}

/// Underlying set for union/intersection, including `mysync $s` (`Atomic` wrapping `Set`).
#[inline]
pub fn set_payload(v: &PerlValue) -> Option<Arc<PerlSet>> {
    if !nanbox::is_heap(v.0) {
        return None;
    }
    match unsafe { v.heap_ref() } {
        HeapObject::Set(s) => Some(Arc::clone(s)),
        HeapObject::Atomic(a) => set_payload(&a.lock()),
        _ => None,
    }
}

pub fn set_union(a: &PerlValue, b: &PerlValue) -> Option<PerlValue> {
    let ia = set_payload(a)?;
    let ib = set_payload(b)?;
    let mut m = (*ia).clone();
    for (k, v) in ib.iter() {
        m.entry(k.clone()).or_insert_with(|| v.clone());
    }
    Some(PerlValue::set(Arc::new(m)))
}

pub fn set_intersection(a: &PerlValue, b: &PerlValue) -> Option<PerlValue> {
    let ia = set_payload(a)?;
    let ib = set_payload(b)?;
    let mut m = PerlSet::new();
    for (k, v) in ia.iter() {
        if ib.contains_key(k) {
            m.insert(k.clone(), v.clone());
        }
    }
    Some(PerlValue::set(Arc::new(m)))
}
fn parse_number(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
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
pub(crate) fn perl_list_range_pair_is_numeric(left: &PerlValue, right: &PerlValue) -> bool {
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

fn perl_list_range_expand_string_magic(from: PerlValue, to: PerlValue) -> Vec<PerlValue> {
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
        out.push(PerlValue::string(cur.clone()));
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
pub(crate) fn perl_list_range_expand(from: PerlValue, to: PerlValue) -> Vec<PerlValue> {
    if perl_list_range_pair_is_numeric(&from, &to) {
        let i = from.to_int();
        let j = to.to_int();
        if j >= i {
            (i..=j).map(PerlValue::integer).collect()
        } else {
            Vec::new()
        }
    } else {
        perl_list_range_expand_string_magic(from, to)
    }
}

impl PerlDataFrame {
    /// One row as a hashref (`$_` in `filter`).
    pub fn row_hashref(&self, row: usize) -> PerlValue {
        let mut m = IndexMap::new();
        for (i, col) in self.columns.iter().enumerate() {
            m.insert(
                col.clone(),
                self.cols[i].get(row).cloned().unwrap_or(PerlValue::UNDEF),
            );
        }
        PerlValue::hash_ref(Arc::new(RwLock::new(m)))
    }
}

#[cfg(test)]
mod tests {
    use super::PerlValue;
    use crate::perl_regex::PerlCompiledRegex;
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::cmp::Ordering;
    use std::sync::Arc;

    #[test]
    fn undef_is_false() {
        assert!(!PerlValue::UNDEF.is_true());
    }

    #[test]
    fn string_zero_is_false() {
        assert!(!PerlValue::string("0".into()).is_true());
        assert!(PerlValue::string("00".into()).is_true());
    }

    #[test]
    fn empty_string_is_false() {
        assert!(!PerlValue::string(String::new()).is_true());
    }

    #[test]
    fn integer_zero_is_false_nonzero_true() {
        assert!(!PerlValue::integer(0).is_true());
        assert!(PerlValue::integer(-1).is_true());
    }

    #[test]
    fn float_zero_is_false_nonzero_true() {
        assert!(!PerlValue::float(0.0).is_true());
        assert!(PerlValue::float(0.1).is_true());
    }

    #[test]
    fn num_cmp_orders_float_against_integer() {
        assert_eq!(
            PerlValue::float(2.5).num_cmp(&PerlValue::integer(3)),
            Ordering::Less
        );
    }

    #[test]
    fn to_int_parses_leading_number_from_string() {
        assert_eq!(PerlValue::string("42xyz".into()).to_int(), 42);
        assert_eq!(PerlValue::string("  -3.7foo".into()).to_int(), -3);
    }

    #[test]
    fn num_cmp_orders_as_numeric() {
        assert_eq!(
            PerlValue::integer(2).num_cmp(&PerlValue::integer(11)),
            Ordering::Less
        );
        assert_eq!(
            PerlValue::string("2foo".into()).num_cmp(&PerlValue::string("11".into())),
            Ordering::Less
        );
    }

    #[test]
    fn str_cmp_orders_as_strings() {
        assert_eq!(
            PerlValue::string("2".into()).str_cmp(&PerlValue::string("11".into())),
            Ordering::Greater
        );
    }

    #[test]
    fn str_eq_heap_strings_fast_path() {
        let a = PerlValue::string("hello".into());
        let b = PerlValue::string("hello".into());
        assert!(a.str_eq(&b));
        assert!(!a.str_eq(&PerlValue::string("hell".into())));
    }

    #[test]
    fn str_eq_fallback_matches_stringified_equality() {
        let n = PerlValue::integer(42);
        let s = PerlValue::string("42".into());
        assert!(n.str_eq(&s));
        assert!(!PerlValue::integer(1).str_eq(&PerlValue::string("2".into())));
    }

    #[test]
    fn str_cmp_heap_strings_fast_path() {
        assert_eq!(
            PerlValue::string("a".into()).str_cmp(&PerlValue::string("b".into())),
            Ordering::Less
        );
    }

    #[test]
    fn scalar_context_array_and_hash() {
        let a =
            PerlValue::array(vec![PerlValue::integer(1), PerlValue::integer(2)]).scalar_context();
        assert_eq!(a.to_int(), 2);
        let mut h = IndexMap::new();
        h.insert("a".into(), PerlValue::integer(1));
        let sc = PerlValue::hash(h).scalar_context();
        assert!(sc.is_string_like());
    }

    #[test]
    fn to_list_array_hash_and_scalar() {
        assert_eq!(
            PerlValue::array(vec![PerlValue::integer(7)])
                .to_list()
                .len(),
            1
        );
        let mut h = IndexMap::new();
        h.insert("k".into(), PerlValue::integer(1));
        let list = PerlValue::hash(h).to_list();
        assert_eq!(list.len(), 2);
        let one = PerlValue::integer(99).to_list();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].to_int(), 99);
    }

    #[test]
    fn type_name_and_ref_type_for_core_kinds() {
        assert_eq!(PerlValue::integer(0).type_name(), "INTEGER");
        assert_eq!(PerlValue::UNDEF.ref_type().to_string(), "");
        assert_eq!(
            PerlValue::array_ref(Arc::new(RwLock::new(vec![])))
                .ref_type()
                .to_string(),
            "ARRAY"
        );
    }

    #[test]
    fn display_undef_is_empty_integer_is_decimal() {
        assert_eq!(PerlValue::UNDEF.to_string(), "");
        assert_eq!(PerlValue::integer(-7).to_string(), "-7");
    }

    #[test]
    fn empty_array_is_false_nonempty_is_true() {
        assert!(!PerlValue::array(vec![]).is_true());
        assert!(PerlValue::array(vec![PerlValue::integer(0)]).is_true());
    }

    #[test]
    fn to_number_undef_and_non_numeric_refs_are_zero() {
        use super::PerlSub;

        assert_eq!(PerlValue::UNDEF.to_number(), 0.0);
        assert_eq!(
            PerlValue::code_ref(Arc::new(PerlSub {
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
        PerlValue::integer(-12).append_to(&mut buf);
        PerlValue::string("ab".into()).append_to(&mut buf);
        assert_eq!(buf, "-12ab");
        let mut u = String::new();
        PerlValue::UNDEF.append_to(&mut u);
        assert!(u.is_empty());
    }

    #[test]
    fn append_to_atomic_delegates_to_inner() {
        use parking_lot::Mutex;
        let a = PerlValue::atomic(Arc::new(Mutex::new(PerlValue::string("z".into()))));
        let mut buf = String::new();
        a.append_to(&mut buf);
        assert_eq!(buf, "z");
    }

    #[test]
    fn unwrap_atomic_reads_inner_other_variants_clone() {
        use parking_lot::Mutex;
        let a = PerlValue::atomic(Arc::new(Mutex::new(PerlValue::integer(9))));
        assert_eq!(a.unwrap_atomic().to_int(), 9);
        assert_eq!(PerlValue::integer(3).unwrap_atomic().to_int(), 3);
    }

    #[test]
    fn is_atomic_only_true_for_atomic_variant() {
        use parking_lot::Mutex;
        assert!(PerlValue::atomic(Arc::new(Mutex::new(PerlValue::UNDEF))).is_atomic());
        assert!(!PerlValue::integer(0).is_atomic());
    }

    #[test]
    fn as_str_only_on_string_variant() {
        assert_eq!(
            PerlValue::string("x".into()).as_str(),
            Some("x".to_string())
        );
        assert_eq!(PerlValue::integer(1).as_str(), None);
    }

    #[test]
    fn as_str_or_empty_defaults_non_string() {
        assert_eq!(PerlValue::string("z".into()).as_str_or_empty(), "z");
        assert_eq!(PerlValue::integer(1).as_str_or_empty(), "");
    }

    #[test]
    fn to_int_truncates_float_toward_zero() {
        assert_eq!(PerlValue::float(3.9).to_int(), 3);
        assert_eq!(PerlValue::float(-2.1).to_int(), -2);
    }

    #[test]
    fn to_number_array_is_length() {
        assert_eq!(
            PerlValue::array(vec![PerlValue::integer(1), PerlValue::integer(2)]).to_number(),
            2.0
        );
    }

    #[test]
    fn scalar_context_empty_hash_is_zero() {
        let h = IndexMap::new();
        assert_eq!(PerlValue::hash(h).scalar_context().to_int(), 0);
    }

    #[test]
    fn scalar_context_nonhash_nonarray_clones() {
        let v = PerlValue::integer(8);
        assert_eq!(v.scalar_context().to_int(), 8);
    }

    #[test]
    fn display_float_integer_like_omits_decimal() {
        assert_eq!(PerlValue::float(4.0).to_string(), "4");
    }

    #[test]
    fn display_array_concatenates_element_displays() {
        let a = PerlValue::array(vec![PerlValue::integer(1), PerlValue::string("b".into())]);
        assert_eq!(a.to_string(), "1b");
    }

    #[test]
    fn display_code_ref_includes_sub_name() {
        use super::PerlSub;
        let c = PerlValue::code_ref(Arc::new(PerlSub {
            name: "foo".into(),
            params: vec![],
            body: vec![],
            closure_env: None,
            prototype: None,
            fib_like: None,
        }));
        assert!(c.to_string().contains("foo"));
    }

    #[test]
    fn display_regex_shows_non_capturing_prefix() {
        let r = PerlValue::regex(
            PerlCompiledRegex::compile("x+").unwrap(),
            "x+".into(),
            "".into(),
        );
        assert_eq!(r.to_string(), "(?:x+)");
    }

    #[test]
    fn display_iohandle_is_name() {
        assert_eq!(PerlValue::io_handle("STDOUT".into()).to_string(), "STDOUT");
    }

    #[test]
    fn ref_type_blessed_uses_class_name() {
        let b = PerlValue::blessed(Arc::new(super::BlessedRef::new_blessed(
            "Pkg".into(),
            PerlValue::UNDEF,
        )));
        assert_eq!(b.ref_type().to_string(), "Pkg");
    }

    #[test]
    fn blessed_drop_enqueues_pending_destroy() {
        let v = PerlValue::blessed(Arc::new(super::BlessedRef::new_blessed(
            "Z".into(),
            PerlValue::integer(7),
        )));
        drop(v);
        let q = crate::pending_destroy::take_queue();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].0, "Z");
        assert_eq!(q[0].1.to_int(), 7);
    }

    #[test]
    fn type_name_iohandle_is_glob() {
        assert_eq!(PerlValue::io_handle("FH".into()).type_name(), "GLOB");
    }

    #[test]
    fn empty_hash_is_false() {
        assert!(!PerlValue::hash(IndexMap::new()).is_true());
    }

    #[test]
    fn hash_nonempty_is_true() {
        let mut h = IndexMap::new();
        h.insert("k".into(), PerlValue::UNDEF);
        assert!(PerlValue::hash(h).is_true());
    }

    #[test]
    fn num_cmp_equal_integers() {
        assert_eq!(
            PerlValue::integer(5).num_cmp(&PerlValue::integer(5)),
            Ordering::Equal
        );
    }

    #[test]
    fn str_cmp_compares_lexicographic_string_forms() {
        // Display forms "2" and "10" — string order differs from numeric order.
        assert_eq!(
            PerlValue::integer(2).str_cmp(&PerlValue::integer(10)),
            Ordering::Greater
        );
    }

    #[test]
    fn to_list_undef_empty() {
        assert!(PerlValue::UNDEF.to_list().is_empty());
    }

    #[test]
    fn unwrap_atomic_nested_atomic() {
        use parking_lot::Mutex;
        let inner = PerlValue::atomic(Arc::new(Mutex::new(PerlValue::integer(2))));
        let outer = PerlValue::atomic(Arc::new(Mutex::new(inner)));
        assert_eq!(outer.unwrap_atomic().to_int(), 2);
    }

    #[test]
    fn errno_dual_parts_extracts_code_and_message() {
        let v = PerlValue::errno_dual(-2, "oops".into());
        assert_eq!(v.errno_dual_parts(), Some((-2, "oops".into())));
    }

    #[test]
    fn errno_dual_parts_none_for_plain_string() {
        assert!(PerlValue::string("hi".into()).errno_dual_parts().is_none());
    }

    #[test]
    fn errno_dual_parts_none_for_integer() {
        assert!(PerlValue::integer(1).errno_dual_parts().is_none());
    }

    #[test]
    fn errno_dual_numeric_context_uses_code_string_uses_msg() {
        let v = PerlValue::errno_dual(5, "five".into());
        assert_eq!(v.to_int(), 5);
        assert_eq!(v.to_string(), "five");
    }

    #[test]
    fn list_range_alpha_joins_like_perl() {
        use super::perl_list_range_expand;
        let v =
            perl_list_range_expand(PerlValue::string("a".into()), PerlValue::string("z".into()));
        let s: String = v.iter().map(|x| x.to_string()).collect();
        assert_eq!(s, "abcdefghijklmnopqrstuvwxyz");
    }

    #[test]
    fn list_range_numeric_string_endpoints() {
        use super::perl_list_range_expand;
        let v = perl_list_range_expand(
            PerlValue::string("9".into()),
            PerlValue::string("11".into()),
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
            PerlValue::string("01".into()),
            PerlValue::string("05".into()),
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
            PerlValue::string(String::new()),
            PerlValue::string("c".into()),
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
        let v_int = PerlValue::integer(large_int);
        assert_eq!(v_int.to_string(), "10000000000");

        // Float that needs boxing (e.g. Infinity)
        let v_inf = PerlValue::float(f64::INFINITY);
        assert_eq!(v_inf.to_string(), "inf");
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
