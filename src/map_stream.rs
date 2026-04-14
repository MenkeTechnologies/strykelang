//! Streaming `maps` / `flat_maps` / `filter` — lazy [`PerlIterator`] output (perlrs extension).

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::ast::Expr;
use crate::error::{PerlError, PerlResult};
use crate::interpreter::{FlowOrError, Interpreter, WantarrayCtx};
use crate::scope::{AtomicArray, AtomicHash};
use crate::value::{PerlIterator, PerlSub, PerlValue, PipelineOp};

struct VecPullIter {
    items: Arc<Vec<PerlValue>>,
    i: Mutex<usize>,
}

impl VecPullIter {
    fn new(items: Vec<PerlValue>) -> Self {
        Self {
            items: Arc::new(items),
            i: Mutex::new(0),
        }
    }
}

impl PerlIterator for VecPullIter {
    fn next_item(&self) -> Option<PerlValue> {
        let mut i = self.i.lock();
        if *i < self.items.len() {
            let v = self.items[*i].clone();
            *i += 1;
            Some(v)
        } else {
            None
        }
    }
}

pub(crate) fn into_pull_iter(val: PerlValue) -> Arc<dyn PerlIterator> {
    if val.is_iterator() {
        val.into_iterator()
    } else {
        Arc::new(VecPullIter::new(val.to_list()))
    }
}

enum MapStreamMode {
    Block(Arc<PerlSub>),
    Expr(Arc<Expr>),
}

pub(crate) struct MapStreamIterator {
    source: Arc<dyn PerlIterator>,
    pending: Mutex<VecDeque<PerlValue>>,
    mode: MapStreamMode,
    subs: std::collections::HashMap<String, Arc<PerlSub>>,
    capture: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
    peel: bool,
}

impl MapStreamIterator {
    pub(crate) fn new_block(
        source: Arc<dyn PerlIterator>,
        sub: Arc<PerlSub>,
        subs: std::collections::HashMap<String, Arc<PerlSub>>,
        capture: Vec<(String, PerlValue)>,
        atomic_arrays: Vec<(String, AtomicArray)>,
        atomic_hashes: Vec<(String, AtomicHash)>,
        peel: bool,
    ) -> Self {
        Self {
            source,
            pending: Mutex::new(VecDeque::new()),
            mode: MapStreamMode::Block(sub),
            subs,
            capture,
            atomic_arrays,
            atomic_hashes,
            peel,
        }
    }

    pub(crate) fn new_expr(
        source: Arc<dyn PerlIterator>,
        expr: Arc<Expr>,
        subs: std::collections::HashMap<String, Arc<PerlSub>>,
        capture: Vec<(String, PerlValue)>,
        atomic_arrays: Vec<(String, AtomicArray)>,
        atomic_hashes: Vec<(String, AtomicHash)>,
        peel: bool,
    ) -> Self {
        Self {
            source,
            pending: Mutex::new(VecDeque::new()),
            mode: MapStreamMode::Expr(expr),
            subs,
            capture,
            atomic_arrays,
            atomic_hashes,
            peel,
        }
    }

    fn refill_one_batch(&self) -> Result<bool, PerlError> {
        {
            let q = self.pending.lock();
            if !q.is_empty() {
                return Ok(true);
            }
        }
        while let Some(item) = self.source.next_item() {
            let mut interp = Interpreter::new();
            interp.subs = self.subs.clone();
            interp.scope.restore_capture(&self.capture);
            interp
                .scope
                .restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
            interp.scope.set_topic(item);
            match &self.mode {
                MapStreamMode::Block(sub) => {
                    match interp.exec_block_with_tail(&sub.body, WantarrayCtx::List) {
                        Ok(val) => {
                            let extended = val.map_flatten_outputs(self.peel);
                            if extended.is_empty() {
                                continue;
                            }
                            let mut q = self.pending.lock();
                            for x in extended {
                                q.push_back(x);
                            }
                            return Ok(true);
                        }
                        Err(FlowOrError::Error(e)) => return Err(e),
                        Err(FlowOrError::Flow(_)) => continue,
                    }
                }
                MapStreamMode::Expr(expr) => {
                    match interp.eval_expr_ctx(expr.as_ref(), WantarrayCtx::List) {
                        Ok(val) => {
                            let extended = val.map_flatten_outputs(self.peel);
                            if extended.is_empty() {
                                continue;
                            }
                            let mut q = self.pending.lock();
                            for x in extended {
                                q.push_back(x);
                            }
                            return Ok(true);
                        }
                        Err(FlowOrError::Error(e)) => return Err(e),
                        Err(FlowOrError::Flow(_)) => continue,
                    }
                }
            }
        }
        Ok(false)
    }
}

