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
use crate::nanbox;

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
            .unwrap_or_else(|| Ok(PerlValue::UNDEF))
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
    CodeRef(Arc<PerlSub>),
    Regex(Arc<regex::Regex>, String),
    Blessed(Arc<BlessedRef>),
    IOHandle(String),
    Atomic(Arc<Mutex<PerlValue>>),
    Set(Arc<PerlSet>),
    ChannelTx(Arc<Sender<PerlValue>>),
    ChannelRx(Arc<Receiver<PerlValue>>),
    AsyncTask(Arc<PerlAsyncTask>),
    Deque(Arc<Mutex<VecDeque<PerlValue>>>),
    Heap(Arc<Mutex<PerlHeap>>),
    Pipeline(Arc<Mutex<PipelineInner>>),
    Capture(Arc<CaptureResult>),
    Ppool(PerlPpool),
    Barrier(PerlBarrier),
    SqliteConn(Arc<Mutex<rusqlite::Connection>>),
    StructInst(Arc<StructInstance>),
    DataFrame(Arc<Mutex<PerlDataFrame>>),
    /// Numeric/string dualvar: **`$!`** (errno + message) and **`$@`** (numeric flag or code + message).
    ErrnoDual { code: i32, msg: String },
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
            let p = nanbox::decode_heap_ptr::<HeapObject>(self.0) as *const HeapObject;
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
    pub fn as_regex(&self) -> Option<Arc<regex::Regex>> {
        self.with_heap(|h| match h {
            HeapObject::Regex(re, _) => Some(Arc::clone(re)),
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
    pub fn regex(rx: Arc<regex::Regex>, src: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::Regex(rx, src)))
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
            HeapObject::Bytes(b) => buf.push_str(&String::from_utf8_lossy(b)),
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
            HeapObject::Pipeline(_) => buf.push_str("Pipeline"),
            HeapObject::DataFrame(d) => {
                let g = d.lock();
                buf.push_str(&format!("DataFrame({}x{})", g.nrows(), g.ncols()));
            }
            HeapObject::Capture(_) => buf.push_str("Capture"),
            HeapObject::Ppool(_) => buf.push_str("Ppool"),
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
            HeapObject::ErrnoDual { code, .. } => *code as f64,
            HeapObject::String(s) => parse_number(s),
            HeapObject::Bytes(b) => b.len() as f64,
            HeapObject::Array(a) => a.len() as f64,
            HeapObject::Atomic(arc) => arc.lock().to_number(),
            HeapObject::Set(s) => s.len() as f64,
            HeapObject::ChannelTx(_) | HeapObject::ChannelRx(_) | HeapObject::AsyncTask(_) => 1.0,
            HeapObject::Deque(d) => d.lock().len() as f64,
            HeapObject::Heap(h) => h.lock().items.len() as f64,
            HeapObject::Pipeline(p) => p.lock().source.len() as f64,
            HeapObject::DataFrame(d) => d.lock().nrows() as f64,
            HeapObject::Capture(_)
            | HeapObject::Ppool(_)
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
            HeapObject::ErrnoDual { code, .. } => *code as i64,
            HeapObject::String(s) => parse_number(s) as i64,
            HeapObject::Bytes(b) => b.len() as i64,
            HeapObject::Array(a) => a.len() as i64,
            HeapObject::Atomic(arc) => arc.lock().to_int(),
            HeapObject::Set(s) => s.len() as i64,
            HeapObject::ChannelTx(_) | HeapObject::ChannelRx(_) | HeapObject::AsyncTask(_) => 1,
            HeapObject::Deque(d) => d.lock().len() as i64,
            HeapObject::Heap(h) => h.lock().items.len() as i64,
            HeapObject::Pipeline(p) => p.lock().source.len() as i64,
            HeapObject::DataFrame(d) => d.lock().nrows() as i64,
            HeapObject::Capture(_)
            | HeapObject::Ppool(_)
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
            HeapObject::ArrayRef(_) => "ARRAY".to_string(),
            HeapObject::HashRef(_) => "HASH".to_string(),
            HeapObject::ScalarRef(_) => "SCALAR".to_string(),
            HeapObject::CodeRef(_) => "CODE".to_string(),
            HeapObject::Regex(_, _) => "Regexp".to_string(),
            HeapObject::Blessed(b) => b.class.clone(),
            HeapObject::IOHandle(_) => "GLOB".to_string(),
            HeapObject::Atomic(_) => "ATOMIC".to_string(),
            HeapObject::Set(_) => "Set".to_string(),
            HeapObject::ChannelTx(_) => "PCHANNEL::Tx".to_string(),
            HeapObject::ChannelRx(_) => "PCHANNEL::Rx".to_string(),
            HeapObject::AsyncTask(_) => "ASYNCTASK".to_string(),
            HeapObject::Deque(_) => "Deque".to_string(),
            HeapObject::Heap(_) => "Heap".to_string(),
            HeapObject::Pipeline(_) => "Pipeline".to_string(),
            HeapObject::DataFrame(_) => "DataFrame".to_string(),
            HeapObject::Capture(_) => "Capture".to_string(),
            HeapObject::Ppool(_) => "Ppool".to_string(),
            HeapObject::Barrier(_) => "Barrier".to_string(),
            HeapObject::SqliteConn(_) => "SqliteConn".to_string(),
            HeapObject::StructInst(s) => s.def.name.to_string(),
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
            HeapObject::ArrayRef(_) => PerlValue::string("ARRAY".into()),
            HeapObject::HashRef(_) => PerlValue::string("HASH".into()),
            HeapObject::ScalarRef(_) => PerlValue::string("SCALAR".into()),
            HeapObject::CodeRef(_) => PerlValue::string("CODE".into()),
            HeapObject::Regex(_, _) => PerlValue::string("Regexp".into()),
            HeapObject::Atomic(_) => PerlValue::string("ATOMIC".into()),
            HeapObject::Set(_) => PerlValue::string("Set".into()),
            HeapObject::ChannelTx(_) => PerlValue::string("PCHANNEL::Tx".into()),
            HeapObject::ChannelRx(_) => PerlValue::string("PCHANNEL::Rx".into()),
            HeapObject::AsyncTask(_) => PerlValue::string("ASYNCTASK".into()),
            HeapObject::Deque(_) => PerlValue::string("Deque".into()),
            HeapObject::Heap(_) => PerlValue::string("Heap".into()),
            HeapObject::Pipeline(_) => PerlValue::string("Pipeline".into()),
            HeapObject::DataFrame(_) => PerlValue::string("DataFrame".into()),
            HeapObject::Capture(_) => PerlValue::string("Capture".into()),
            HeapObject::Ppool(_) => PerlValue::string("Ppool".into()),
            HeapObject::Barrier(_) => PerlValue::string("Barrier".into()),
            HeapObject::SqliteConn(_) => PerlValue::string("SqliteConn".into()),
            HeapObject::StructInst(s) => PerlValue::string(s.def.name.clone()),
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
            match unsafe { (self.heap_ref(), other.heap_ref()) } {
                (HeapObject::String(a), HeapObject::String(b)) => return a == b,
                _ => {}
            }
        }
        self.to_string() == other.to_string()
    }

    pub fn str_cmp(&self, other: &PerlValue) -> Ordering {
        if nanbox::is_heap(self.0) && nanbox::is_heap(other.0) {
            match unsafe { (self.heap_ref(), other.heap_ref()) } {
                (HeapObject::String(a), HeapObject::String(b)) => return a.cmp(b),
                _ => {}
            }
        }
        self.to_string().cmp(&other.to_string())
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
            _ => vec![self.clone()],
        }
    }

    pub fn scalar_context(&self) -> PerlValue {
        if !nanbox::is_heap(self.0) {
            return self.clone();
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
            HeapObject::Capture(_) | HeapObject::Ppool(_) | HeapObject::Barrier(_) => {
                PerlValue::integer(1)
            }
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
            HeapObject::ErrnoDual { msg, .. } => f.write_str(msg),
            HeapObject::String(s) => f.write_str(s),
            HeapObject::Bytes(b) => f.write_str(&String::from_utf8_lossy(b)),
            HeapObject::Array(a) => {
                for v in a {
                    write!(f, "{v}")?;
                }
                Ok(())
            }
            HeapObject::Hash(h) => write!(f, "{}/{}", h.len(), h.capacity()),
            HeapObject::ArrayRef(_) => f.write_str("ARRAY(0x...)"),
            HeapObject::HashRef(_) => f.write_str("HASH(0x...)"),
            HeapObject::ScalarRef(_) => f.write_str("SCALAR(0x...)"),
            HeapObject::CodeRef(sub) => write!(f, "CODE({})", sub.name),
            HeapObject::Regex(_, src) => write!(f, "(?:{src})"),
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
            HeapObject::Deque(d) => write!(f, "Deque({})", d.lock().len()),
            HeapObject::Heap(h) => write!(f, "Heap({})", h.lock().items.len()),
            HeapObject::Pipeline(p) => {
                let g = p.lock();
                write!(f, "Pipeline({} ops)", g.ops.len())
            }
            HeapObject::Capture(c) => write!(f, "Capture(exit={})", c.exitcode),
            HeapObject::Ppool(_) => f.write_str("Ppool"),
            HeapObject::Barrier(_) => f.write_str("Barrier"),
            HeapObject::SqliteConn(_) => f.write_str("SqliteConn"),
            HeapObject::StructInst(s) => write!(f, "{}=STRUCT(...)", s.def.name),
            HeapObject::DataFrame(d) => {
                let g = d.lock();
                write!(f, "DataFrame({} rows)", g.nrows())
            }
            HeapObject::Integer(n) => write!(f, "{n}"),
            HeapObject::Float(fl) => write!(f, "{}", format_float(*fl)),
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
        HeapObject::ScalarRef(_) => format!("sr:{v}"),
        HeapObject::CodeRef(_) => format!("c:{v}"),
        HeapObject::Regex(_, src) => format!("r:{src}"),
        HeapObject::IOHandle(s) => format!("io:{s}"),
        HeapObject::Atomic(arc) => format!("at:{}", set_member_key(&arc.lock())),
        HeapObject::ChannelTx(tx) => format!("chtx:{:p}", Arc::as_ptr(tx)),
        HeapObject::ChannelRx(rx) => format!("chrx:{:p}", Arc::as_ptr(rx)),
        HeapObject::AsyncTask(t) => format!("async:{:p}", Arc::as_ptr(t)),
        HeapObject::Deque(d) => format!("dq:{:p}", Arc::as_ptr(d)),
        HeapObject::Heap(h) => format!("hp:{:p}", Arc::as_ptr(h)),
        HeapObject::Pipeline(p) => format!("pl:{:p}", Arc::as_ptr(p)),
        HeapObject::Capture(c) => format!("cap:{:p}", Arc::as_ptr(c)),
        HeapObject::Ppool(p) => format!("pp:{:p}", Arc::as_ptr(&p.0)),
        HeapObject::Barrier(b) => format!("br:{:p}", Arc::as_ptr(&b.0)),
        HeapObject::SqliteConn(c) => format!("sql:{:p}", Arc::as_ptr(c)),
        HeapObject::StructInst(s) => format!("st:{}:{:?}", s.def.name, s.values),
        HeapObject::DataFrame(d) => format!("df:{:p}", Arc::as_ptr(d)),
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
        // Perl uses %g-like formatting
        let s = format!("{}", f);
        s
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
        }));
        assert!(c.to_string().contains("foo"));
    }

    #[test]
    fn display_regex_shows_non_capturing_prefix() {
        use regex::Regex;
        let r = PerlValue::regex(Arc::new(Regex::new("x+").unwrap()), "x+".into());
        assert_eq!(r.to_string(), "(?:x+)");
    }

    #[test]
    fn display_iohandle_is_name() {
        assert_eq!(PerlValue::io_handle("STDOUT".into()).to_string(), "STDOUT");
    }

    #[test]
    fn ref_type_blessed_uses_class_name() {
        let b = PerlValue::blessed(Arc::new(super::BlessedRef {
            class: "Pkg".into(),
            data: RwLock::new(PerlValue::UNDEF),
        }));
        assert_eq!(b.ref_type().to_string(), "Pkg");
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
}
