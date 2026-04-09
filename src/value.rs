use crossbeam::channel::{Receiver, Sender};
use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::sync::Barrier;

use crate::ast::{Block, StructDef};
use crate::error::PerlResult;

/// Handle returned by `async { ... }`; join with `await`.
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
            .unwrap_or_else(|| Ok(PerlValue::Undef))
    }
}

/// `Set->new` storage: canonical key → member value (insertion order preserved).
pub type PerlSet = IndexMap<String, PerlValue>;

/// Min-heap ordered by a Perl comparator (`$a` / `$b` in scope, like `sort { }`).
#[derive(Debug, Clone)]
pub struct PerlHeap {
    pub items: Vec<PerlValue>,
    pub cmp: Arc<PerlSub>,
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

/// Core Perl value type. Clone-cheap via Arc for references.
#[derive(Debug, Clone, Default)]
pub enum PerlValue {
    #[default]
    Undef,
    Integer(i64),
    Float(f64),
    String(String),
    /// Raw bytes from `pack` / binary I/O (not guaranteed UTF-8).
    Bytes(Arc<Vec<u8>>),
    Array(Vec<PerlValue>),
    Hash(IndexMap<String, PerlValue>),
    ArrayRef(Arc<RwLock<Vec<PerlValue>>>),
    HashRef(Arc<RwLock<IndexMap<String, PerlValue>>>),
    ScalarRef(Arc<RwLock<PerlValue>>),
    CodeRef(Arc<PerlSub>),
    Regex(Arc<regex::Regex>, String),
    Blessed(Arc<BlessedRef>),
    /// File handle (wraps an index into the interpreter's handle table)
    IOHandle(String),
    /// Thread-safe atomic variable created by `mysync`.
    /// Reads/writes go through the Mutex. Cloning shares the same lock
    /// so parallel blocks (fan/pmap/pfor) see the same storage.
    Atomic(Arc<Mutex<PerlValue>>),
    /// Native set from `Set->new(...)`; `|` is union, `&` is intersection when both operands are sets.
    Set(Arc<PerlSet>),
    /// `pchannel()` sender.
    ChannelTx(Arc<Sender<PerlValue>>),
    /// `pchannel()` receiver.
    ChannelRx(Arc<Receiver<PerlValue>>),
    /// Task from `async { BLOCK }` — join with `await`.
    AsyncTask(Arc<PerlAsyncTask>),
    /// `deque()` — double-ended queue.
    Deque(Arc<Mutex<VecDeque<PerlValue>>>),
    /// `heap(sub { $a <=> $b })` — priority queue.
    Heap(Arc<Mutex<PerlHeap>>),
    /// Lazy iterator pipeline: `pipeline(...)->filter(...)->map(...)->collect()`.
    Pipeline(Arc<Mutex<PipelineInner>>),
    /// `capture("cmd")` — run via `sh -c`; inspect with `->stdout`, `->exitcode`, `->failed`.
    Capture(Arc<CaptureResult>),
    /// `ppool(N)` — persistent worker threads with `submit` / `collect`.
    Ppool(PerlPpool),
    /// `barrier(N)` — thread barrier (`->wait`).
    Barrier(PerlBarrier),
    /// `sqlite("path")` — embedded SQLite (`rusqlite`); use `->exec`, `->query`, `->last_insert_rowid`.
    SqliteConn(Arc<Mutex<rusqlite::Connection>>),
    /// `struct`-defined record instance (`Point->new`, `$p->x`).
    StructInst(Arc<StructInstance>),
}

/// Handle returned by `ppool(N)`; use `->submit(sub { ... })` and `->collect()`.
#[derive(Clone)]
pub struct PerlPpool(pub(crate) Arc<crate::ppool::PpoolInner>);

impl fmt::Debug for PerlPpool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PerlPpool")
    }
}

#[derive(Debug, Clone)]
pub struct PerlSub {
    pub name: String,
    pub params: Vec<String>,
    pub body: Block,
    /// Captured lexical scope (for closures)
    pub closure_env: Option<Vec<(String, PerlValue)>>,
    /// Prototype string from `sub name (PROTO) { }`, or `None`.
    pub prototype: Option<String>,
}