impl PerlIterator for MapStreamIterator {
    fn next_item(&self) -> Option<PerlValue> {
        loop {
            {
                let mut q = self.pending.lock();
                if let Some(v) = q.pop_front() {
                    return Some(v);
                }
            }
            match self.refill_one_batch() {
                Ok(true) => continue,
                Ok(false) => return None,
                Err(e) => panic!("maps iterator: {e}"),
            }
        }
    }
}

enum FilterStreamMode {
    Block(Arc<PerlSub>),
    Expr(Arc<Expr>),
}

pub(crate) struct FilterStreamIterator {
    source: Arc<dyn PerlIterator>,
    mode: FilterStreamMode,
    subs: std::collections::HashMap<String, Arc<PerlSub>>,
    capture: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
}

impl FilterStreamIterator {
    fn new_block(
        source: Arc<dyn PerlIterator>,
        sub: Arc<PerlSub>,
        subs: std::collections::HashMap<String, Arc<PerlSub>>,
        capture: Vec<(String, PerlValue)>,
        atomic_arrays: Vec<(String, AtomicArray)>,
        atomic_hashes: Vec<(String, AtomicHash)>,
    ) -> Self {
        Self {
            source,
            mode: FilterStreamMode::Block(sub),
            subs,
            capture,
            atomic_arrays,
            atomic_hashes,
        }
    }

    fn new_expr(
        source: Arc<dyn PerlIterator>,
        expr: Arc<Expr>,
        subs: std::collections::HashMap<String, Arc<PerlSub>>,
        capture: Vec<(String, PerlValue)>,
        atomic_arrays: Vec<(String, AtomicArray)>,
        atomic_hashes: Vec<(String, AtomicHash)>,
    ) -> Self {
        Self {
            source,
            mode: FilterStreamMode::Expr(expr),
            subs,
            capture,
            atomic_arrays,
            atomic_hashes,
        }
    }
}

impl PerlIterator for FilterStreamIterator {
    fn next_item(&self) -> Option<PerlValue> {
        while let Some(item) = self.source.next_item() {
            let mut interp = Interpreter::new();
            interp.subs = self.subs.clone();
            interp.scope.restore_capture(&self.capture);
            interp
                .scope
                .restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
            interp.scope.set_topic(item.clone());
            match &self.mode {
                FilterStreamMode::Block(sub) => match interp.exec_block(&sub.body) {
                    Ok(v) if v.is_true() => return Some(item),
                    Ok(_) => continue,
                    Err(FlowOrError::Error(e)) => panic!("filter iterator: {e}"),
                    Err(_) => continue,
                },
                FilterStreamMode::Expr(expr) => match interp.eval_expr(expr.as_ref()) {
                    Ok(v) if v.is_true() => return Some(item),
                    Ok(_) => continue,
                    Err(FlowOrError::Error(e)) => panic!("filter iterator: {e}"),
                    Err(_) => continue,
                },
            }
        }
        None
    }
}

