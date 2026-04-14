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
            interp.scope.restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
            let _ = interp.scope.set_scalar("_", item);
            match &self.mode {
                MapStreamMode::Block(sub) => {
                    if let Some(ref env) = sub.closure_env {
                        interp.scope.restore_capture(env);
                    }
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
                MapStreamMode::Expr(expr) => match interp.eval_expr_ctx(expr.as_ref(), WantarrayCtx::List)
                {
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
                },
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
            interp.scope.restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
            let _ = interp.scope.set_scalar("_", item.clone());
            match &self.mode {
                FilterStreamMode::Block(sub) => {
                    if let Some(ref env) = sub.closure_env {
                        interp.scope.restore_capture(env);
                    }
                    match interp.exec_block(&sub.body) {
                        Ok(v) if v.is_true() => return Some(item),
                        Ok(_) => continue,
                        Err(FlowOrError::Error(e)) => panic!("filter iterator: {e}"),
                        Err(_) => continue,
                    }
                }
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
        Ok(PerlValue::iterator(Arc::new(FilterStreamIterator::new_block(
            source,
            sub,
            self.subs.clone(),
            capture,
            atomic_arrays,
            atomic_hashes,
        ))))
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
        Ok(PerlValue::iterator(Arc::new(FilterStreamIterator::new_expr(
            source,
            Arc::new(expr.clone()),
            self.subs.clone(),
            capture,
            atomic_arrays,
            atomic_hashes,
        ))))
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
                if self.source.next_item().is_none() {
                    return None;
                }
                *skip -= 1;
            }
        }
        self.source.next_item()
    }
}

/// Streaming `take_while { BLOCK }` — yields items while predicate is true.
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
            interp.scope.restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
            let _ = interp.scope.set_scalar("_", item.clone());
            if let Some(ref env) = self.sub.closure_env {
                interp.scope.restore_capture(env);
            }
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
            interp.scope.restore_atomics(&self.atomic_arrays, &self.atomic_hashes);
            let _ = interp.scope.set_scalar("_", item.clone());
            if let Some(ref env) = self.sub.closure_env {
                interp.scope.restore_capture(env);
            }
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
                hr.read().get(&self.key).cloned().unwrap_or(PerlValue::UNDEF)
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
        let lines: Vec<PerlValue> = s.lines().map(|l| PerlValue::string(l.to_string())).collect();
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

/// Streaming `chars` — yields characters from a string.
pub(crate) struct CharsIterator {
    chars: Arc<Vec<PerlValue>>,
    idx: Mutex<usize>,
}

impl CharsIterator {
    pub(crate) fn new(s: &str) -> Self {
        let chars: Vec<PerlValue> = s.chars().map(|c| PerlValue::string(c.to_string())).collect();
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