/// Operations queued on a [`PerlValue::Pipeline`] until `collect()`.
#[derive(Debug, Clone)]
pub enum PipelineOp {
    Filter(Arc<PerlSub>),
    Map(Arc<PerlSub>),
    Take(i64),
}

#[derive(Debug)]
pub struct PipelineInner {
    pub source: Vec<PerlValue>,
    pub ops: Vec<PipelineOp>,
}

#[derive(Debug)]
pub struct BlessedRef {
    pub class: String,
    pub data: RwLock<PerlValue>,
}

impl Clone for BlessedRef {
    fn clone(&self) -> Self {
        Self {
            class: self.class.clone(),
            data: RwLock::new(self.data.read().clone()),
        }
    }
}

/// Instance of a `struct Name { ... }` definition; field access via `$obj->name`.
#[derive(Debug)]
pub struct StructInstance {
    pub def: Arc<StructDef>,
    pub values: Vec<PerlValue>,
}

impl Clone for StructInstance {
    fn clone(&self) -> Self {
        Self {
            def: Arc::clone(&self.def),
            values: self.values.clone(),
        }
    }
}

impl PerlValue {
    /// Borrow the inner string without allocation if this is a String variant.
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PerlValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Append this value's string representation to `buf` without allocating a new String.
    #[inline]
    pub fn append_to(&self, buf: &mut String) {
        match self {
            PerlValue::Undef => {}
            PerlValue::Integer(n) => {
                let mut b = itoa::Buffer::new();
                buf.push_str(b.format(*n));
            }
            PerlValue::String(s) => buf.push_str(s),
            PerlValue::Bytes(b) => buf.push_str(&String::from_utf8_lossy(b)),
            PerlValue::Atomic(arc) => arc.lock().append_to(buf),
            PerlValue::Set(s) => {
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
            PerlValue::ChannelTx(_) => buf.push_str("PCHANNEL::Tx"),
            PerlValue::ChannelRx(_) => buf.push_str("PCHANNEL::Rx"),
            PerlValue::AsyncTask(_) => buf.push_str("AsyncTask"),
            PerlValue::Pipeline(_) => buf.push_str("Pipeline"),
            PerlValue::Capture(_) => buf.push_str("Capture"),
            PerlValue::Ppool(_) => buf.push_str("Ppool"),
            PerlValue::Barrier(_) => buf.push_str("Barrier"),
            PerlValue::SqliteConn(_) => buf.push_str("SqliteConn"),
            PerlValue::StructInst(s) => {
                buf.push_str(&s.def.name);
            }
            other => buf.push_str(&other.to_string()),
        }
    }

    /// Unwrap Atomic transparently — returns the inner value (cloned).
    #[inline]
    pub fn unwrap_atomic(&self) -> PerlValue {
        match self {
            PerlValue::Atomic(arc) => arc.lock().clone(),
            other => other.clone(),
        }
    }

    /// Check if this is an Atomic wrapper.
    #[inline]
    pub fn is_atomic(&self) -> bool {
        matches!(self, PerlValue::Atomic(_))
    }

    // ── Truthiness (Perl rules) ──

    #[inline]
    pub fn is_true(&self) -> bool {
        match self {
            PerlValue::Undef => false,
            PerlValue::Integer(n) => *n != 0,
            PerlValue::Float(f) => *f != 0.0,
            PerlValue::String(s) => !s.is_empty() && s != "0",
            PerlValue::Bytes(b) => !b.is_empty(),
            PerlValue::Array(a) => !a.is_empty(),
            PerlValue::Hash(h) => !h.is_empty(),
            PerlValue::Atomic(arc) => arc.lock().is_true(),
            PerlValue::Set(s) => !s.is_empty(),
            PerlValue::Deque(d) => !d.lock().is_empty(),
            PerlValue::Heap(h) => !h.lock().items.is_empty(),
            PerlValue::Pipeline(_) => true,
            PerlValue::Capture(_) => true,
            _ => true,
        }
    }

    // ── String coercion (zero-copy) ──

    /// Move the inner `String` out of a `PerlValue::String`, avoiding the
    /// allocation that `.to_string()` (Display) would trigger.
    #[inline]
    pub fn into_string(self) -> String {
        match self {
            PerlValue::String(s) => s,
            PerlValue::Integer(n) => {
                let mut buf = itoa::Buffer::new();
                buf.format(n).to_owned()
            }
            other => other.to_string(),
        }
    }

    /// Borrow the inner `&str` of a `PerlValue::String`. Returns `""` for
    /// non-string variants (cheap default for bytecode pattern/flag constants).
    #[inline]
    pub fn as_str_or_empty(&self) -> &str {
        match self {
            PerlValue::String(s) => s.as_str(),
            _ => "",
        }
    }

    // ── Numeric coercion ──

    #[inline]
    pub fn to_number(&self) -> f64 {
        match self {
            PerlValue::Undef => 0.0,
            PerlValue::Integer(n) => *n as f64,
            PerlValue::Float(f) => *f,
            PerlValue::String(s) => parse_number(s),
            PerlValue::Bytes(b) => b.len() as f64,
            PerlValue::Array(a) => a.len() as f64,
            PerlValue::Atomic(arc) => arc.lock().to_number(),
            PerlValue::Set(s) => s.len() as f64,
            PerlValue::ChannelTx(_) | PerlValue::ChannelRx(_) | PerlValue::AsyncTask(_) => 1.0,
            PerlValue::Deque(d) => d.lock().len() as f64,
            PerlValue::Heap(h) => h.lock().items.len() as f64,
            PerlValue::Pipeline(p) => p.lock().source.len() as f64,
            PerlValue::Capture(_) => 1.0,
            PerlValue::Ppool(_) => 1.0,
            PerlValue::Barrier(_) => 1.0,
            PerlValue::SqliteConn(_) => 1.0,
            PerlValue::StructInst(_) => 1.0,
            _ => 0.0,
        }
    }

    #[inline]
    pub fn to_int(&self) -> i64 {
        match self {
            PerlValue::Undef => 0,
            PerlValue::Integer(n) => *n,
            PerlValue::Float(f) => *f as i64,
            PerlValue::String(s) => parse_number(s) as i64,
            PerlValue::Bytes(b) => b.len() as i64,
            PerlValue::Array(a) => a.len() as i64,
            PerlValue::Atomic(arc) => arc.lock().to_int(),
            PerlValue::Set(s) => s.len() as i64,
            PerlValue::ChannelTx(_) | PerlValue::ChannelRx(_) | PerlValue::AsyncTask(_) => 1,
            PerlValue::Deque(d) => d.lock().len() as i64,
            PerlValue::Heap(h) => h.lock().items.len() as i64,
            PerlValue::Pipeline(p) => p.lock().source.len() as i64,
            PerlValue::Capture(_) => 1,
            PerlValue::Ppool(_) => 1,
            PerlValue::Barrier(_) => 1,
            PerlValue::SqliteConn(_) => 1,
            PerlValue::StructInst(_) => 1,
            _ => 0,
        }
    }

    // ── Type checks ──

    pub fn type_name(&self) -> &str {
        match self {
            PerlValue::Undef => "undef",
            PerlValue::Integer(_) => "INTEGER",
            PerlValue::Float(_) => "FLOAT",
            PerlValue::String(_) => "STRING",
            PerlValue::Bytes(_) => "BYTES",
            PerlValue::Array(_) => "ARRAY",
            PerlValue::Hash(_) => "HASH",
            PerlValue::ArrayRef(_) => "ARRAY",
            PerlValue::HashRef(_) => "HASH",
            PerlValue::ScalarRef(_) => "SCALAR",
            PerlValue::CodeRef(_) => "CODE",
            PerlValue::Regex(_, _) => "Regexp",
            PerlValue::Blessed(b) => &b.class,
            PerlValue::IOHandle(_) => "GLOB",
            PerlValue::Atomic(_) => "ATOMIC",
            PerlValue::Set(_) => "Set",
            PerlValue::ChannelTx(_) => "PCHANNEL::Tx",
            PerlValue::ChannelRx(_) => "PCHANNEL::Rx",
            PerlValue::AsyncTask(_) => "ASYNCTASK",
            PerlValue::Deque(_) => "Deque",
            PerlValue::Heap(_) => "Heap",
            PerlValue::Pipeline(_) => "Pipeline",
            PerlValue::Capture(_) => "Capture",
            PerlValue::Ppool(_) => "Ppool",
            PerlValue::Barrier(_) => "Barrier",
            PerlValue::SqliteConn(_) => "SqliteConn",
            PerlValue::StructInst(s) => s.def.name.as_str(),
        }
    }

    pub fn ref_type(&self) -> PerlValue {
        match self {
            PerlValue::ArrayRef(_) => PerlValue::String("ARRAY".into()),
            PerlValue::HashRef(_) => PerlValue::String("HASH".into()),
            PerlValue::ScalarRef(_) => PerlValue::String("SCALAR".into()),
            PerlValue::CodeRef(_) => PerlValue::String("CODE".into()),
            PerlValue::Regex(_, _) => PerlValue::String("Regexp".into()),
            PerlValue::Atomic(_) => PerlValue::String("ATOMIC".into()),
            PerlValue::Set(_) => PerlValue::String("Set".into()),
            PerlValue::ChannelTx(_) => PerlValue::String("PCHANNEL::Tx".into()),
            PerlValue::ChannelRx(_) => PerlValue::String("PCHANNEL::Rx".into()),
            PerlValue::AsyncTask(_) => PerlValue::String("ASYNCTASK".into()),
            PerlValue::Deque(_) => PerlValue::String("Deque".into()),
            PerlValue::Heap(_) => PerlValue::String("Heap".into()),
            PerlValue::Pipeline(_) => PerlValue::String("Pipeline".into()),
            PerlValue::Capture(_) => PerlValue::String("Capture".into()),
            PerlValue::Ppool(_) => PerlValue::String("Ppool".into()),
            PerlValue::Barrier(_) => PerlValue::String("Barrier".into()),
            PerlValue::SqliteConn(_) => PerlValue::String("SqliteConn".into()),
            PerlValue::StructInst(s) => PerlValue::String(s.def.name.clone()),
            PerlValue::Bytes(_) => PerlValue::String("BYTES".into()),
            PerlValue::Blessed(b) => PerlValue::String(b.class.clone()),
            _ => PerlValue::String(String::new()),
        }
    }

    // ── Comparison ──

    pub fn num_cmp(&self, other: &PerlValue) -> Ordering {
        let a = self.to_number();
        let b = other.to_number();
        a.partial_cmp(&b).unwrap_or(Ordering::Equal)
    }

    pub fn str_cmp(&self, other: &PerlValue) -> Ordering {
        self.to_string().cmp(&other.to_string())
    }

    /// Return the value as a list (flatten arrays, hash to kv pairs).
    pub fn to_list(&self) -> Vec<PerlValue> {
        match self {
            PerlValue::Array(a) => a.clone(),
            PerlValue::Hash(h) => h
                .iter()
                .flat_map(|(k, v)| vec![PerlValue::String(k.clone()), v.clone()])
                .collect(),
            PerlValue::Undef => vec![],
            PerlValue::Atomic(arc) => arc.lock().to_list(),
            PerlValue::Set(s) => s.values().cloned().collect(),
            PerlValue::Deque(d) => d.lock().iter().cloned().collect(),
            other => vec![other.clone()],
        }
    }

    /// Scalar context: arrays → length, hashes → "n/m" string.
    pub fn scalar_context(&self) -> PerlValue {
        match self {
            PerlValue::Array(a) => PerlValue::Integer(a.len() as i64),
            PerlValue::Hash(h) => {
                if h.is_empty() {
                    PerlValue::Integer(0)
                } else {
                    PerlValue::String(format!("{}/{}", h.len(), h.capacity()))
                }
            }
            PerlValue::Set(s) => PerlValue::Integer(s.len() as i64),
            PerlValue::Deque(d) => PerlValue::Integer(d.lock().len() as i64),
            PerlValue::Heap(h) => PerlValue::Integer(h.lock().items.len() as i64),
            PerlValue::Pipeline(p) => PerlValue::Integer(p.lock().source.len() as i64),
            PerlValue::Capture(_) => PerlValue::Integer(1),
            PerlValue::Ppool(_) => PerlValue::Integer(1),
            PerlValue::Barrier(_) => PerlValue::Integer(1),
            other => other.clone(),
        }
    }
}

impl fmt::Display for PerlValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PerlValue::Undef => Ok(()),
            PerlValue::Integer(n) => write!(f, "{n}"),
            PerlValue::Float(fl) => write!(f, "{}", format_float(*fl)),
            PerlValue::String(s) => f.write_str(s),
            PerlValue::Bytes(b) => f.write_str(&String::from_utf8_lossy(b)),
            PerlValue::Array(a) => {
                for v in a {
                    write!(f, "{v}")?;
                }
                Ok(())
            }
            PerlValue::Hash(h) => write!(f, "{}/{}", h.len(), h.capacity()),
            PerlValue::ArrayRef(_) => f.write_str("ARRAY(0x...)"),
            PerlValue::HashRef(_) => f.write_str("HASH(0x...)"),
            PerlValue::ScalarRef(_) => f.write_str("SCALAR(0x...)"),
            PerlValue::CodeRef(sub) => write!(f, "CODE({})", sub.name),
            PerlValue::Regex(_, src) => write!(f, "(?:{src})"),
            PerlValue::Blessed(b) => write!(f, "{}=HASH(0x...)", b.class),
            PerlValue::IOHandle(name) => f.write_str(name),
            PerlValue::Atomic(arc) => write!(f, "{}", arc.lock()),
            PerlValue::Set(s) => {
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
            PerlValue::ChannelTx(_) => f.write_str("PCHANNEL::Tx"),
            PerlValue::ChannelRx(_) => f.write_str("PCHANNEL::Rx"),
            PerlValue::AsyncTask(_) => f.write_str("AsyncTask"),
            PerlValue::Deque(d) => write!(f, "Deque({})", d.lock().len()),
            PerlValue::Heap(h) => write!(f, "Heap({})", h.lock().items.len()),
            PerlValue::Pipeline(p) => {
                let g = p.lock();
                write!(f, "Pipeline({} ops)", g.ops.len())
            }
            PerlValue::Capture(c) => write!(f, "Capture(exit={})", c.exitcode),
            PerlValue::Ppool(_) => f.write_str("Ppool"),
            PerlValue::Barrier(_) => f.write_str("Barrier"),
            PerlValue::SqliteConn(_) => f.write_str("SqliteConn"),
            PerlValue::StructInst(s) => write!(f, "{}=STRUCT(...)", s.def.name),
        }
    }
}