impl Interpreter {
    /// Lazy `filter { }` / `filter EXPR` iterator (or push `->filter` onto a lone pipeline).
    pub(crate) fn filter_stream_block_output(
        &mut self,
        list_val: PerlValue,
        block: &crate::ast::Block,
        line: usize,
    ) -> PerlResult<PerlValue> {
        if let Some(p) = list_val.as_pipeline() {
            let sub = self.anon_coderef_from_block(block);
            self.pipeline_push(&p, PipelineOp::Filter(sub), line)?;
            return Ok(PerlValue::pipeline(Arc::clone(&p)));
        }
        if let Some(items) = list_val.as_array_vec() {
            if items.len() == 1 {
                if let Some(p) = items[0].as_pipeline() {
                    let sub = self.anon_coderef_from_block(block);
                    self.pipeline_push(&p, PipelineOp::Filter(sub), line)?;
                    return Ok(PerlValue::pipeline(Arc::clone(&p)));
                }
            }
        }
        let source = into_pull_iter(list_val);
        let (capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
        let sub = self.anon_coderef_from_block(block);
        Ok(PerlValue::iterator(Arc::new(
            FilterStreamIterator::new_block(
                source,
                sub,
                self.subs.clone(),
                capture,
                atomic_arrays,
                atomic_hashes,
            ),
        )))
    }

    /// Lazy `filter EXPR, LIST` iterator.
    pub(crate) fn filter_stream_expr_output(
        &mut self,
        list_val: PerlValue,
        expr: &Expr,
        line: usize,
    ) -> PerlResult<PerlValue> {
        if list_val.as_pipeline().is_some()
            || list_val
                .as_array_vec()
                .map(|a| a.len() == 1 && a[0].as_pipeline().is_some())
                .unwrap_or(false)
        {
            return Err(PerlError::runtime(
                "filter EXPR onto a pipeline value is not supported — use a block or a pipeline ->filter stage",
                line,
            ));
        }
        let source = into_pull_iter(list_val);
        let (capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
        Ok(PerlValue::iterator(Arc::new(
            FilterStreamIterator::new_expr(
                source,
                Arc::new(expr.clone()),
                self.subs.clone(),
                capture,
                atomic_arrays,
                atomic_hashes,
            ),
        )))
    }

    /// Build lazy `maps` / `maps { }` iterator (or push a stage onto a lone [`PerlValue::pipeline`]).
    pub(crate) fn map_stream_block_output(
        &mut self,
        list_val: PerlValue,
        block: &crate::ast::Block,
        peel: bool,
        line: usize,
    ) -> PerlResult<PerlValue> {
        if !peel {
            if let Some(p) = list_val.as_pipeline() {
                let sub = self.anon_coderef_from_block(block);
                self.pipeline_push(&p, PipelineOp::Map(sub), line)?;
                return Ok(PerlValue::pipeline(Arc::clone(&p)));
            }
            if let Some(items) = list_val.as_array_vec() {
                if items.len() == 1 {
                    if let Some(p) = items[0].as_pipeline() {
                        let sub = self.anon_coderef_from_block(block);
                        self.pipeline_push(&p, PipelineOp::Map(sub), line)?;
                        return Ok(PerlValue::pipeline(Arc::clone(&p)));
                    }
                }
            }
        } else if list_val.as_pipeline().is_some()
            || list_val
                .as_array_vec()
                .map(|a| a.len() == 1 && a[0].as_pipeline().is_some())
                .unwrap_or(false)
        {
            return Err(PerlError::runtime(
                "flat_maps onto a pipeline value is not supported in this form — use a pipeline ->map stage",
                line,
            ));
        }

        let source = into_pull_iter(list_val);
        let (capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
        let sub = self.anon_coderef_from_block(block);
        Ok(PerlValue::iterator(Arc::new(MapStreamIterator::new_block(
            source,
            sub,
            self.subs.clone(),
            capture,
            atomic_arrays,
            atomic_hashes,
            peel,
        ))))
    }

    /// Build lazy `maps EXPR, LIST` iterator.
    pub(crate) fn map_stream_expr_output(
        &mut self,
        list_val: PerlValue,
        expr: &Expr,
        peel: bool,
        line: usize,
    ) -> PerlResult<PerlValue> {
        if list_val.as_pipeline().is_some()
            || list_val
                .as_array_vec()
                .map(|a| a.len() == 1 && a[0].as_pipeline().is_some())
                .unwrap_or(false)
        {
            return Err(PerlError::runtime(
                if peel {
                    "flat_maps EXPR onto a pipeline value is not supported — use a block or a pipeline ->map stage"
                } else {
                    "maps EXPR onto a pipeline value is not supported — use a block or a pipeline ->map stage"
                },
                line,
            ));
        }
        let source = into_pull_iter(list_val);
        let (capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
        Ok(PerlValue::iterator(Arc::new(MapStreamIterator::new_expr(
            source,
            Arc::new(expr.clone()),
            self.subs.clone(),
            capture,
            atomic_arrays,
            atomic_hashes,
            peel,
        ))))
    }
}

/// Streaming `take N` — yields up to N items from the source.
pub(crate) struct TakeIterator {
    source: Arc<dyn PerlIterator>,
    remaining: Mutex<usize>,
}

impl TakeIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>, n: usize) -> Self {
        Self {
            source,
            remaining: Mutex::new(n),
        }
    }
}

impl PerlIterator for TakeIterator {
    fn next_item(&self) -> Option<PerlValue> {
        let mut rem = self.remaining.lock();
        if *rem == 0 {
            return None;
        }
        if let Some(item) = self.source.next_item() {
            *rem -= 1;
            Some(item)
        } else {
            None
        }
    }
}

/// Streaming `skip N` — drops N items then yields the rest.
pub(crate) struct SkipIterator {
    source: Arc<dyn PerlIterator>,
    to_skip: Mutex<usize>,
}

impl SkipIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>, n: usize) -> Self {
        Self {
            source,
            to_skip: Mutex::new(n),
        }
    }
}

impl PerlIterator for SkipIterator {
    fn next_item(&self) -> Option<PerlValue> {
        {
            let mut skip = self.to_skip.lock();
            while *skip > 0 {
                self.source.next_item()?;
                *skip -= 1;
            }
        }
        self.source.next_item()
    }
}

/// Streaming `enumerate` — yields `[$index, $item]` pairs.
pub(crate) struct EnumerateIterator {
    source: Arc<dyn PerlIterator>,
    idx: Mutex<usize>,
}

impl EnumerateIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>) -> Self {
        Self {
            source,
            idx: Mutex::new(0),
        }
    }
}

impl PerlIterator for EnumerateIterator {
    fn next_item(&self) -> Option<PerlValue> {
        let item = self.source.next_item()?;
        let mut i = self.idx.lock();
        let idx = *i;
        *i += 1;
        Some(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
            vec![PerlValue::integer(idx as i64), item],
        ))))
    }
}

/// Streaming `chunk N` — yields N-element arrayrefs.
pub(crate) struct ChunkIterator {
    source: Arc<dyn PerlIterator>,
    size: usize,
}