/// Stable key for set membership (dedup of `PerlValue` in this runtime).
pub fn set_member_key(v: &PerlValue) -> String {
    match v {
        PerlValue::Undef => "u:".to_string(),
        PerlValue::Integer(n) => format!("i:{n}"),
        PerlValue::Float(f) => format!("f:{}", f.to_bits()),
        PerlValue::String(s) => format!("s:{s}"),
        PerlValue::Bytes(b) => {
            use std::fmt::Write as _;
            let mut h = String::with_capacity(b.len() * 2);
            for &x in b.iter() {
                let _ = write!(&mut h, "{:02x}", x);
            }
            format!("by:{h}")
        }
        PerlValue::Array(a) => {
            let parts: Vec<_> = a.iter().map(set_member_key).collect();
            format!("a:{}", parts.join(","))
        }
        PerlValue::Hash(h) => {
            let mut keys: Vec<_> = h.keys().cloned().collect();
            keys.sort();
            let parts: Vec<_> = keys
                .iter()
                .map(|k| format!("{}={}", k, set_member_key(h.get(k).unwrap())))
                .collect();
            format!("h:{}", parts.join(","))
        }
        PerlValue::Set(inner) => {
            let mut keys: Vec<_> = inner.keys().cloned().collect();
            keys.sort();
            format!("S:{}", keys.join(","))
        }
        PerlValue::ArrayRef(a) => {
            let g = a.read();
            let parts: Vec<_> = g.iter().map(set_member_key).collect();
            format!("ar:{}", parts.join(","))
        }
        PerlValue::HashRef(h) => {
            let g = h.read();
            let mut keys: Vec<_> = g.keys().cloned().collect();
            keys.sort();
            let parts: Vec<_> = keys
                .iter()
                .map(|k| format!("{}={}", k, set_member_key(g.get(k).unwrap())))
                .collect();
            format!("hr:{}", parts.join(","))
        }
        PerlValue::Blessed(b) => {
            let d = b.data.read();
            format!("b:{}:{}", b.class, set_member_key(&d))
        }
        PerlValue::ScalarRef(_) => format!("sr:{v}"),
        PerlValue::CodeRef(_) => format!("c:{v}"),
        PerlValue::Regex(_, src) => format!("r:{src}"),
        PerlValue::IOHandle(s) => format!("io:{s}"),
        PerlValue::Atomic(arc) => format!("at:{}", set_member_key(&arc.lock())),
        PerlValue::ChannelTx(tx) => format!("chtx:{:p}", Arc::as_ptr(tx)),
        PerlValue::ChannelRx(rx) => format!("chrx:{:p}", Arc::as_ptr(rx)),
        PerlValue::AsyncTask(t) => format!("async:{:p}", Arc::as_ptr(t)),
        PerlValue::Deque(d) => format!("dq:{:p}", Arc::as_ptr(d)),
        PerlValue::Heap(h) => format!("hp:{:p}", Arc::as_ptr(h)),
        PerlValue::Pipeline(p) => format!("pl:{:p}", Arc::as_ptr(p)),
        PerlValue::Capture(c) => format!("cap:{:p}", Arc::as_ptr(c)),
        PerlValue::Ppool(p) => format!("pp:{:p}", Arc::as_ptr(&p.0)),
        PerlValue::Barrier(b) => format!("br:{:p}", Arc::as_ptr(&b.0)),
        PerlValue::SqliteConn(c) => format!("sql:{:p}", Arc::as_ptr(c)),
        PerlValue::StructInst(s) => format!("st:{}:{:?}", s.def.name, s.values),
    }
}