impl ChunkIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>, size: usize) -> Self {
        Self {
            source,
            size: size.max(1),
        }
    }
}

impl PerlIterator for ChunkIterator {
    fn next_item(&self) -> Option<PerlValue> {
        let mut chunk = Vec::with_capacity(self.size);
        for _ in 0..self.size {
            match self.source.next_item() {
                Some(item) => chunk.push(item),
                None => break,
            }
        }
        if chunk.is_empty() {
            None
        } else {
            Some(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
                chunk,
            ))))
        }
    }
}

/// Streaming `dedup` — drops consecutive duplicates.
pub(crate) struct DedupIterator {
    source: Arc<dyn PerlIterator>,
    last: Mutex<Option<String>>,
}

impl DedupIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>) -> Self {
        Self {
            source,
            last: Mutex::new(None),
        }
    }
}

impl PerlIterator for DedupIterator {
    fn next_item(&self) -> Option<PerlValue> {
        loop {
            let item = self.source.next_item()?;
            let s = item.to_string();
            let mut last = self.last.lock();
            if last.as_ref() != Some(&s) {
                *last = Some(s);
                return Some(item);
            }
        }
    }
}

/// Streaming `range N, M` — lazy integer sequence.
pub(crate) struct RangeIterator {
    current: Mutex<i64>,
    end: i64,
    step: i64,
}

impl RangeIterator {
    pub(crate) fn new(start: i64, end: i64) -> Self {
        let step = if end >= start { 1 } else { -1 };
        Self {
            current: Mutex::new(start),
            end,
            step,
        }
    }
}

impl PerlIterator for RangeIterator {
    fn next_item(&self) -> Option<PerlValue> {
        let mut cur = self.current.lock();
        if (self.step > 0 && *cur > self.end) || (self.step < 0 && *cur < self.end) {
            return None;
        }
        let val = *cur;
        *cur += self.step;
        Some(PerlValue::integer(val))
    }
}

/// Streaming `take_while { BLOCK }` — yields items while predicate is true.
#[allow(dead_code)]
pub(crate) struct TakeWhileIterator {
    source: Arc<dyn PerlIterator>,
    sub: Arc<PerlSub>,
    subs: std::collections::HashMap<String, Arc<PerlSub>>,
    capture: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
    done: Mutex<bool>,
}

impl TakeWhileIterator {
    #[allow(dead_code)]
    pub(crate) fn new(
        source: Arc<dyn PerlIterator>,
        sub: Arc<PerlSub>,
        subs: std::collections::HashMap<String, Arc<PerlSub>>,
        capture: Vec<(String, PerlValue)>,
        atomic_arrays: Vec<(String, AtomicArray)>,
        atomic_hashes: Vec<(String, AtomicHash)>,
    ) -> Self {
        Self {
            source,
            sub,
            subs,
            capture,
            atomic_arrays,
            atomic_hashes,
            done: Mutex::new(false),
        }
    }
}

impl PerlIterator for TakeWhileIterator {
    fn next_item(&self) -> Option<PerlValue> {
        if *self.done.lock() {
            return None;
        }
        if let Some(item) = self.source.next_item() {
            let mut interp = Interpreter::new();
            interp.subs = self.subs.clone();
            interp.scope.restore_capture(&self.capture);
            interp
                .scope
                .restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
            interp.scope.set_topic(item.clone());
            match interp.exec_block(&self.sub.body) {
                Ok(v) if v.is_true() => Some(item),
                _ => {
                    *self.done.lock() = true;
                    None
                }
            }
        } else {
            None
        }
    }
}

/// Streaming `skip_while { BLOCK }` — drops items while predicate is true, then yields the rest.
#[allow(dead_code)]
pub(crate) struct SkipWhileIterator {
    source: Arc<dyn PerlIterator>,
    sub: Arc<PerlSub>,
    subs: std::collections::HashMap<String, Arc<PerlSub>>,
    capture: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
    skipping: Mutex<bool>,
}

impl SkipWhileIterator {
    #[allow(dead_code)]
    pub(crate) fn new(
        source: Arc<dyn PerlIterator>,
        sub: Arc<PerlSub>,
        subs: std::collections::HashMap<String, Arc<PerlSub>>,
        capture: Vec<(String, PerlValue)>,
        atomic_arrays: Vec<(String, AtomicArray)>,
        atomic_hashes: Vec<(String, AtomicHash)>,
    ) -> Self {
        Self {
            source,
            sub,
            subs,
            capture,
            atomic_arrays,
            atomic_hashes,
            skipping: Mutex::new(true),
        }
    }
}

impl PerlIterator for SkipWhileIterator {
    fn next_item(&self) -> Option<PerlValue> {
        loop {
            let item = self.source.next_item()?;
            let still_skipping = *self.skipping.lock();
            if !still_skipping {
                return Some(item);
            }
            let mut interp = Interpreter::new();
            interp.subs = self.subs.clone();
            interp.scope.restore_capture(&self.capture);
            interp
                .scope
                .restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
            interp.scope.set_topic(item.clone());
            match interp.exec_block(&self.sub.body) {
                Ok(v) if v.is_true() => continue,
                _ => {
                    *self.skipping.lock() = false;
                    return Some(item);
                }
            }
        }
    }
}

/// Streaming `tap { BLOCK }` / `peek { BLOCK }` — execute side effect, pass through unchanged.
pub(crate) struct TapIterator {
    source: Arc<dyn PerlIterator>,
    sub: Arc<PerlSub>,
    subs: std::collections::HashMap<String, Arc<PerlSub>>,
    capture: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
}

impl TapIterator {
    pub(crate) fn new(
        source: Arc<dyn PerlIterator>,
        sub: Arc<PerlSub>,
        subs: std::collections::HashMap<String, Arc<PerlSub>>,
        capture: Vec<(String, PerlValue)>,
        atomic_arrays: Vec<(String, AtomicArray)>,
        atomic_hashes: Vec<(String, AtomicHash)>,
    ) -> Self {
        Self {
            source,
            sub,
            subs,
            capture,
            atomic_arrays,
            atomic_hashes,
        }
    }
}

impl PerlIterator for TapIterator {
    fn next_item(&self) -> Option<PerlValue> {
        let item = self.source.next_item()?;
        let mut interp = Interpreter::new();
        interp.subs = self.subs.clone();
        interp.scope.restore_capture(&self.capture);
        interp
            .scope
            .restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
        interp.scope.set_topic(item.clone());
        let _ = interp.exec_block(&self.sub.body);
        Some(item)
    }
}

/// Streaming `tee FILE` — write each item to file while passing through.
pub(crate) struct TeeIterator {
    source: Arc<dyn PerlIterator>,
    file: Mutex<std::io::BufWriter<std::fs::File>>,
}

impl TeeIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>, path: &str) -> std::io::Result<Self> {
        let file = std::fs::File::create(path)?;
        Ok(Self {
            source,
            file: Mutex::new(std::io::BufWriter::new(file)),
        })
    }
}

impl PerlIterator for TeeIterator {
    fn next_item(&self) -> Option<PerlValue> {
        use std::io::Write;
        let item = self.source.next_item()?;
        let s = item.to_string();
        let mut f = self.file.lock();
        let _ = writeln!(f, "{}", s);
        let _ = f.flush();
        Some(item)
    }
}

/// Streaming `grep_v PATTERN` — inverse filter (rejects matching items).
pub(crate) struct GrepVIterator {
    source: Arc<dyn PerlIterator>,
    re: regex::Regex,
}

impl GrepVIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>, re: regex::Regex) -> Self {
        Self { source, re }
    }
}

impl PerlIterator for GrepVIterator {
    fn next_item(&self) -> Option<PerlValue> {
        while let Some(item) = self.source.next_item() {
            if !self.re.is_match(&item.to_string()) {
                return Some(item);
            }
        }
        None
    }
}

/// Streaming `trim` — trims whitespace from each string item.
pub(crate) struct TrimIterator {
    source: Arc<dyn PerlIterator>,
}

impl TrimIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>) -> Self {
        Self { source }
    }
}

impl PerlIterator for TrimIterator {
    fn next_item(&self) -> Option<PerlValue> {
        self.source
            .next_item()
            .map(|v| PerlValue::string(v.to_string().trim().to_string()))
    }
}

/// Streaming `pluck KEY` — extracts a key from each hash ref.
pub(crate) struct PluckIterator {
    source: Arc<dyn PerlIterator>,
    key: String,
}

impl PluckIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>, key: String) -> Self {
        Self { source, key }
    }
}

impl PerlIterator for PluckIterator {
    fn next_item(&self) -> Option<PerlValue> {
        self.source.next_item().map(|v| {
            if let Some(hr) = v.as_hash_ref() {
                hr.read()
                    .get(&self.key)
                    .cloned()
                    .unwrap_or(PerlValue::UNDEF)
            } else {
                PerlValue::UNDEF
            }
        })
    }
}

/// Streaming `lines` — yields lines from a string (splits on newlines).
pub(crate) struct LinesIterator {
    lines: Arc<Vec<PerlValue>>,
    idx: Mutex<usize>,
}

impl LinesIterator {
    pub(crate) fn new(s: &str) -> Self {
        let lines: Vec<PerlValue> = s
            .lines()
            .map(|l| PerlValue::string(l.to_string()))
            .collect();
        Self {
            lines: Arc::new(lines),
            idx: Mutex::new(0),
        }
    }
}

impl PerlIterator for LinesIterator {
    fn next_item(&self) -> Option<PerlValue> {
        let mut i = self.idx.lock();
        if *i < self.lines.len() {
            let v = self.lines[*i].clone();
            *i += 1;
            Some(v)
        } else {
            None
        }
    }
}