pub fn set_from_elements<I: IntoIterator<Item = PerlValue>>(items: I) -> PerlValue {
    let mut map = PerlSet::new();
    for v in items {
        let k = set_member_key(&v);
        map.insert(k, v);
    }
    PerlValue::Set(Arc::new(map))
}

/// Underlying set for union/intersection, including `mysync $s` (`Atomic` wrapping `Set`).
#[inline]
pub fn set_payload(v: &PerlValue) -> Option<Arc<PerlSet>> {
    match v {
        PerlValue::Set(s) => Some(Arc::clone(s)),
        PerlValue::Atomic(arc) => match &*arc.lock() {
            PerlValue::Set(s) => Some(Arc::clone(s)),
            _ => None,
        },
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
    Some(PerlValue::Set(Arc::new(m)))
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
    Some(PerlValue::Set(Arc::new(m)))
}

// ── Helpers ──

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
        // Perl uses %g-like formatting
        let s = format!("{}", f);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::PerlValue;
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::cmp::Ordering;
    use std::sync::Arc;

    #[test]
    fn undef_is_false() {
        assert!(!PerlValue::Undef.is_true());
    }

    #[test]
    fn string_zero_is_false() {
        assert!(!PerlValue::String("0".into()).is_true());
        assert!(PerlValue::String("00".into()).is_true());
    }

    #[test]
    fn empty_string_is_false() {
        assert!(!PerlValue::String(String::new()).is_true());
    }

    #[test]
    fn integer_zero_is_false_nonzero_true() {
        assert!(!PerlValue::Integer(0).is_true());
        assert!(PerlValue::Integer(-1).is_true());
    }

    #[test]
    fn float_zero_is_false_nonzero_true() {
        assert!(!PerlValue::Float(0.0).is_true());
        assert!(PerlValue::Float(0.1).is_true());
    }

    #[test]
    fn num_cmp_orders_float_against_integer() {
        assert_eq!(
            PerlValue::Float(2.5).num_cmp(&PerlValue::Integer(3)),
            Ordering::Less
        );
    }

    #[test]
    fn to_int_parses_leading_number_from_string() {
        assert_eq!(PerlValue::String("42xyz".into()).to_int(), 42);
        assert_eq!(PerlValue::String("  -3.7foo".into()).to_int(), -3);
    }

    #[test]
    fn num_cmp_orders_as_numeric() {
        assert_eq!(
            PerlValue::Integer(2).num_cmp(&PerlValue::Integer(11)),
            Ordering::Less
        );
        assert_eq!(
            PerlValue::String("2foo".into()).num_cmp(&PerlValue::String("11".into())),
            Ordering::Less
        );
    }

    #[test]
    fn str_cmp_orders_as_strings() {
        assert_eq!(
            PerlValue::String("2".into()).str_cmp(&PerlValue::String("11".into())),
            Ordering::Greater
        );
    }

    #[test]
    fn scalar_context_array_and_hash() {
        let a =
            PerlValue::Array(vec![PerlValue::Integer(1), PerlValue::Integer(2)]).scalar_context();
        assert!(matches!(a, PerlValue::Integer(2)));
        let mut h = IndexMap::new();
        h.insert("a".into(), PerlValue::Integer(1));
        let sc = PerlValue::Hash(h).scalar_context();
        assert!(matches!(sc, PerlValue::String(_)));
    }

    #[test]
    fn to_list_array_hash_and_scalar() {
        assert_eq!(
            PerlValue::Array(vec![PerlValue::Integer(7)])
                .to_list()
                .len(),
            1
        );
        let mut h = IndexMap::new();
        h.insert("k".into(), PerlValue::Integer(1));
        let list = PerlValue::Hash(h).to_list();
        assert_eq!(list.len(), 2);
        let one = PerlValue::Integer(99).to_list();
        assert_eq!(one.len(), 1);
        assert!(matches!(one[0], PerlValue::Integer(99)));
    }

    #[test]
    fn type_name_and_ref_type_for_core_kinds() {
        assert_eq!(PerlValue::Integer(0).type_name(), "INTEGER");
        assert_eq!(PerlValue::Undef.ref_type().to_string(), "");
        assert_eq!(
            PerlValue::ArrayRef(Arc::new(RwLock::new(vec![])))
                .ref_type()
                .to_string(),
            "ARRAY"
        );
    }

    #[test]
    fn display_undef_is_empty_integer_is_decimal() {
        assert_eq!(PerlValue::Undef.to_string(), "");
        assert_eq!(PerlValue::Integer(-7).to_string(), "-7");
    }

    #[test]
    fn empty_array_is_false_nonempty_is_true() {
        assert!(!PerlValue::Array(vec![]).is_true());
        assert!(PerlValue::Array(vec![PerlValue::Integer(0)]).is_true());
    }

    #[test]
    fn to_number_undef_and_non_numeric_refs_are_zero() {
        use super::PerlSub;

        assert_eq!(PerlValue::Undef.to_number(), 0.0);
        assert_eq!(
            PerlValue::CodeRef(Arc::new(PerlSub {
                name: "f".into(),
                params: vec![],
                body: vec![],
                closure_env: None,
                prototype: None,
            }))
            .to_number(),
            0.0
        );
    }

    #[test]
    fn append_to_builds_string_without_extra_alloc_for_int_and_string() {
        let mut buf = String::new();
        PerlValue::Integer(-12).append_to(&mut buf);
        PerlValue::String("ab".into()).append_to(&mut buf);
        assert_eq!(buf, "-12ab");
        let mut u = String::new();
        PerlValue::Undef.append_to(&mut u);
        assert!(u.is_empty());
    }

    #[test]
    fn append_to_atomic_delegates_to_inner() {
        use parking_lot::Mutex;
        let a = PerlValue::Atomic(Arc::new(Mutex::new(PerlValue::String("z".into()))));
        let mut buf = String::new();
        a.append_to(&mut buf);
        assert_eq!(buf, "z");
    }

    #[test]
    fn unwrap_atomic_reads_inner_other_variants_clone() {
        use parking_lot::Mutex;
        let a = PerlValue::Atomic(Arc::new(Mutex::new(PerlValue::Integer(9))));
        assert_eq!(a.unwrap_atomic().to_int(), 9);
        assert_eq!(PerlValue::Integer(3).unwrap_atomic().to_int(), 3);
    }

    #[test]
    fn is_atomic_only_true_for_atomic_variant() {
        use parking_lot::Mutex;
        assert!(PerlValue::Atomic(Arc::new(Mutex::new(PerlValue::Undef))).is_atomic());
        assert!(!PerlValue::Integer(0).is_atomic());
    }

    #[test]
    fn as_str_only_on_string_variant() {
        assert_eq!(PerlValue::String("x".into()).as_str(), Some("x"));
        assert_eq!(PerlValue::Integer(1).as_str(), None);
    }

    #[test]
    fn as_str_or_empty_defaults_non_string() {
        assert_eq!(PerlValue::String("z".into()).as_str_or_empty(), "z");
        assert_eq!(PerlValue::Integer(1).as_str_or_empty(), "");
    }

    #[test]
    fn to_int_truncates_float_toward_zero() {
        assert_eq!(PerlValue::Float(3.9).to_int(), 3);
        assert_eq!(PerlValue::Float(-2.1).to_int(), -2);
    }

    #[test]
    fn to_number_array_is_length() {
        assert_eq!(
            PerlValue::Array(vec![PerlValue::Integer(1), PerlValue::Integer(2)]).to_number(),
            2.0
        );
    }

    #[test]
    fn scalar_context_empty_hash_is_zero() {
        let h = IndexMap::new();
        assert_eq!(PerlValue::Hash(h).scalar_context().to_int(), 0);
    }

    #[test]
    fn scalar_context_nonhash_nonarray_clones() {
        let v = PerlValue::Integer(8);
        assert_eq!(v.scalar_context().to_int(), 8);
    }

    #[test]
    fn display_float_integer_like_omits_decimal() {
        assert_eq!(PerlValue::Float(4.0).to_string(), "4");
    }

    #[test]
    fn display_array_concatenates_element_displays() {
        let a = PerlValue::Array(vec![PerlValue::Integer(1), PerlValue::String("b".into())]);
        assert_eq!(a.to_string(), "1b");
    }

    #[test]
    fn display_code_ref_includes_sub_name() {
        use super::PerlSub;
        let c = PerlValue::CodeRef(Arc::new(PerlSub {
            name: "foo".into(),
            params: vec![],
            body: vec![],
            closure_env: None,
            prototype: None,
        }));
        assert!(c.to_string().contains("foo"));
    }

    #[test]
    fn display_regex_shows_non_capturing_prefix() {
        use regex::Regex;
        let r = PerlValue::Regex(Arc::new(Regex::new("x+").unwrap()), "x+".into());
        assert_eq!(r.to_string(), "(?:x+)");
    }

    #[test]
    fn display_iohandle_is_name() {
        assert_eq!(PerlValue::IOHandle("STDOUT".into()).to_string(), "STDOUT");
    }

    #[test]
    fn ref_type_blessed_uses_class_name() {
        let b = PerlValue::Blessed(Arc::new(super::BlessedRef {
            class: "Pkg".into(),
            data: RwLock::new(PerlValue::Undef),
        }));
        assert_eq!(b.ref_type().to_string(), "Pkg");
    }

    #[test]
    fn type_name_iohandle_is_glob() {
        assert_eq!(PerlValue::IOHandle("FH".into()).type_name(), "GLOB");
    }

    #[test]
    fn empty_hash_is_false() {
        assert!(!PerlValue::Hash(IndexMap::new()).is_true());
    }

    #[test]
    fn hash_nonempty_is_true() {
        let mut h = IndexMap::new();
        h.insert("k".into(), PerlValue::Undef);
        assert!(PerlValue::Hash(h).is_true());
    }

    #[test]
    fn num_cmp_equal_integers() {
        assert_eq!(
            PerlValue::Integer(5).num_cmp(&PerlValue::Integer(5)),
            Ordering::Equal
        );
    }

    #[test]
    fn str_cmp_compares_lexicographic_string_forms() {
        // Display forms "2" and "10" — string order differs from numeric order.
        assert_eq!(
            PerlValue::Integer(2).str_cmp(&PerlValue::Integer(10)),
            Ordering::Greater
        );
    }

    #[test]
    fn to_list_undef_empty() {
        assert!(PerlValue::Undef.to_list().is_empty());
    }

    #[test]
    fn unwrap_atomic_nested_atomic() {
        use parking_lot::Mutex;
        let inner = PerlValue::Atomic(Arc::new(Mutex::new(PerlValue::Integer(2))));
        let outer = PerlValue::Atomic(Arc::new(Mutex::new(inner)));
        assert_eq!(outer.unwrap_atomic().to_int(), 2);
    }
}