/// Streaming `words` — yields words from a string (splits on whitespace).
pub(crate) struct WordsIterator {
    words: Arc<Vec<PerlValue>>,
    idx: Mutex<usize>,
}

impl WordsIterator {
    pub(crate) fn new(s: &str) -> Self {
        let words: Vec<PerlValue> = s
            .split_whitespace()
            .map(|w| PerlValue::string(w.to_string()))
            .collect();
        Self {
            words: Arc::new(words),
            idx: Mutex::new(0),
        }
    }
}

impl PerlIterator for WordsIterator {
    fn next_item(&self) -> Option<PerlValue> {
        let mut i = self.idx.lock();
        if *i < self.words.len() {
            let v = self.words[*i].clone();
            *i += 1;
            Some(v)
        } else {
            None
        }
    }
}

/// Streaming `chars` — yields characters from a string.
pub(crate) struct CharsIterator {
    chars: Arc<Vec<PerlValue>>,
    idx: Mutex<usize>,
}

impl CharsIterator {
    pub(crate) fn new(s: &str) -> Self {
        let chars: Vec<PerlValue> = s
            .chars()
            .map(|c| PerlValue::string(c.to_string()))
            .collect();
        Self {
            chars: Arc::new(chars),
            idx: Mutex::new(0),
        }
    }
}

impl PerlIterator for CharsIterator {
    fn next_item(&self) -> Option<PerlValue> {
        let mut i = self.idx.lock();
        if *i < self.chars.len() {
            let v = self.chars[*i].clone();
            *i += 1;
            Some(v)
        } else {
            None
        }
    }
}

/// Streaming `compact` — filters out undef and empty string values.
pub(crate) struct CompactIterator {
    source: Arc<dyn PerlIterator>,
}

impl CompactIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>) -> Self {
        Self { source }
    }
}

impl PerlIterator for CompactIterator {
    fn next_item(&self) -> Option<PerlValue> {
        while let Some(item) = self.source.next_item() {
            if !item.is_undef() && !item.to_string().is_empty() {
                return Some(item);
            }
        }
        None
    }
}

/// Streaming `reject { BLOCK }` — inverse of filter (keeps items where block returns false).
#[allow(dead_code)]
pub(crate) struct RejectIterator {
    source: Arc<dyn PerlIterator>,
    sub: Arc<PerlSub>,
    subs: std::collections::HashMap<String, Arc<PerlSub>>,
    capture: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
}

impl RejectIterator {
    #[allow(dead_code)]
    pub(crate) fn new(
        source: Arc<dyn PerlIterator>,
        sub: Arc<PerlSub>,
        subs: std::collections::HashMap<String, Arc<PerlSub>>,
        capture: Vec<(String, PerlValue)>,
        atomic_arrays: Vec<(String, AtomicArray)>,
        atomic_hashes: Vec<(String, AtomicHash)>,
    ) -> Self {
        Self {
            source,
            sub,
            subs,
            capture,
            atomic_arrays,
            atomic_hashes,
        }
    }
}

impl PerlIterator for RejectIterator {
    fn next_item(&self) -> Option<PerlValue> {
        while let Some(item) = self.source.next_item() {
            let mut interp = Interpreter::new();
            interp.subs = self.subs.clone();
            interp.scope.restore_capture(&self.capture);
            interp
                .scope
                .restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
            interp.scope.set_topic(item.clone());
            match interp.exec_block(&self.sub.body) {
                Ok(v) if !v.is_true() => return Some(item),
                Ok(_) => continue,
                Err(FlowOrError::Error(e)) => panic!("reject iterator: {e}"),
                Err(_) => continue,
            }
        }
        None
    }
}

/// Streaming `concat` / `chain` — concatenates multiple iterators.
pub(crate) struct ConcatIterator {
    sources: Vec<Arc<dyn PerlIterator>>,
    current_idx: Mutex<usize>,
}

impl ConcatIterator {
    pub(crate) fn new(sources: Vec<Arc<dyn PerlIterator>>) -> Self {
        Self {
            sources,
            current_idx: Mutex::new(0),
        }
    }
}

impl PerlIterator for ConcatIterator {
    fn next_item(&self) -> Option<PerlValue> {
        loop {
            let idx = *self.current_idx.lock();
            if idx >= self.sources.len() {
                return None;
            }
            if let Some(item) = self.sources[idx].next_item() {
                return Some(item);
            }
            *self.current_idx.lock() += 1;
        }
    }
}

/// Streaming `stdin` — yields lines from standard input lazily.
pub(crate) struct StdinIterator {
    reader: Mutex<std::io::BufReader<std::io::Stdin>>,
}

impl StdinIterator {
    pub(crate) fn new() -> Self {
        Self {
            reader: Mutex::new(std::io::BufReader::new(std::io::stdin())),
        }
    }
}

impl PerlIterator for StdinIterator {
    fn next_item(&self) -> Option<PerlValue> {
        use std::io::BufRead;
        let mut reader = self.reader.lock();
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => None, // EOF
            Ok(_) => {
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                Some(PerlValue::string(line))
            }
            Err(_) => None,
        }
    }
}

/// Generic streaming map with a closure — applies `f(string)` to each element.
pub(crate) struct MapFnIterator<F>
where
    F: Fn(String) -> String + Send + Sync,
{
    source: Arc<dyn PerlIterator>,
    f: F,
}

impl<F> MapFnIterator<F>
where
    F: Fn(String) -> String + Send + Sync,
{
    pub(crate) fn new(source: Arc<dyn PerlIterator>, f: F) -> Self {
        Self { source, f }
    }
}

impl<F> PerlIterator for MapFnIterator<F>
where
    F: Fn(String) -> String + Send + Sync,
{
    fn next_item(&self) -> Option<PerlValue> {
        self.source
            .next_item()
            .map(|v| PerlValue::string((self.f)(v.to_string())))
    }
}

/// Streaming `lines` over an iterator — flat-maps each element's lines.
pub(crate) struct LinesFlatMapIterator {
    source: Arc<dyn PerlIterator>,
    pending: Mutex<VecDeque<PerlValue>>,
}

impl LinesFlatMapIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>) -> Self {
        Self {
            source,
            pending: Mutex::new(VecDeque::new()),
        }
    }
}

impl PerlIterator for LinesFlatMapIterator {
    fn next_item(&self) -> Option<PerlValue> {
        loop {
            {
                let mut q = self.pending.lock();
                if let Some(v) = q.pop_front() {
                    return Some(v);
                }
            }
            let item = self.source.next_item()?;
            let s = item.to_string();
            let mut q = self.pending.lock();
            for line in s.lines() {
                q.push_back(PerlValue::string(line.to_string()));
            }
            if !q.is_empty() {
                return q.pop_front();
            }
        }
    }
}

/// Streaming `words` over an iterator — flat-maps each element's words.
pub(crate) struct WordsFlatMapIterator {
    source: Arc<dyn PerlIterator>,
    pending: Mutex<VecDeque<PerlValue>>,
}

impl WordsFlatMapIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>) -> Self {
        Self {
            source,
            pending: Mutex::new(VecDeque::new()),
        }
    }
}

impl PerlIterator for WordsFlatMapIterator {
    fn next_item(&self) -> Option<PerlValue> {
        loop {
            {
                let mut q = self.pending.lock();
                if let Some(v) = q.pop_front() {
                    return Some(v);
                }
            }
            let item = self.source.next_item()?;
            let s = item.to_string();
            let mut q = self.pending.lock();
            for word in s.split_whitespace() {
                q.push_back(PerlValue::string(word.to_string()));
            }
            if !q.is_empty() {
                return q.pop_front();
            }
        }
    }
}

/// Streaming `chars` over an iterator — flat-maps each element's characters.
pub(crate) struct CharsFlatMapIterator {
    source: Arc<dyn PerlIterator>,
    pending: Mutex<VecDeque<PerlValue>>,
}

impl CharsFlatMapIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>) -> Self {
        Self {
            source,
            pending: Mutex::new(VecDeque::new()),
        }
    }
}

impl PerlIterator for CharsFlatMapIterator {
    fn next_item(&self) -> Option<PerlValue> {
        loop {
            {
                let mut q = self.pending.lock();
                if let Some(v) = q.pop_front() {
                    return Some(v);
                }
            }
            let item = self.source.next_item()?;
            let s = item.to_string();
            let mut q = self.pending.lock();
            for c in s.chars() {
                q.push_back(PerlValue::string(c.to_string()));
            }
            if !q.is_empty() {
                return q.pop_front();
            }
        }
    }
}

/// Streaming `|> s/pat/rep/flags` — applies substitution to each iterator element.
pub(crate) struct SubstStreamIterator {
    source: Arc<dyn PerlIterator>,
    re: Arc<crate::perl_regex::PerlCompiledRegex>,
    replacement: String,
    global: bool,
}

impl SubstStreamIterator {
    pub(crate) fn new(
        source: Arc<dyn PerlIterator>,
        re: Arc<crate::perl_regex::PerlCompiledRegex>,
        replacement: String,
        global: bool,
    ) -> Self {
        Self {
            source,
            re,
            replacement,
            global,
        }
    }
}

impl PerlIterator for SubstStreamIterator {
    fn next_item(&self) -> Option<PerlValue> {
        self.source.next_item().map(|v| {
            let s = v.to_string();
            let result = if self.global {
                self.re.replace_all(&s, &self.replacement)
            } else {
                self.re.replace(&s, &self.replacement)
            };
            PerlValue::string(result)
        })
    }
}

/// Streaming `|> tr/from/to/flags` — applies transliteration to each iterator element.
pub(crate) struct TransliterateStreamIterator {
    source: Arc<dyn PerlIterator>,
    from_chars: Vec<char>,
    to_chars: Vec<char>,
    complement: bool,
    delete: bool,
    squash: bool,
}

impl TransliterateStreamIterator {
    pub(crate) fn new(source: Arc<dyn PerlIterator>, from: &str, to: &str, flags: &str) -> Self {
        let from_chars = Interpreter::tr_expand_ranges(from);
        let to_chars = Interpreter::tr_expand_ranges(to);
        Self {
            source,
            from_chars,
            to_chars,
            complement: flags.contains('c'),
            delete: flags.contains('d'),
            squash: flags.contains('s'),
        }
    }
}

impl PerlIterator for TransliterateStreamIterator {
    fn next_item(&self) -> Option<PerlValue> {
        self.source.next_item().map(|v| {
            let s = v.to_string();
            let result = transliterate_string(
                &s,
                &self.from_chars,
                &self.to_chars,
                self.complement,
                self.delete,
                self.squash,
            );
            PerlValue::string(result)
        })
    }
}

/// Standalone transliteration logic (used by streaming iterator).
pub(crate) fn transliterate_string(
    s: &str,
    from_chars: &[char],
    to_chars: &[char],
    complement: bool,
    delete: bool,
    squash: bool,
) -> String {
    let mut result = String::with_capacity(s.len());
    let mut last_out: Option<char> = None;
    for c in s.chars() {
        let in_from = from_chars.iter().position(|&fc| fc == c);
        let should_replace = if complement {
            in_from.is_none()
        } else {
            in_from.is_some()
        };
        if should_replace {
            if delete {
                continue;
            }
            let out_c = if complement {
                to_chars.last().copied().unwrap_or(c)
            } else if let Some(pos) = in_from {
                to_chars.get(pos).or(to_chars.last()).copied().unwrap_or(c)
            } else {
                c
            };
            if squash && last_out == Some(out_c) {
                continue;
            }
            result.push(out_c);
            last_out = Some(out_c);
        } else {
            result.push(c);
            last_out = Some(c);
        }
    }
    result
}

/// Streaming `|> /re/` (non-global) — maps each element to its first match/captures.
pub(crate) struct MatchStreamIterator {
    source: Arc<dyn PerlIterator>,
    re: Arc<crate::perl_regex::PerlCompiledRegex>,
}

impl MatchStreamIterator {
    pub(crate) fn new(
        source: Arc<dyn PerlIterator>,
        re: Arc<crate::perl_regex::PerlCompiledRegex>,
    ) -> Self {
        Self { source, re }
    }
}

impl PerlIterator for MatchStreamIterator {
    fn next_item(&self) -> Option<PerlValue> {
        self.source.next_item().map(|v| {
            let s = v.to_string();
            if let Some(caps) = self.re.captures(&s) {
                let len = caps.len();
                if len > 1 {
                    let captures: Vec<PerlValue> = (1..len)
                        .map(|i| match caps.get(i) {
                            Some(m) => PerlValue::string(m.text.to_string()),
                            None => PerlValue::UNDEF,
                        })
                        .collect();
                    PerlValue::array(captures)
                } else if let Some(m) = caps.get(0) {
                    PerlValue::string(m.text.to_string())
                } else {
                    PerlValue::UNDEF
                }
            } else {
                PerlValue::UNDEF
            }
        })
    }
}

/// Streaming `|> /re/g` (global) — flat_maps each element to all matches.
pub(crate) struct MatchGlobalStreamIterator {
    source: Arc<dyn PerlIterator>,
    re: Arc<crate::perl_regex::PerlCompiledRegex>,
    pending: Mutex<VecDeque<PerlValue>>,
}

impl MatchGlobalStreamIterator {
    pub(crate) fn new(
        source: Arc<dyn PerlIterator>,
        re: Arc<crate::perl_regex::PerlCompiledRegex>,
    ) -> Self {
        Self {
            source,
            re,
            pending: Mutex::new(VecDeque::new()),
        }
    }
}

impl PerlIterator for MatchGlobalStreamIterator {
    fn next_item(&self) -> Option<PerlValue> {
        loop {
            {
                let mut q = self.pending.lock();
                if let Some(v) = q.pop_front() {
                    return Some(v);
                }
            }
            let item = self.source.next_item()?;
            let s = item.to_string();
            let mut q = self.pending.lock();
            for caps in self.re.captures_iter(&s) {
                let len = caps.len();
                if len > 1 {
                    for i in 1..len {
                        if let Some(m) = caps.get(i) {
                            q.push_back(PerlValue::string(m.text.to_string()));
                        }
                    }
                } else if let Some(m) = caps.get(0) {
                    q.push_back(PerlValue::string(m.text.to_string()));
                }
            }
            if !q.is_empty() {
                return q.pop_front();
            }
        }
    }
}
