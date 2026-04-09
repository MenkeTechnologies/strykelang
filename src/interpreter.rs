use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write as IoWrite};
use std::process::Command;
use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;

use caseless::default_case_fold_str;

use crate::ast::*;
use crate::crypt_util::perl_crypt;
use crate::error::{ErrorKind, PerlError, PerlResult};
use crate::scope::Scope;
use crate::value::{PerlAsyncTask, PerlHeap, PerlSub, PerlValue};

/// Flow control signals propagated via Result.
#[derive(Debug)]
pub(crate) enum Flow {
    Return(PerlValue),
    Last(Option<String>),
    Next(Option<String>),
    Redo(Option<String>),
}

pub(crate) type ExecResult = Result<PerlValue, FlowOrError>;

#[derive(Debug)]
pub(crate) enum FlowOrError {
    Flow(Flow),
    Error(PerlError),
}

impl From<PerlError> for FlowOrError {
    fn from(e: PerlError) -> Self {
        FlowOrError::Error(e)
    }
}

impl From<Flow> for FlowOrError {
    fn from(f: Flow) -> Self {
        FlowOrError::Flow(f)
    }
}

pub struct Interpreter {
    pub scope: Scope,
    pub(crate) subs: HashMap<String, Arc<PerlSub>>,
    pub(crate) file: String,
    /// File handles: name → writer
    output_handles: HashMap<String, Box<dyn IoWrite + Send>>,
    input_handles: HashMap<String, BufReader<Box<dyn Read + Send>>>,
    /// Output separator ($,)
    pub ofs: String,
    /// Output record separator ($\)
    pub ors: String,
    /// Input record separator ($/)
    pub irs: String,
    /// $! — last OS error
    pub errno: String,
    /// $@ — last eval error
    pub eval_error: String,
    /// @ARGV
    pub argv: Vec<String>,
    /// %ENV
    pub env: IndexMap<String, PerlValue>,
    /// $0
    pub program_name: String,
    /// Current line number $.
    pub line_number: i64,
    /// Auto-split mode (-a)
    pub auto_split: bool,
    /// Field separator for -F
    pub field_separator: Option<String>,
    /// BEGIN blocks
    begin_blocks: Vec<Block>,
    /// END blocks
    end_blocks: Vec<Block>,
    /// -w warnings
    pub warnings: bool,
    /// Number of parallel threads
    pub num_threads: usize,
    /// Compiled regex cache: "flags///pattern" → Regex
    regex_cache: HashMap<String, regex::Regex>,
    /// Offsets for Perl `m//g` in scalar context (`pos`), keyed by scalar name (`"_"` for `$_`).
    pub(crate) regex_pos: HashMap<String, Option<usize>>,
    /// PRNG for `rand` / `srand` (matches Perl-style seeding, not crypto).
    pub(crate) rand_rng: StdRng,
    /// Directory handles from `opendir`: name → snapshot + read cursor (`readdir` / `rewinddir` / …).
    pub(crate) dir_handles: HashMap<String, DirHandleState>,
}

/// Buffered directory listing for Perl `opendir` / `readdir` (Rust `ReadDir` is single-pass).
#[derive(Debug, Clone)]
pub(crate) struct DirHandleState {
    pub entries: Vec<String>,
    pub pos: usize,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        let mut env = IndexMap::new();
        for (k, v) in std::env::vars() {
            env.insert(k, PerlValue::String(v));
        }

        Self {
            scope: Scope::new(),
            subs: HashMap::new(),
            file: "-e".to_string(),
            output_handles: HashMap::new(),
            input_handles: HashMap::new(),
            ofs: String::new(),
            ors: String::new(),
            irs: "\n".to_string(),
            errno: String::new(),
            eval_error: String::new(),
            argv: Vec::new(),
            env,
            program_name: "perlrs".to_string(),
            line_number: 0,
            auto_split: false,
            field_separator: None,
            begin_blocks: Vec::new(),
            end_blocks: Vec::new(),
            warnings: false,
            num_threads: rayon::current_num_threads(),
            regex_cache: HashMap::new(),
            regex_pos: HashMap::new(),
            rand_rng: StdRng::from_entropy(),
            dir_handles: HashMap::new(),
        }
    }

    pub(crate) fn opendir_handle(&mut self, handle: &str, path: &str) -> PerlValue {
        match std::fs::read_dir(path) {
            Ok(rd) => {
                let entries: Vec<String> = rd
                    .filter_map(|e| {
                        e.ok()
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                    })
                    .collect();
                self.dir_handles.insert(
                    handle.to_string(),
                    DirHandleState { entries, pos: 0 },
                );
                PerlValue::Integer(1)
            }
            Err(e) => {
                self.errno = e.to_string();
                PerlValue::Integer(0)
            }
        }
    }

    pub(crate) fn readdir_handle(&mut self, handle: &str) -> PerlValue {
        if let Some(dh) = self.dir_handles.get_mut(handle) {
            if dh.pos < dh.entries.len() {
                let s = dh.entries[dh.pos].clone();
                dh.pos += 1;
                PerlValue::String(s)
            } else {
                PerlValue::Undef
            }
        } else {
            PerlValue::Undef
        }
    }

    pub(crate) fn closedir_handle(&mut self, handle: &str) -> PerlValue {
        PerlValue::Integer(if self.dir_handles.remove(handle).is_some() {
            1
        } else {
            0
        })
    }

    pub(crate) fn rewinddir_handle(&mut self, handle: &str) -> PerlValue {
        if let Some(dh) = self.dir_handles.get_mut(handle) {
            dh.pos = 0;
            PerlValue::Integer(1)
        } else {
            PerlValue::Integer(0)
        }
    }

    pub(crate) fn telldir_handle(&mut self, handle: &str) -> PerlValue {
        self.dir_handles
            .get(handle)
            .map(|dh| PerlValue::Integer(dh.pos as i64))
            .unwrap_or(PerlValue::Undef)
    }

    pub(crate) fn seekdir_handle(&mut self, handle: &str, pos: usize) -> PerlValue {
        if let Some(dh) = self.dir_handles.get_mut(handle) {
            dh.pos = pos.min(dh.entries.len());
            PerlValue::Integer(1)
        } else {
            PerlValue::Integer(0)
        }
    }

    /// Shared regex match for tree-walker and VM (`pos` is updated for scalar `/g`).
    pub(crate) fn regex_match_execute(
        &mut self,
        s: String,
        pattern: &str,
        flags: &str,
        scalar_g: bool,
        pos_key: &str,
        line: usize,
    ) -> ExecResult {
        let re = self.compile_regex(pattern, flags, line)?;
        if flags.contains('g') && scalar_g {
            let key = pos_key.to_string();
            let start = self.regex_pos.get(&key).copied().flatten().unwrap_or(0);
            if start > s.len() {
                self.regex_pos.insert(key, None);
                return Ok(PerlValue::Integer(0));
            }
            let sub = s.get(start..).unwrap_or("");
            if let Some(caps) = re.captures(sub) {
                let overall = caps.get(0).unwrap();
                let abs_end = start + overall.end();
                self.regex_pos.insert(key, Some(abs_end));
                for i in 1..caps.len() {
                    if let Some(m) = caps.get(i) {
                        self.scope
                            .set_scalar(&i.to_string(), PerlValue::String(m.as_str().to_string()));
                    }
                }
                Ok(PerlValue::Integer(1))
            } else {
                self.regex_pos.insert(key, None);
                Ok(PerlValue::Integer(0))
            }
        } else if flags.contains('g') {
            let matches: Vec<PerlValue> = re
                .find_iter(&s)
                .map(|m| PerlValue::String(m.as_str().to_string()))
                .collect();
            if matches.is_empty() {
                Ok(PerlValue::Integer(0))
            } else {
                Ok(PerlValue::Array(matches))
            }
        } else if let Some(caps) = re.captures(&s) {
            for i in 1..caps.len() {
                if let Some(m) = caps.get(i) {
                    self.scope
                        .set_scalar(&i.to_string(), PerlValue::String(m.as_str().to_string()));
                }
            }
            Ok(PerlValue::Integer(1))
        } else {
            Ok(PerlValue::Integer(0))
        }
    }

    /// Random fractional value like Perl `rand`: `[0, upper)` when `upper > 0`,
    /// `(upper, 0]` when `upper < 0`, and `[0, 1)` when `upper == 0`.
    pub(crate) fn perl_rand(&mut self, upper: f64) -> f64 {
        if upper == 0.0 {
            self.rand_rng.gen_range(0.0..1.0)
        } else if upper > 0.0 {
            self.rand_rng.gen_range(0.0..upper)
        } else {
            self.rand_rng.gen_range(upper..0.0)
        }
    }

    /// Seed the PRNG; returns the seed Perl would report (truncated integer / time).
    pub(crate) fn perl_srand(&mut self, seed: Option<f64>) -> i64 {
        let n = if let Some(s) = seed {
            s as i64
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(1)
        };
        let mag = n.unsigned_abs();
        self.rand_rng = StdRng::seed_from_u64(mag);
        n.abs()
    }

    pub fn set_file(&mut self, file: &str) {
        self.file = file.to_string();
    }

    pub fn execute(&mut self, program: &Program) -> PerlResult<PerlValue> {
        // Try bytecode VM first — falls back to tree-walker on unsupported features
        if let Some(result) = crate::try_vm_execute(program, self) {
            return result;
        }

        // Tree-walker fallback
        self.execute_tree(program)
    }

    /// Tree-walking execution (fallback when bytecode compilation fails).
    pub fn execute_tree(&mut self, program: &Program) -> PerlResult<PerlValue> {
        // First pass: collect subs and BEGIN/END blocks
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::SubDecl { name, params, body } => {
                    self.subs.insert(
                        name.clone(),
                        Arc::new(PerlSub {
                            name: name.clone(),
                            params: params.clone(),
                            body: body.clone(),
                            closure_env: None,
                        }),
                    );
                }
                StmtKind::Begin(block) => self.begin_blocks.push(block.clone()),
                StmtKind::End(block) => self.end_blocks.push(block.clone()),
                _ => {}
            }
        }

        // Execute BEGIN blocks
        let begins = std::mem::take(&mut self.begin_blocks);
        for block in &begins {
            self.exec_block(block).map_err(|e| match e {
                FlowOrError::Error(e) => e,
                FlowOrError::Flow(_) => PerlError::runtime("Unexpected flow control in BEGIN", 0),
            })?;
        }

        // Execute main program
        let mut last = PerlValue::Undef;
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::SubDecl { .. } | StmtKind::Begin(_) | StmtKind::End(_) => continue,
                _ => {
                    match self.exec_statement(stmt) {
                        Ok(val) => last = val,
                        Err(FlowOrError::Error(e)) => {
                            // Execute END blocks before propagating (all exit codes, including 0)
                            let ends = std::mem::take(&mut self.end_blocks);
                            for block in &ends {
                                let _ = self.exec_block(block);
                            }
                            return Err(e);
                        }
                        Err(FlowOrError::Flow(Flow::Return(v))) => {
                            last = v;
                            break;
                        }
                        Err(FlowOrError::Flow(_)) => {}
                    }
                }
            }
        }

        // Execute END blocks
        let ends = std::mem::take(&mut self.end_blocks);
        for block in &ends {
            let _ = self.exec_block(block);
        }

        Ok(last)
    }

    pub(crate) fn exec_block(&mut self, block: &Block) -> ExecResult {
        self.scope.push_frame();
        let result = self.exec_block_no_scope(block);
        self.scope.pop_frame();
        result
    }

    /// Execute block statements without pushing/popping a scope frame.
    /// Used internally by loops and the VM for sub calls.
    #[inline]
    pub(crate) fn exec_block_no_scope(&mut self, block: &Block) -> ExecResult {
        let mut last = PerlValue::Undef;
        for stmt in block {
            match self.exec_statement(stmt) {
                Ok(v) => last = v,
                Err(e) => return Err(e),
            }
        }
        Ok(last)
    }

    /// Spawn `block` on a worker thread; returns an [`PerlValue::AsyncTask`] handle.
    pub(crate) fn spawn_async_block(&self, block: &Block) -> PerlValue {
        use parking_lot::Mutex as ParkMutex;

        let block = block.clone();
        let subs = self.subs.clone();
        let (scalars, aar, ahash) = self.scope.capture_with_atomics();
        let result = Arc::new(ParkMutex::new(None));
        let join = Arc::new(ParkMutex::new(None));
        let result2 = result.clone();
        let h = std::thread::spawn(move || {
            let mut interp = Interpreter::new();
            interp.subs = subs;
            interp.scope.restore_capture(&scalars);
            interp.scope.restore_atomics(&aar, &ahash);
            let r = match interp.exec_block(&block) {
                Ok(v) => Ok(v),
                Err(FlowOrError::Error(e)) => Err(e),
                Err(FlowOrError::Flow(_)) => Ok(PerlValue::Undef),
            };
            *result2.lock() = Some(r);
        });
        *join.lock() = Some(h);
        PerlValue::AsyncTask(Arc::new(PerlAsyncTask { result, join }))
    }

    /// Check if a block declares variables (needs its own scope frame).
    #[inline]
    fn block_needs_scope(block: &Block) -> bool {
        block.iter().any(|s| {
            matches!(
                s.kind,
                StmtKind::My(_) | StmtKind::Our(_) | StmtKind::Local(_)
            )
        })
    }

    /// Execute block, only pushing a scope frame if needed.
    #[inline]
    fn exec_block_smart(&mut self, block: &Block) -> ExecResult {
        if Self::block_needs_scope(block) {
            self.exec_block(block)
        } else {
            self.exec_block_no_scope(block)
        }
    }

    fn exec_statement(&mut self, stmt: &Statement) -> ExecResult {
        match &stmt.kind {
            StmtKind::Expression(expr) => self.eval_expr(expr),
            StmtKind::If {
                condition,
                body,
                elsifs,
                else_block,
            } => {
                let cond = self.eval_expr(condition)?;
                if cond.is_true() {
                    return self.exec_block(body);
                }
                for (c, b) in elsifs {
                    let cv = self.eval_expr(c)?;
                    if cv.is_true() {
                        return self.exec_block(b);
                    }
                }
                if let Some(eb) = else_block {
                    return self.exec_block(eb);
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::Unless {
                condition,
                body,
                else_block,
            } => {
                let cond = self.eval_expr(condition)?;
                if !cond.is_true() {
                    return self.exec_block(body);
                }
                if let Some(eb) = else_block {
                    return self.exec_block(eb);
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::While {
                condition,
                body,
                label,
            } => {
                loop {
                    let cond = self.eval_expr(condition)?;
                    if !cond.is_true() {
                        break;
                    }
                    match self.exec_block_smart(body) {
                        Ok(_) => {}
                        Err(FlowOrError::Flow(Flow::Last(ref l))) if l == label || l.is_none() => {
                            break
                        }
                        Err(FlowOrError::Flow(Flow::Next(ref l))) if l == label || l.is_none() => {
                            continue
                        }
                        Err(FlowOrError::Flow(Flow::Redo(ref l))) if l == label || l.is_none() => {
                            let _ = self.exec_block_smart(body);
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::Until {
                condition,
                body,
                label,
            } => {
                loop {
                    let cond = self.eval_expr(condition)?;
                    if cond.is_true() {
                        break;
                    }
                    match self.exec_block(body) {
                        Ok(_) => {}
                        Err(FlowOrError::Flow(Flow::Last(ref l))) if l == label || l.is_none() => {
                            break
                        }
                        Err(FlowOrError::Flow(Flow::Next(ref l))) if l == label || l.is_none() => {
                            continue
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::DoWhile { body, condition } => {
                loop {
                    self.exec_block(body)?;
                    let cond = self.eval_expr(condition)?;
                    if !cond.is_true() {
                        break;
                    }
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
                label,
            } => {
                self.scope.push_frame();
                if let Some(init) = init {
                    self.exec_statement(init)?;
                }
                loop {
                    if let Some(cond) = condition {
                        let cv = self.eval_expr(cond)?;
                        if !cv.is_true() {
                            break;
                        }
                    }
                    match self.exec_block_smart(body) {
                        Ok(_) => {}
                        Err(FlowOrError::Flow(Flow::Last(ref l))) if l == label || l.is_none() => {
                            break
                        }
                        Err(FlowOrError::Flow(Flow::Next(ref l))) if l == label || l.is_none() => {}
                        Err(e) => {
                            self.scope.pop_frame();
                            return Err(e);
                        }
                    }
                    if let Some(step) = step {
                        self.eval_expr(step)?;
                    }
                }
                self.scope.pop_frame();
                Ok(PerlValue::Undef)
            }
            StmtKind::Foreach {
                var,
                list,
                body,
                label,
            } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                self.scope.push_frame();
                self.scope.declare_scalar(var, PerlValue::Undef);
                for item in items {
                    self.scope.set_scalar(var, item);
                    match self.exec_block_smart(body) {
                        Ok(_) => {}
                        Err(FlowOrError::Flow(Flow::Last(ref l))) if l == label || l.is_none() => {
                            break
                        }
                        Err(FlowOrError::Flow(Flow::Next(ref l))) if l == label || l.is_none() => {
                            continue
                        }
                        Err(e) => {
                            self.scope.pop_frame();
                            return Err(e);
                        }
                    }
                }
                self.scope.pop_frame();
                Ok(PerlValue::Undef)
            }
            StmtKind::SubDecl { name, params, body } => {
                self.subs.insert(
                    name.clone(),
                    Arc::new(PerlSub {
                        name: name.clone(),
                        params: params.clone(),
                        body: body.clone(),
                        closure_env: None,
                    }),
                );
                Ok(PerlValue::Undef)
            }
            StmtKind::My(decls) | StmtKind::Our(decls) | StmtKind::Local(decls) => {
                // For list assignment my ($a, $b) = (10, 20), distribute elements.
                // All decls share the same initializer in the AST (parser clones it).
                if decls.len() > 1 && decls[0].initializer.is_some() {
                    let val = self.eval_expr(decls[0].initializer.as_ref().unwrap())?;
                    let items = val.to_list();
                    let mut idx = 0;
                    for decl in decls {
                        match decl.sigil {
                            Sigil::Scalar => {
                                let v = items.get(idx).cloned().unwrap_or(PerlValue::Undef);
                                self.scope.declare_scalar(&decl.name, v);
                                idx += 1;
                            }
                            Sigil::Array => {
                                // Array slurps remaining elements
                                let rest: Vec<PerlValue> = items[idx..].to_vec();
                                idx = items.len();
                                self.scope.declare_array(&decl.name, rest);
                            }
                            Sigil::Hash => {
                                let rest: Vec<PerlValue> = items[idx..].to_vec();
                                idx = items.len();
                                let mut map = IndexMap::new();
                                let mut i = 0;
                                while i + 1 < rest.len() {
                                    map.insert(rest[i].to_string(), rest[i + 1].clone());
                                    i += 2;
                                }
                                self.scope.declare_hash(&decl.name, map);
                            }
                        }
                    }
                } else {
                    // Single decl or no initializer
                    for decl in decls {
                        let val = if let Some(init) = &decl.initializer {
                            self.eval_expr(init)?
                        } else {
                            PerlValue::Undef
                        };
                        match decl.sigil {
                            Sigil::Scalar => self.scope.declare_scalar(&decl.name, val),
                            Sigil::Array => {
                                let items = val.to_list();
                                self.scope.declare_array(&decl.name, items);
                            }
                            Sigil::Hash => {
                                let items = val.to_list();
                                let mut map = IndexMap::new();
                                let mut i = 0;
                                while i + 1 < items.len() {
                                    let k = items[i].to_string();
                                    let v = items[i + 1].clone();
                                    map.insert(k, v);
                                    i += 2;
                                }
                                self.scope.declare_hash(&decl.name, map);
                            }
                        }
                    }
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::MySync(decls) => {
                for decl in decls {
                    let val = if let Some(init) = &decl.initializer {
                        self.eval_expr(init)?
                    } else {
                        PerlValue::Undef
                    };
                    match decl.sigil {
                        Sigil::Scalar => {
                            let atomic = PerlValue::Atomic(std::sync::Arc::new(
                                parking_lot::Mutex::new(val),
                            ));
                            self.scope.declare_scalar(&decl.name, atomic);
                        }
                        Sigil::Array => {
                            self.scope.declare_atomic_array(&decl.name, val.to_list());
                        }
                        Sigil::Hash => {
                            let items = val.to_list();
                            let mut map = IndexMap::new();
                            let mut i = 0;
                            while i + 1 < items.len() {
                                map.insert(items[i].to_string(), items[i + 1].clone());
                                i += 2;
                            }
                            self.scope.declare_atomic_hash(&decl.name, map);
                        }
                    }
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::Package { name } => {
                // Minimal package support — just set a variable
                self.scope
                    .set_scalar("__PACKAGE__", PerlValue::String(name.clone()));
                Ok(PerlValue::Undef)
            }
            StmtKind::Use { module, imports: _ } => {
                match module.as_str() {
                    "strict" | "warnings" | "utf8" | "feature" | "v5" => {
                        if module == "warnings" {
                            self.warnings = true;
                        }
                    }
                    "threads" | "Thread::Pool" | "Parallel::ForkManager" => {
                        // Our parallel primitives handle this
                    }
                    _ => {
                        // Try to load module file
                        // For now, silently ignore unknown modules
                    }
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::No { module, .. } => {
                if module == "warnings" {
                    self.warnings = false;
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::Return(val) => {
                let v = if let Some(e) = val {
                    self.eval_expr(e)?
                } else {
                    PerlValue::Undef
                };
                Err(Flow::Return(v).into())
            }
            StmtKind::Last(label) => Err(Flow::Last(label.clone()).into()),
            StmtKind::Next(label) => Err(Flow::Next(label.clone()).into()),
            StmtKind::Redo(label) => Err(Flow::Redo(label.clone()).into()),
            StmtKind::Block(block) => self.exec_block(block),
            StmtKind::Begin(_) | StmtKind::End(_) => Ok(PerlValue::Undef),
            StmtKind::Empty => Ok(PerlValue::Undef),
        }
    }

    #[inline]
    fn eval_expr(&mut self, expr: &Expr) -> ExecResult {
        let line = expr.line;
        match &expr.kind {
            ExprKind::Integer(n) => Ok(PerlValue::Integer(*n)),
            ExprKind::Float(f) => Ok(PerlValue::Float(*f)),
            ExprKind::String(s) => Ok(PerlValue::String(s.clone())),
            ExprKind::Undef => Ok(PerlValue::Undef),
            ExprKind::Regex(pattern, flags) => {
                let re = self.compile_regex(pattern, flags, line)?;
                Ok(PerlValue::Regex(Arc::new(re), pattern.clone()))
            }
            ExprKind::QW(words) => Ok(PerlValue::Array(
                words.iter().map(|w| PerlValue::String(w.clone())).collect(),
            )),

            // Interpolated strings
            ExprKind::InterpolatedString(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        StringPart::Literal(s) => result.push_str(s),
                        StringPart::ScalarVar(name) => {
                            let val = self.get_special_var(name);
                            result.push_str(&val.to_string());
                        }
                        StringPart::ArrayVar(name) => {
                            let arr = self.scope.get_array(name);
                            let joined = arr
                                .iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(" ");
                            result.push_str(&joined);
                        }
                        StringPart::Expr(e) => {
                            let val = self.eval_expr(e)?;
                            result.push_str(&val.to_string());
                        }
                    }
                }
                Ok(PerlValue::String(result))
            }

            // Variables
            ExprKind::ScalarVar(name) => Ok(self.get_special_var(name)),
            ExprKind::ArrayVar(name) => Ok(PerlValue::Array(self.scope.get_array(name))),
            ExprKind::HashVar(name) => Ok(PerlValue::Hash(self.scope.get_hash(name))),
            ExprKind::ArrayElement { array, index } => {
                let idx = self.eval_expr(index)?.to_int();
                Ok(self.scope.get_array_element(array, idx))
            }
            ExprKind::HashElement { hash, key } => {
                let k = self.eval_expr(key)?.to_string();
                Ok(self.scope.get_hash_element(hash, &k))
            }
            ExprKind::ArraySlice { array, indices } => {
                let mut result = Vec::new();
                for idx_expr in indices {
                    let idx = self.eval_expr(idx_expr)?.to_int();
                    result.push(self.scope.get_array_element(array, idx));
                }
                Ok(PerlValue::Array(result))
            }
            ExprKind::HashSlice { hash, keys } => {
                let mut result = Vec::new();
                for key_expr in keys {
                    let k = self.eval_expr(key_expr)?.to_string();
                    result.push(self.scope.get_hash_element(hash, &k));
                }
                Ok(PerlValue::Array(result))
            }

            // References
            ExprKind::ScalarRef(inner) => {
                let val = self.eval_expr(inner)?;
                Ok(PerlValue::ScalarRef(Arc::new(RwLock::new(val))))
            }
            ExprKind::ArrayRef(elems) => {
                let mut arr = Vec::with_capacity(elems.len());
                for e in elems {
                    arr.push(self.eval_expr(e)?);
                }
                Ok(PerlValue::ArrayRef(Arc::new(RwLock::new(arr))))
            }
            ExprKind::HashRef(pairs) => {
                let mut map = IndexMap::new();
                for (k, v) in pairs {
                    let key = self.eval_expr(k)?.to_string();
                    let val = self.eval_expr(v)?;
                    map.insert(key, val);
                }
                Ok(PerlValue::HashRef(Arc::new(RwLock::new(map))))
            }
            ExprKind::CodeRef { params, body } => {
                let captured = self.scope.capture();
                Ok(PerlValue::CodeRef(Arc::new(PerlSub {
                    name: "__ANON__".to_string(),
                    params: params.clone(),
                    body: body.clone(),
                    closure_env: Some(captured),
                })))
            }
            ExprKind::Deref { expr, kind } => {
                let val = self.eval_expr(expr)?;
                match kind {
                    Sigil::Scalar => match val {
                        PerlValue::ScalarRef(r) => Ok(r.read().clone()),
                        _ => Err(PerlError::runtime(
                            "Can't dereference non-reference as scalar",
                            line,
                        )
                        .into()),
                    },
                    Sigil::Array => match val {
                        PerlValue::ArrayRef(r) => Ok(PerlValue::Array(r.read().clone())),
                        _ => Err(PerlError::runtime(
                            "Can't dereference non-reference as array",
                            line,
                        )
                        .into()),
                    },
                    Sigil::Hash => match val {
                        PerlValue::HashRef(r) => Ok(PerlValue::Hash(r.read().clone())),
                        _ => Err(PerlError::runtime(
                            "Can't dereference non-reference as hash",
                            line,
                        )
                        .into()),
                    },
                }
            }
            ExprKind::ArrowDeref { expr, index, kind } => {
                let val = self.eval_expr(expr)?;
                match kind {
                    DerefKind::Array => {
                        let idx = self.eval_expr(index)?.to_int();
                        match val {
                            PerlValue::ArrayRef(r) => {
                                let arr = r.read();
                                let i = if idx < 0 {
                                    (arr.len() as i64 + idx) as usize
                                } else {
                                    idx as usize
                                };
                                Ok(arr.get(i).cloned().unwrap_or(PerlValue::Undef))
                            }
                            _ => Err(PerlError::runtime(
                                "Can't use arrow deref on non-array-ref",
                                line,
                            )
                            .into()),
                        }
                    }
                    DerefKind::Hash => {
                        let key = self.eval_expr(index)?.to_string();
                        match val {
                            PerlValue::HashRef(r) => {
                                let h = r.read();
                                Ok(h.get(&key).cloned().unwrap_or(PerlValue::Undef))
                            }
                            PerlValue::Blessed(b) => {
                                let data = b.data.read();
                                if let PerlValue::Hash(ref h) = *data {
                                    Ok(h.get(&key).cloned().unwrap_or(PerlValue::Undef))
                                } else {
                                    Err(PerlError::runtime(
                                        "Can't access hash field on non-hash blessed ref",
                                        line,
                                    )
                                    .into())
                                }
                            }
                            _ => Err(PerlError::runtime(
                                "Can't use arrow deref on non-hash-ref",
                                line,
                            )
                            .into()),
                        }
                    }
                    DerefKind::Call => {
                        // $coderef->(args)
                        if let ExprKind::List(ref arg_exprs) = index.kind {
                            let mut args = Vec::new();
                            for a in arg_exprs {
                                args.push(self.eval_expr(a)?);
                            }
                            match val {
                                PerlValue::CodeRef(sub) => self.call_sub(&sub, args, line),
                                _ => Err(PerlError::runtime("Not a code reference", line).into()),
                            }
                        } else {
                            Err(PerlError::runtime("Invalid call deref", line).into())
                        }
                    }
                }
            }

            // Binary operators
            ExprKind::BinOp { left, op, right } => {
                let lv = self.eval_expr(left)?;
                // Short-circuit for logical operators
                match op {
                    BinOp::LogAnd | BinOp::LogAndWord => {
                        if !lv.is_true() {
                            return Ok(lv);
                        }
                        return self.eval_expr(right);
                    }
                    BinOp::LogOr | BinOp::LogOrWord => {
                        if lv.is_true() {
                            return Ok(lv);
                        }
                        return self.eval_expr(right);
                    }
                    BinOp::DefinedOr => {
                        if !matches!(lv, PerlValue::Undef) {
                            return Ok(lv);
                        }
                        return self.eval_expr(right);
                    }
                    _ => {}
                }
                let rv = self.eval_expr(right)?;
                self.eval_binop(*op, &lv, &rv, line)
            }

            // Unary
            ExprKind::UnaryOp { op, expr } => match op {
                UnaryOp::PreIncrement => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
                        return Ok(self
                            .scope
                            .atomic_mutate(name, |v| PerlValue::Integer(v.to_int() + 1)));
                    }
                    let val = self.eval_expr(expr)?;
                    let new_val = PerlValue::Integer(val.to_int() + 1);
                    self.assign_value(expr, new_val.clone())?;
                    Ok(new_val)
                }
                UnaryOp::PreDecrement => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
                        return Ok(self
                            .scope
                            .atomic_mutate(name, |v| PerlValue::Integer(v.to_int() - 1)));
                    }
                    let val = self.eval_expr(expr)?;
                    let new_val = PerlValue::Integer(val.to_int() - 1);
                    self.assign_value(expr, new_val.clone())?;
                    Ok(new_val)
                }
                _ => {
                    let val = self.eval_expr(expr)?;
                    match op {
                        UnaryOp::Negate => match val {
                            PerlValue::Integer(n) => Ok(PerlValue::Integer(-n)),
                            _ => Ok(PerlValue::Float(-val.to_number())),
                        },
                        UnaryOp::LogNot => {
                            Ok(PerlValue::Integer(if val.is_true() { 0 } else { 1 }))
                        }
                        UnaryOp::BitNot => Ok(PerlValue::Integer(!val.to_int())),
                        UnaryOp::LogNotWord => {
                            Ok(PerlValue::Integer(if val.is_true() { 0 } else { 1 }))
                        }
                        UnaryOp::Ref => Ok(PerlValue::ScalarRef(Arc::new(RwLock::new(val)))),
                        _ => unreachable!(),
                    }
                }
            },

            ExprKind::PostfixOp { expr, op } => {
                // For scalar variables, use atomic_mutate_post to hold the lock
                // for the entire read-modify-write (critical for mysync).
                if let ExprKind::ScalarVar(name) = &expr.kind {
                    let f: fn(&PerlValue) -> PerlValue = match op {
                        PostfixOp::Increment => |v| PerlValue::Integer(v.to_int() + 1),
                        PostfixOp::Decrement => |v| PerlValue::Integer(v.to_int() - 1),
                    };
                    return Ok(self.scope.atomic_mutate_post(name, f));
                }
                let val = self.eval_expr(expr)?;
                let old = val.clone();
                let new_val = match op {
                    PostfixOp::Increment => PerlValue::Integer(val.to_int() + 1),
                    PostfixOp::Decrement => PerlValue::Integer(val.to_int() - 1),
                };
                self.assign_value(expr, new_val)?;
                Ok(old)
            }

            // Assignment
            ExprKind::Assign { target, value } => {
                let val = self.eval_expr(value)?;
                self.assign_value(target, val.clone())?;
                Ok(val)
            }
            ExprKind::CompoundAssign { target, op, value } => {
                // Evaluate the RHS first (before locking for atomic vars)
                let rhs = self.eval_expr(value)?;
                // For scalar targets, use atomic_mutate to hold the lock
                if let ExprKind::ScalarVar(name) = &target.kind {
                    let op = *op;
                    return Ok(self.scope.atomic_mutate(name, |old| match op {
                        BinOp::Add => match (old, &rhs) {
                            (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                                PerlValue::Integer(a.wrapping_add(*b))
                            }
                            _ => PerlValue::Float(old.to_number() + rhs.to_number()),
                        },
                        BinOp::Sub => match (old, &rhs) {
                            (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                                PerlValue::Integer(a.wrapping_sub(*b))
                            }
                            _ => PerlValue::Float(old.to_number() - rhs.to_number()),
                        },
                        BinOp::Mul => match (old, &rhs) {
                            (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                                PerlValue::Integer(a.wrapping_mul(*b))
                            }
                            _ => PerlValue::Float(old.to_number() * rhs.to_number()),
                        },
                        BinOp::Concat => {
                            let mut s = old.to_string();
                            rhs.append_to(&mut s);
                            PerlValue::String(s)
                        }
                        BinOp::BitAnd => {
                            if let Some(s) = crate::value::set_intersection(old, &rhs) {
                                s
                            } else {
                                PerlValue::Integer(old.to_int() & rhs.to_int())
                            }
                        }
                        BinOp::BitOr => {
                            if let Some(s) = crate::value::set_union(old, &rhs) {
                                s
                            } else {
                                PerlValue::Integer(old.to_int() | rhs.to_int())
                            }
                        }
                        BinOp::BitXor => PerlValue::Integer(old.to_int() ^ rhs.to_int()),
                        BinOp::ShiftLeft => PerlValue::Integer(old.to_int() << rhs.to_int()),
                        BinOp::ShiftRight => PerlValue::Integer(old.to_int() >> rhs.to_int()),
                        _ => PerlValue::Float(old.to_number() + rhs.to_number()),
                    }));
                }
                // For hash element targets: $h{key} += 1
                if let ExprKind::HashElement { hash, key } = &target.kind {
                    let k = self.eval_expr(key)?.to_string();
                    let op = *op;
                    return Ok(self.scope.atomic_hash_mutate(hash, &k, |old| match op {
                        BinOp::Add => match (old, &rhs) {
                            (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                                PerlValue::Integer(a.wrapping_add(*b))
                            }
                            _ => PerlValue::Float(old.to_number() + rhs.to_number()),
                        },
                        BinOp::Sub => match (old, &rhs) {
                            (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                                PerlValue::Integer(a.wrapping_sub(*b))
                            }
                            _ => PerlValue::Float(old.to_number() - rhs.to_number()),
                        },
                        BinOp::Concat => {
                            let mut s = old.to_string();
                            rhs.append_to(&mut s);
                            PerlValue::String(s)
                        }
                        _ => PerlValue::Float(old.to_number() + rhs.to_number()),
                    }));
                }
                // For array element targets: $a[i] += 1
                if let ExprKind::ArrayElement { array, index } = &target.kind {
                    let idx = self.eval_expr(index)?.to_int();
                    let op = *op;
                    return Ok(self.scope.atomic_array_mutate(array, idx, |old| match op {
                        BinOp::Add => match (old, &rhs) {
                            (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                                PerlValue::Integer(a.wrapping_add(*b))
                            }
                            _ => PerlValue::Float(old.to_number() + rhs.to_number()),
                        },
                        _ => PerlValue::Float(old.to_number() + rhs.to_number()),
                    }));
                }
                let old = self.eval_expr(target)?;
                let new_val = self.eval_binop(*op, &old, &rhs, line)?;
                self.assign_value(target, new_val.clone())?;
                Ok(new_val)
            }

            // Ternary
            ExprKind::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                let cond = self.eval_expr(condition)?;
                if cond.is_true() {
                    self.eval_expr(then_expr)
                } else {
                    self.eval_expr(else_expr)
                }
            }

            // Range
            ExprKind::Range { from, to } => {
                let f = self.eval_expr(from)?.to_int();
                let t = self.eval_expr(to)?.to_int();
                let list: Vec<PerlValue> = (f..=t).map(PerlValue::Integer).collect();
                Ok(PerlValue::Array(list))
            }

            // Repeat
            ExprKind::Repeat { expr, count } => {
                let val = self.eval_expr(expr)?;
                let n = self.eval_expr(count)?.to_int().max(0) as usize;
                match val {
                    PerlValue::String(s) => Ok(PerlValue::String(s.repeat(n))),
                    PerlValue::Array(a) => {
                        let mut result = Vec::with_capacity(a.len() * n);
                        for _ in 0..n {
                            result.extend(a.iter().cloned());
                        }
                        Ok(PerlValue::Array(result))
                    }
                    _ => Ok(PerlValue::String(val.to_string().repeat(n))),
                }
            }

            // Function calls
            ExprKind::FuncCall { name, args } => {
                let mut arg_vals = Vec::with_capacity(args.len());
                for a in args {
                    let v = self.eval_expr(a)?;
                    // Flatten arrays in argument lists
                    match v {
                        PerlValue::Array(items) => arg_vals.extend(items),
                        other => arg_vals.push(other),
                    }
                }
                self.call_named_sub(name, arg_vals, line)
            }
            ExprKind::MethodCall {
                object,
                method,
                args,
            } => {
                let obj = self.eval_expr(object)?;
                let mut arg_vals = vec![obj.clone()];
                for a in args {
                    arg_vals.push(self.eval_expr(a)?);
                }
                if let Some(r) =
                    crate::pchannel::dispatch_method(&obj, method, &arg_vals[1..], line)
                {
                    return r.map_err(Into::into);
                }
                if let Some(r) = self.try_native_method(&obj, method, &arg_vals[1..], line) {
                    return r.map_err(Into::into);
                }
                // Get class name
                let class = match &obj {
                    PerlValue::Blessed(b) => b.class.clone(),
                    PerlValue::String(s) => s.clone(), // Class->method()
                    _ => {
                        return Err(
                            PerlError::runtime("Can't call method on non-object", line).into()
                        )
                    }
                };
                let full_name = format!("{}::{}", class, method);
                if let Some(sub) = self.subs.get(&full_name).cloned() {
                    self.call_sub(&sub, arg_vals, line)
                } else if method == "new" {
                    // Default constructor
                    self.builtin_new(&class, arg_vals, line)
                } else {
                    Err(PerlError::runtime(
                        format!(
                            "Can't locate method \"{}\" in package \"{}\"",
                            method, class
                        ),
                        line,
                    )
                    .into())
                }
            }

            // Print/Say/Printf
            ExprKind::Print { handle, args } => {
                self.exec_print(handle.as_deref(), args, false, line)
            }
            ExprKind::Say { handle, args } => self.exec_print(handle.as_deref(), args, true, line),
            ExprKind::Printf { handle, args } => self.exec_printf(handle.as_deref(), args, line),
            ExprKind::Die(args) => {
                let mut msg = String::new();
                for a in args {
                    let v = self.eval_expr(a)?;
                    msg.push_str(&v.to_string());
                }
                if msg.is_empty() {
                    msg = "Died".to_string();
                }
                if !msg.ends_with('\n') {
                    msg.push_str(&format!(" at {} line {}", self.file, line));
                    msg.push('\n');
                }
                Err(PerlError::die(msg, line).into())
            }
            ExprKind::Warn(args) => {
                let mut msg = String::new();
                for a in args {
                    let v = self.eval_expr(a)?;
                    msg.push_str(&v.to_string());
                }
                if msg.is_empty() {
                    msg = "Warning: something's wrong".to_string();
                }
                if !msg.ends_with('\n') {
                    msg.push_str(&format!(" at {} line {}", self.file, line));
                    msg.push('\n');
                }
                eprint!("{}", msg);
                Ok(PerlValue::Integer(1))
            }

            // Regex
            ExprKind::Match {
                expr,
                pattern,
                flags,
                scalar_g,
            } => {
                let val = self.eval_expr(expr)?;
                let s = val.to_string();
                let pos_key = match &expr.kind {
                    ExprKind::ScalarVar(n) => n.as_str(),
                    _ => "_",
                };
                self.regex_match_execute(s, pattern, flags, *scalar_g, pos_key, line)
            }
            ExprKind::Substitution {
                expr,
                pattern,
                replacement,
                flags,
            } => {
                let val = self.eval_expr(expr)?;
                let s = val.to_string();
                let re = self.compile_regex(pattern, flags, line)?;
                let (new_s, count) = if flags.contains('g') {
                    let count = re.find_iter(&s).count();
                    (re.replace_all(&s, replacement.as_str()).to_string(), count)
                } else {
                    let count = if re.is_match(&s) { 1 } else { 0 };
                    (re.replace(&s, replacement.as_str()).to_string(), count)
                };
                self.assign_value(expr, PerlValue::String(new_s))?;
                Ok(PerlValue::Integer(count as i64))
            }
            ExprKind::Transliterate {
                expr,
                from,
                to,
                flags,
            } => {
                let val = self.eval_expr(expr)?;
                let s = val.to_string();
                let from_chars: Vec<char> = from.chars().collect();
                let to_chars: Vec<char> = to.chars().collect();
                let mut count = 0i64;
                let new_s: String = s
                    .chars()
                    .map(|c| {
                        if let Some(pos) = from_chars.iter().position(|&fc| fc == c) {
                            count += 1;
                            to_chars.get(pos).or(to_chars.last()).copied().unwrap_or(c)
                        } else {
                            c
                        }
                    })
                    .collect();
                if !flags.contains('d') || flags.contains('r') {
                    self.assign_value(expr, PerlValue::String(new_s))?;
                }
                Ok(PerlValue::Integer(count))
            }

            // List operations
            ExprKind::MapExpr { block, list } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let mut result = Vec::new();
                for item in items {
                    self.scope.set_scalar("_", item);
                    let val = self.exec_block(block)?;
                    match val {
                        PerlValue::Array(a) => result.extend(a),
                        other => result.push(other),
                    }
                }
                Ok(PerlValue::Array(result))
            }
            ExprKind::GrepExpr { block, list } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let mut result = Vec::new();
                for item in items {
                    self.scope.set_scalar("_", item.clone());
                    let val = self.exec_block(block)?;
                    if val.is_true() {
                        result.push(item);
                    }
                }
                Ok(PerlValue::Array(result))
            }
            ExprKind::SortExpr { cmp, list } => {
                let list_val = self.eval_expr(list)?;
                let mut items = list_val.to_list();
                if let Some(cmp_block) = cmp {
                    // Custom comparator
                    let cmp_block = cmp_block.clone();
                    items.sort_by(|a, b| {
                        self.scope.set_scalar("a", a.clone());
                        self.scope.set_scalar("b", b.clone());
                        match self.exec_block(&cmp_block) {
                            Ok(v) => {
                                let n = v.to_int();
                                if n < 0 {
                                    std::cmp::Ordering::Less
                                } else if n > 0 {
                                    std::cmp::Ordering::Greater
                                } else {
                                    std::cmp::Ordering::Equal
                                }
                            }
                            Err(_) => std::cmp::Ordering::Equal,
                        }
                    });
                } else {
                    items.sort_by_key(|a| a.to_string());
                }
                Ok(PerlValue::Array(items))
            }
            ExprKind::ReverseExpr(list) => {
                let val = self.eval_expr(list)?;
                match val {
                    PerlValue::Array(mut a) => {
                        a.reverse();
                        Ok(PerlValue::Array(a))
                    }
                    PerlValue::String(s) => Ok(PerlValue::String(s.chars().rev().collect())),
                    other => {
                        let s: String = other.to_string().chars().rev().collect();
                        Ok(PerlValue::String(s))
                    }
                }
            }

            // ── Parallel operations (rayon-powered) ──
            ExprKind::PMapExpr { block, list } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();

                let results: Vec<PerlValue> = items
                    .into_par_iter()
                    .map(|item| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        local_interp
                            .scope
                            .restore_atomics(&atomic_arrays, &atomic_hashes);
                        local_interp.scope.set_scalar("_", item);
                        match local_interp.exec_block(&block) {
                            Ok(val) => val,
                            Err(_) => PerlValue::Undef,
                        }
                    })
                    .collect();
                Ok(PerlValue::Array(results))
            }
            ExprKind::PGrepExpr { block, list } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();

                let results: Vec<PerlValue> = items
                    .into_par_iter()
                    .filter(|item| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        local_interp
                            .scope
                            .restore_atomics(&atomic_arrays, &atomic_hashes);
                        local_interp.scope.set_scalar("_", item.clone());
                        match local_interp.exec_block(&block) {
                            Ok(val) => val.is_true(),
                            Err(_) => false,
                        }
                    })
                    .collect();
                Ok(PerlValue::Array(results))
            }
            ExprKind::PForExpr { block, list } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();

                items.into_par_iter().for_each(|item| {
                    let mut local_interp = Interpreter::new();
                    local_interp.subs = subs.clone();
                    local_interp.scope.restore_capture(&scope_capture);
                    local_interp
                        .scope
                        .restore_atomics(&atomic_arrays, &atomic_hashes);
                    local_interp.scope.set_scalar("_", item);
                    let _ = local_interp.exec_block(&block);
                });
                Ok(PerlValue::Undef)
            }
            ExprKind::FanExpr { count, block } => {
                let n = self.eval_expr(count)?.to_int().max(0) as usize;
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();

                (0..n).into_par_iter().for_each(|i| {
                    let mut local_interp = Interpreter::new();
                    local_interp.subs = subs.clone();
                    local_interp.scope.restore_capture(&scope_capture);
                    local_interp
                        .scope
                        .restore_atomics(&atomic_arrays, &atomic_hashes);
                    local_interp
                        .scope
                        .set_scalar("_", PerlValue::Integer(i as i64));
                    crate::parallel_trace::fan_worker_set_index(Some(i as i64));
                    let _ = local_interp.exec_block(&block);
                    crate::parallel_trace::fan_worker_set_index(None);
                });
                Ok(PerlValue::Undef)
            }
            ExprKind::AsyncBlock { body } => Ok(self.spawn_async_block(body)),
            ExprKind::Trace { body } => {
                crate::parallel_trace::trace_enter();
                let out = self.exec_block(body);
                crate::parallel_trace::trace_leave();
                out
            }
            ExprKind::Await(expr) => {
                let v = self.eval_expr(expr)?;
                match v {
                    PerlValue::AsyncTask(t) => t.await_result().map_err(FlowOrError::from),
                    other => Ok(other),
                }
            }
            ExprKind::Slurp(e) => {
                let path = self.eval_expr(e)?.to_string();
                std::fs::read_to_string(&path)
                    .map(PerlValue::String)
                    .map_err(|e| {
                        FlowOrError::Error(PerlError::runtime(format!("slurp: {}", e), line))
                    })
            }
            ExprKind::FetchUrl(e) => {
                let url = self.eval_expr(e)?.to_string();
                ureq::get(&url)
                    .call()
                    .map_err(|e| {
                        FlowOrError::Error(PerlError::runtime(format!("fetch_url: {}", e), line))
                    })
                    .and_then(|r| {
                        r.into_string().map(PerlValue::String).map_err(|e| {
                            FlowOrError::Error(PerlError::runtime(
                                format!("fetch_url: {}", e),
                                line,
                            ))
                        })
                    })
            }
            ExprKind::Pchannel => Ok(crate::pchannel::create_pair()),
            ExprKind::PSortExpr { cmp, list } => {
                let list_val = self.eval_expr(list)?;
                let mut items = list_val.to_list();
                if let Some(cmp_block) = cmp {
                    let cmp_block = cmp_block.clone();
                    let subs = self.subs.clone();
                    let scope_capture = self.scope.capture();
                    items.par_sort_by(|a, b| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        local_interp.scope.set_scalar("a", a.clone());
                        local_interp.scope.set_scalar("b", b.clone());
                        match local_interp.exec_block(&cmp_block) {
                            Ok(v) => {
                                let n = v.to_int();
                                if n < 0 {
                                    std::cmp::Ordering::Less
                                } else if n > 0 {
                                    std::cmp::Ordering::Greater
                                } else {
                                    std::cmp::Ordering::Equal
                                }
                            }
                            Err(_) => std::cmp::Ordering::Equal,
                        }
                    });
                } else {
                    items.par_sort_by(|a, b| a.to_string().cmp(&b.to_string()));
                }
                Ok(PerlValue::Array(items))
            }

            ExprKind::PReduceExpr { block, list } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                if items.is_empty() {
                    return Ok(PerlValue::Undef);
                }
                if items.len() == 1 {
                    return Ok(items.into_iter().next().unwrap());
                }
                let block = block.clone();
                let subs = self.subs.clone();
                let scope_capture = self.scope.capture();

                let result = items.into_par_iter().reduce_with(|a, b| {
                    let mut local_interp = Interpreter::new();
                    local_interp.subs = subs.clone();
                    local_interp.scope.restore_capture(&scope_capture);
                    local_interp.scope.set_scalar("a", a);
                    local_interp.scope.set_scalar("b", b);
                    match local_interp.exec_block(&block) {
                        Ok(val) => val,
                        Err(_) => PerlValue::Undef,
                    }
                });
                Ok(result.unwrap_or(PerlValue::Undef))
            }

            // Array ops
            ExprKind::Push { array, values } => {
                let arr_name = self.extract_array_name(array)?;
                for v in values {
                    let val = self.eval_expr(v)?;
                    match val {
                        PerlValue::Array(items) => {
                            for item in items {
                                self.scope.push_to_array(&arr_name, item);
                            }
                        }
                        other => self.scope.push_to_array(&arr_name, other),
                    }
                }
                let len = self.scope.array_len(&arr_name);
                Ok(PerlValue::Integer(len as i64))
            }
            ExprKind::Pop(array) => {
                let arr_name = self.extract_array_name(array)?;
                Ok(self.scope.pop_from_array(&arr_name))
            }
            ExprKind::Shift(array) => {
                let arr_name = self.extract_array_name(array)?;
                Ok(self.scope.shift_from_array(&arr_name))
            }
            ExprKind::Unshift { array, values } => {
                let arr_name = self.extract_array_name(array)?;
                let mut vals = Vec::new();
                for v in values {
                    let val = self.eval_expr(v)?;
                    vals.push(val);
                }
                let arr = self.scope.get_array_mut(&arr_name);
                for (i, v) in vals.into_iter().enumerate() {
                    arr.insert(i, v);
                }
                let len = arr.len();
                Ok(PerlValue::Integer(len as i64))
            }
            ExprKind::Splice {
                array,
                offset,
                length,
                replacement,
            } => {
                let arr_name = self.extract_array_name(array)?;
                let off = if let Some(o) = offset {
                    self.eval_expr(o)?.to_int() as usize
                } else {
                    0
                };
                let len = if let Some(l) = length {
                    self.eval_expr(l)?.to_int() as usize
                } else {
                    let arr = self.scope.get_array_mut(&arr_name);
                    arr.len() - off
                };
                let mut rep_vals = Vec::new();
                for r in replacement {
                    rep_vals.push(self.eval_expr(r)?);
                }
                let arr = self.scope.get_array_mut(&arr_name);
                let end = (off + len).min(arr.len());
                let removed: Vec<PerlValue> = arr.drain(off..end).collect();
                for (i, v) in rep_vals.into_iter().enumerate() {
                    arr.insert(off + i, v);
                }
                Ok(PerlValue::Array(removed))
            }
            ExprKind::Delete(expr) => match &expr.kind {
                ExprKind::HashElement { hash, key } => {
                    let k = self.eval_expr(key)?.to_string();
                    Ok(self.scope.delete_hash_element(hash, &k))
                }
                _ => Err(PerlError::runtime("delete requires hash element", line).into()),
            },
            ExprKind::Exists(expr) => match &expr.kind {
                ExprKind::HashElement { hash, key } => {
                    let k = self.eval_expr(key)?.to_string();
                    Ok(PerlValue::Integer(
                        if self.scope.exists_hash_element(hash, &k) {
                            1
                        } else {
                            0
                        },
                    ))
                }
                _ => Err(PerlError::runtime("exists requires hash element", line).into()),
            },
            ExprKind::Keys(expr) => {
                let val = self.eval_expr(expr)?;
                match val {
                    PerlValue::Hash(h) => Ok(PerlValue::Array(
                        h.keys().map(|k| PerlValue::String(k.clone())).collect(),
                    )),
                    PerlValue::HashRef(r) => Ok(PerlValue::Array(
                        r.read()
                            .keys()
                            .map(|k| PerlValue::String(k.clone()))
                            .collect(),
                    )),
                    _ => Err(PerlError::runtime("keys requires hash", line).into()),
                }
            }
            ExprKind::Values(expr) => {
                let val = self.eval_expr(expr)?;
                match val {
                    PerlValue::Hash(h) => Ok(PerlValue::Array(h.values().cloned().collect())),
                    PerlValue::HashRef(r) => {
                        Ok(PerlValue::Array(r.read().values().cloned().collect()))
                    }
                    _ => Err(PerlError::runtime("values requires hash", line).into()),
                }
            }
            ExprKind::Each(_) => {
                // Simplified: returns empty list (full iterator state would need more work)
                Ok(PerlValue::Array(vec![]))
            }

            // String ops
            ExprKind::Chomp(expr) => {
                let val = self.eval_expr(expr)?;
                let mut s = val.to_string();
                let removed = if s.ends_with('\n') {
                    s.pop();
                    1
                } else {
                    0
                };
                self.assign_value(expr, PerlValue::String(s))?;
                Ok(PerlValue::Integer(removed))
            }
            ExprKind::Chop(expr) => {
                let val = self.eval_expr(expr)?;
                let mut s = val.to_string();
                let chopped = s
                    .pop()
                    .map(|c| PerlValue::String(c.to_string()))
                    .unwrap_or(PerlValue::Undef);
                self.assign_value(expr, PerlValue::String(s))?;
                Ok(chopped)
            }
            ExprKind::Length(expr) => {
                let val = self.eval_expr(expr)?;
                match val {
                    PerlValue::Array(a) => Ok(PerlValue::Integer(a.len() as i64)),
                    PerlValue::Hash(h) => Ok(PerlValue::Integer(h.len() as i64)),
                    other => Ok(PerlValue::Integer(other.to_string().len() as i64)),
                }
            }
            ExprKind::Substr {
                string,
                offset,
                length,
                replacement,
            } => {
                let s = self.eval_expr(string)?.to_string();
                let off = self.eval_expr(offset)?.to_int();
                let start = if off < 0 {
                    (s.len() as i64 + off).max(0) as usize
                } else {
                    off as usize
                };
                let len = if let Some(l) = length {
                    self.eval_expr(l)?.to_int() as usize
                } else {
                    s.len() - start
                };
                let end = (start + len).min(s.len());
                let result = s.get(start..end).unwrap_or("").to_string();
                if let Some(rep) = replacement {
                    let rep_s = self.eval_expr(rep)?.to_string();
                    let mut new_s = String::new();
                    new_s.push_str(&s[..start]);
                    new_s.push_str(&rep_s);
                    new_s.push_str(&s[end..]);
                    self.assign_value(string, PerlValue::String(new_s))?;
                }
                Ok(PerlValue::String(result))
            }
            ExprKind::Index {
                string,
                substr,
                position,
            } => {
                let s = self.eval_expr(string)?.to_string();
                let sub = self.eval_expr(substr)?.to_string();
                let pos = if let Some(p) = position {
                    self.eval_expr(p)?.to_int() as usize
                } else {
                    0
                };
                let result = s[pos..].find(&sub).map(|i| (i + pos) as i64).unwrap_or(-1);
                Ok(PerlValue::Integer(result))
            }
            ExprKind::Rindex {
                string,
                substr,
                position,
            } => {
                let s = self.eval_expr(string)?.to_string();
                let sub = self.eval_expr(substr)?.to_string();
                let end = if let Some(p) = position {
                    self.eval_expr(p)?.to_int() as usize + sub.len()
                } else {
                    s.len()
                };
                let search = &s[..end.min(s.len())];
                let result = search.rfind(&sub).map(|i| i as i64).unwrap_or(-1);
                Ok(PerlValue::Integer(result))
            }
            ExprKind::Sprintf { format, args } => {
                let fmt = self.eval_expr(format)?.to_string();
                let mut arg_vals = Vec::new();
                for a in args {
                    arg_vals.push(self.eval_expr(a)?);
                }
                Ok(PerlValue::String(perl_sprintf(&fmt, &arg_vals)))
            }
            ExprKind::JoinExpr { separator, list } => {
                let sep = self.eval_expr(separator)?.to_string();
                let items = self.eval_expr(list)?.to_list();
                let joined = items
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(&sep);
                Ok(PerlValue::String(joined))
            }
            ExprKind::SplitExpr {
                pattern,
                string,
                limit,
            } => {
                let pat = self.eval_expr(pattern)?.to_string();
                let s = self.eval_expr(string)?.to_string();
                let lim = if let Some(l) = limit {
                    self.eval_expr(l)?.to_int() as usize
                } else {
                    0
                };
                let re = self.compile_regex(&pat, "", line)?;
                let parts: Vec<PerlValue> = if lim > 0 {
                    re.splitn(&s, lim)
                        .map(|p| PerlValue::String(p.to_string()))
                        .collect()
                } else {
                    re.split(&s)
                        .map(|p| PerlValue::String(p.to_string()))
                        .collect()
                };
                Ok(PerlValue::Array(parts))
            }

            // Numeric
            ExprKind::Abs(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::Float(val.to_number().abs()))
            }
            ExprKind::Int(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::Integer(val.to_number() as i64))
            }
            ExprKind::Sqrt(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::Float(val.to_number().sqrt()))
            }
            ExprKind::Sin(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::Float(val.to_number().sin()))
            }
            ExprKind::Cos(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::Float(val.to_number().cos()))
            }
            ExprKind::Atan2 { y, x } => {
                let yv = self.eval_expr(y)?.to_number();
                let xv = self.eval_expr(x)?.to_number();
                Ok(PerlValue::Float(yv.atan2(xv)))
            }
            ExprKind::Exp(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::Float(val.to_number().exp()))
            }
            ExprKind::Log(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::Float(val.to_number().ln()))
            }
            ExprKind::Rand(upper) => {
                let u = match upper {
                    Some(e) => self.eval_expr(e)?.to_number(),
                    None => 1.0,
                };
                Ok(PerlValue::Float(self.perl_rand(u)))
            }
            ExprKind::Srand(seed) => {
                let s = match seed {
                    Some(e) => Some(self.eval_expr(e)?.to_number()),
                    None => None,
                };
                Ok(PerlValue::Integer(self.perl_srand(s)))
            }
            ExprKind::Hex(expr) => {
                let val = self.eval_expr(expr)?.to_string();
                let clean = val.trim().trim_start_matches("0x").trim_start_matches("0X");
                let n = i64::from_str_radix(clean, 16).unwrap_or(0);
                Ok(PerlValue::Integer(n))
            }
            ExprKind::Oct(expr) => {
                let val = self.eval_expr(expr)?.to_string();
                let s = val.trim();
                let n = if s.starts_with("0x") || s.starts_with("0X") {
                    i64::from_str_radix(&s[2..], 16).unwrap_or(0)
                } else if s.starts_with("0b") || s.starts_with("0B") {
                    i64::from_str_radix(&s[2..], 2).unwrap_or(0)
                } else {
                    i64::from_str_radix(s.trim_start_matches('0'), 8).unwrap_or(0)
                };
                Ok(PerlValue::Integer(n))
            }

            // Case
            ExprKind::Lc(expr) => Ok(PerlValue::String(
                self.eval_expr(expr)?.to_string().to_lowercase(),
            )),
            ExprKind::Uc(expr) => Ok(PerlValue::String(
                self.eval_expr(expr)?.to_string().to_uppercase(),
            )),
            ExprKind::Lcfirst(expr) => {
                let s = self.eval_expr(expr)?.to_string();
                let mut chars = s.chars();
                let result = match chars.next() {
                    Some(c) => c.to_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                };
                Ok(PerlValue::String(result))
            }
            ExprKind::Ucfirst(expr) => {
                let s = self.eval_expr(expr)?.to_string();
                let mut chars = s.chars();
                let result = match chars.next() {
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                };
                Ok(PerlValue::String(result))
            }
            ExprKind::Fc(expr) => Ok(PerlValue::String(default_case_fold_str(
                &self.eval_expr(expr)?.to_string(),
            ))),
            ExprKind::Crypt { plaintext, salt } => {
                let p = self.eval_expr(plaintext)?.to_string();
                let sl = self.eval_expr(salt)?.to_string();
                Ok(PerlValue::String(perl_crypt(&p, &sl)))
            }
            ExprKind::Pos(e) => {
                let key = match e {
                    None => "_".to_string(),
                    Some(expr) => match &expr.kind {
                        ExprKind::ScalarVar(n) => n.clone(),
                        _ => {
                            return Err(FlowOrError::Error(PerlError::runtime(
                                "pos requires a simple scalar",
                                line,
                            )))
                        }
                    },
                };
                Ok(self
                    .regex_pos
                    .get(&key)
                    .copied()
                    .flatten()
                    .map(|p| PerlValue::Integer(p as i64))
                    .unwrap_or(PerlValue::Undef))
            }
            ExprKind::Study(expr) => {
                let s = self.eval_expr(expr)?.to_string();
                Ok(PerlValue::Integer(s.len() as i64))
            }

            // Type
            ExprKind::Defined(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::Integer(if matches!(val, PerlValue::Undef) {
                    0
                } else {
                    1
                }))
            }
            ExprKind::Ref(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(val.ref_type())
            }
            ExprKind::ScalarContext(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(val.scalar_context())
            }

            // Char
            ExprKind::Chr(expr) => {
                let n = self.eval_expr(expr)?.to_int() as u32;
                Ok(PerlValue::String(
                    char::from_u32(n).map(|c| c.to_string()).unwrap_or_default(),
                ))
            }
            ExprKind::Ord(expr) => {
                let s = self.eval_expr(expr)?.to_string();
                Ok(PerlValue::Integer(
                    s.chars().next().map(|c| c as i64).unwrap_or(0),
                ))
            }

            // I/O
            ExprKind::Open { handle, mode, file } => {
                let handle_name = self.eval_expr(handle)?.to_string();
                let mode_s = self.eval_expr(mode)?.to_string();
                let (actual_mode, path) = if let Some(f) = file {
                    (mode_s, self.eval_expr(f)?.to_string())
                } else {
                    // Parse mode from combined string: ">file", "<file", ">>file"
                    if let Some(rest) = mode_s.strip_prefix(">>") {
                        (">>".to_string(), rest.trim().to_string())
                    } else if let Some(rest) = mode_s.strip_prefix('>') {
                        (">".to_string(), rest.trim().to_string())
                    } else if let Some(rest) = mode_s.strip_prefix('<') {
                        ("<".to_string(), rest.trim().to_string())
                    } else {
                        ("<".to_string(), mode_s)
                    }
                };
                match actual_mode.as_str() {
                    "<" => {
                        let file = std::fs::File::open(&path).map_err(|e| {
                            self.errno = e.to_string();
                            PerlError::runtime(format!("Can't open '{}': {}", path, e), line)
                        })?;
                        self.input_handles
                            .insert(handle_name, BufReader::new(Box::new(file)));
                    }
                    ">" => {
                        let file = std::fs::File::create(&path).map_err(|e| {
                            self.errno = e.to_string();
                            PerlError::runtime(format!("Can't open '{}': {}", path, e), line)
                        })?;
                        self.output_handles.insert(handle_name, Box::new(file));
                    }
                    ">>" => {
                        let file = std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(&path)
                            .map_err(|e| {
                                self.errno = e.to_string();
                                PerlError::runtime(format!("Can't open '{}': {}", path, e), line)
                            })?;
                        self.output_handles.insert(handle_name, Box::new(file));
                    }
                    _ => {
                        return Err(PerlError::runtime(
                            format!("Unknown open mode '{}'", actual_mode),
                            line,
                        )
                        .into());
                    }
                }
                Ok(PerlValue::Integer(1))
            }
            ExprKind::Close(expr) => {
                let name = self.eval_expr(expr)?.to_string();
                self.output_handles.remove(&name);
                self.input_handles.remove(&name);
                Ok(PerlValue::Integer(1))
            }
            ExprKind::ReadLine(handle) => {
                let handle_name = handle.as_deref().unwrap_or("STDIN");
                let mut line_str = String::new();
                if handle_name == "STDIN" {
                    match io::stdin().lock().read_line(&mut line_str) {
                        Ok(0) => Ok(PerlValue::Undef),
                        Ok(_) => {
                            self.line_number += 1;
                            Ok(PerlValue::String(line_str))
                        }
                        Err(e) => {
                            self.errno = e.to_string();
                            Ok(PerlValue::Undef)
                        }
                    }
                } else if let Some(reader) = self.input_handles.get_mut(handle_name) {
                    match reader.read_line(&mut line_str) {
                        Ok(0) => Ok(PerlValue::Undef),
                        Ok(_) => {
                            self.line_number += 1;
                            Ok(PerlValue::String(line_str))
                        }
                        Err(e) => {
                            self.errno = e.to_string();
                            Ok(PerlValue::Undef)
                        }
                    }
                } else {
                    Ok(PerlValue::Undef)
                }
            }
            ExprKind::Eof(expr) => {
                if let Some(e) = expr {
                    let name = self.eval_expr(e)?.to_string();
                    let at_eof = !self.input_handles.contains_key(&name);
                    Ok(PerlValue::Integer(if at_eof { 1 } else { 0 }))
                } else {
                    Ok(PerlValue::Integer(0))
                }
            }

            ExprKind::Opendir { handle, path } => {
                let h = self.eval_expr(handle)?.to_string();
                let p = self.eval_expr(path)?.to_string();
                Ok(self.opendir_handle(&h, &p))
            }
            ExprKind::Readdir(e) => {
                let h = self.eval_expr(e)?.to_string();
                Ok(self.readdir_handle(&h))
            }
            ExprKind::Closedir(e) => {
                let h = self.eval_expr(e)?.to_string();
                Ok(self.closedir_handle(&h))
            }
            ExprKind::Rewinddir(e) => {
                let h = self.eval_expr(e)?.to_string();
                Ok(self.rewinddir_handle(&h))
            }
            ExprKind::Telldir(e) => {
                let h = self.eval_expr(e)?.to_string();
                Ok(self.telldir_handle(&h))
            }
            ExprKind::Seekdir { handle, position } => {
                let h = self.eval_expr(handle)?.to_string();
                let pos = self.eval_expr(position)?.to_int().max(0) as usize;
                Ok(self.seekdir_handle(&h, pos))
            }

            // File tests
            ExprKind::FileTest { op, expr } => {
                let path = self.eval_expr(expr)?.to_string();
                let result = match op {
                    'e' => std::path::Path::new(&path).exists(),
                    'f' => std::path::Path::new(&path).is_file(),
                    'd' => std::path::Path::new(&path).is_dir(),
                    'l' => std::path::Path::new(&path).is_symlink(),
                    'r' => std::fs::metadata(&path).is_ok(), // simplified
                    'w' => std::fs::metadata(&path).is_ok(),
                    's' => std::fs::metadata(&path)
                        .map(|m| m.len() > 0)
                        .unwrap_or(false),
                    'z' => std::fs::metadata(&path)
                        .map(|m| m.len() == 0)
                        .unwrap_or(true),
                    _ => false,
                };
                Ok(PerlValue::Integer(if result { 1 } else { 0 }))
            }

            // System
            ExprKind::System(args) => {
                let mut cmd_args = Vec::new();
                for a in args {
                    cmd_args.push(self.eval_expr(a)?.to_string());
                }
                if cmd_args.is_empty() {
                    return Ok(PerlValue::Integer(-1));
                }
                let status = Command::new("sh")
                    .arg("-c")
                    .arg(cmd_args.join(" "))
                    .status();
                match status {
                    Ok(s) => Ok(PerlValue::Integer(s.code().unwrap_or(-1) as i64)),
                    Err(e) => {
                        self.errno = e.to_string();
                        Ok(PerlValue::Integer(-1))
                    }
                }
            }
            ExprKind::Exec(args) => {
                let mut cmd_args = Vec::new();
                for a in args {
                    cmd_args.push(self.eval_expr(a)?.to_string());
                }
                if cmd_args.is_empty() {
                    return Ok(PerlValue::Integer(-1));
                }
                let status = Command::new("sh")
                    .arg("-c")
                    .arg(cmd_args.join(" "))
                    .status();
                match status {
                    Ok(s) => std::process::exit(s.code().unwrap_or(-1)),
                    Err(e) => {
                        self.errno = e.to_string();
                        Ok(PerlValue::Integer(-1))
                    }
                }
            }
            ExprKind::Eval(expr) => {
                match &expr.kind {
                    ExprKind::CodeRef { body, .. } => match self.exec_block(body) {
                        Ok(v) => {
                            self.eval_error = String::new();
                            Ok(v)
                        }
                        Err(FlowOrError::Error(e)) => {
                            self.eval_error = e.to_string();
                            Ok(PerlValue::Undef)
                        }
                        Err(FlowOrError::Flow(f)) => Err(FlowOrError::Flow(f)),
                    },
                    _ => {
                        let code = self.eval_expr(expr)?.to_string();
                        // Parse and execute the string as Perl code
                        match crate::parse_and_run_string(&code, self) {
                            Ok(v) => {
                                self.eval_error = String::new();
                                Ok(v)
                            }
                            Err(e) => {
                                self.eval_error = e.to_string();
                                Ok(PerlValue::Undef)
                            }
                        }
                    }
                }
            }
            ExprKind::Do(expr) => {
                let val = self.eval_expr(expr)?;
                match &expr.kind {
                    ExprKind::CodeRef { body, .. } => self.exec_block(body),
                    _ => {
                        let filename = val.to_string();
                        match std::fs::read_to_string(&filename) {
                            Ok(code) => match crate::parse_and_run_string(&code, self) {
                                Ok(v) => Ok(v),
                                Err(e) => {
                                    self.errno = e.to_string();
                                    Ok(PerlValue::Undef)
                                }
                            },
                            Err(e) => {
                                self.errno = e.to_string();
                                Ok(PerlValue::Undef)
                            }
                        }
                    }
                }
            }
            ExprKind::Require(expr) => {
                let filename = self.eval_expr(expr)?.to_string();
                let path = filename.replace("::", "/") + ".pm";
                // Search @INC (simplified: just current dir and /usr/lib/perl5)
                for dir in [".", "/usr/lib/perl5", "/usr/share/perl5"] {
                    let full = format!("{}/{}", dir, path);
                    if std::path::Path::new(&full).exists() {
                        let code = std::fs::read_to_string(&full).map_err(|e| {
                            PerlError::runtime(format!("Can't open {}: {}", full, e), line)
                        })?;
                        return crate::parse_and_run_string(&code, self)
                            .map_err(FlowOrError::Error);
                    }
                }
                Ok(PerlValue::Integer(1)) // silently succeed for now
            }
            ExprKind::Exit(code) => {
                let c = if let Some(e) = code {
                    self.eval_expr(e)?.to_int() as i32
                } else {
                    0
                };
                Err(PerlError::new(ErrorKind::Exit(c), "", line, &self.file).into())
            }
            ExprKind::Chdir(expr) => {
                let path = self.eval_expr(expr)?.to_string();
                match std::env::set_current_dir(&path) {
                    Ok(_) => Ok(PerlValue::Integer(1)),
                    Err(e) => {
                        self.errno = e.to_string();
                        Ok(PerlValue::Integer(0))
                    }
                }
            }
            ExprKind::Mkdir { path, mode: _ } => {
                let p = self.eval_expr(path)?.to_string();
                match std::fs::create_dir(&p) {
                    Ok(_) => Ok(PerlValue::Integer(1)),
                    Err(e) => {
                        self.errno = e.to_string();
                        Ok(PerlValue::Integer(0))
                    }
                }
            }
            ExprKind::Unlink(args) => {
                let mut count = 0i64;
                for a in args {
                    let path = self.eval_expr(a)?.to_string();
                    if std::fs::remove_file(&path).is_ok() {
                        count += 1;
                    }
                }
                Ok(PerlValue::Integer(count))
            }
            ExprKind::Stat(e) => {
                let path = self.eval_expr(e)?.to_string();
                Ok(crate::perl_fs::stat_path(&path, false))
            }
            ExprKind::Lstat(e) => {
                let path = self.eval_expr(e)?.to_string();
                Ok(crate::perl_fs::stat_path(&path, true))
            }
            ExprKind::Link { old, new } => {
                let o = self.eval_expr(old)?.to_string();
                let n = self.eval_expr(new)?.to_string();
                Ok(crate::perl_fs::link_hard(&o, &n))
            }
            ExprKind::Symlink { old, new } => {
                let o = self.eval_expr(old)?.to_string();
                let n = self.eval_expr(new)?.to_string();
                Ok(crate::perl_fs::link_sym(&o, &n))
            }
            ExprKind::Readlink(e) => {
                let path = self.eval_expr(e)?.to_string();
                Ok(crate::perl_fs::read_link(&path))
            }
            ExprKind::Glob(args) => {
                let mut pats = Vec::new();
                for a in args {
                    pats.push(self.eval_expr(a)?.to_string());
                }
                Ok(crate::perl_fs::glob_patterns(&pats))
            }
            ExprKind::GlobPar(args) => {
                let mut pats = Vec::new();
                for a in args {
                    pats.push(self.eval_expr(a)?.to_string());
                }
                Ok(crate::perl_fs::glob_patterns(&pats))
            }
            ExprKind::Bless { ref_expr, class } => {
                let val = self.eval_expr(ref_expr)?;
                let class_name = if let Some(c) = class {
                    self.eval_expr(c)?.to_string()
                } else {
                    self.scope.get_scalar("__PACKAGE__").to_string()
                };
                Ok(PerlValue::Blessed(Arc::new(crate::value::BlessedRef {
                    class: class_name,
                    data: RwLock::new(val),
                })))
            }
            ExprKind::Caller(_) => {
                // Simplified: return package, file, line
                Ok(PerlValue::Array(vec![
                    PerlValue::String("main".into()),
                    PerlValue::String(self.file.clone()),
                    PerlValue::Integer(line as i64),
                ]))
            }
            ExprKind::Wantarray => Ok(PerlValue::Undef),

            ExprKind::List(exprs) => {
                let mut vals = Vec::new();
                for e in exprs {
                    let v = self.eval_expr(e)?;
                    match v {
                        PerlValue::Array(items) => vals.extend(items),
                        other => vals.push(other),
                    }
                }
                if vals.len() == 1 {
                    Ok(vals.pop().unwrap())
                } else {
                    Ok(PerlValue::Array(vals))
                }
            }

            // Postfix modifiers
            ExprKind::PostfixIf { expr, condition } => {
                let cond = self.eval_expr(condition)?;
                if cond.is_true() {
                    self.eval_expr(expr)
                } else {
                    Ok(PerlValue::Undef)
                }
            }
            ExprKind::PostfixUnless { expr, condition } => {
                let cond = self.eval_expr(condition)?;
                if !cond.is_true() {
                    self.eval_expr(expr)
                } else {
                    Ok(PerlValue::Undef)
                }
            }
            ExprKind::PostfixWhile { expr, condition } => {
                // `do { ... } while (COND)` — body runs before the first condition check.
                // Parsed as PostfixWhile(Do(CodeRef), cond), not plain postfix-while.
                let is_do_block = matches!(
                    &expr.kind,
                    ExprKind::Do(inner) if matches!(inner.kind, ExprKind::CodeRef { .. })
                );
                let mut last = PerlValue::Undef;
                if is_do_block {
                    loop {
                        last = self.eval_expr(expr)?;
                        let cond = self.eval_expr(condition)?;
                        if !cond.is_true() {
                            break;
                        }
                    }
                } else {
                    loop {
                        let cond = self.eval_expr(condition)?;
                        if !cond.is_true() {
                            break;
                        }
                        last = self.eval_expr(expr)?;
                    }
                }
                Ok(last)
            }
            ExprKind::PostfixUntil { expr, condition } => {
                let is_do_block = matches!(
                    &expr.kind,
                    ExprKind::Do(inner) if matches!(inner.kind, ExprKind::CodeRef { .. })
                );
                let mut last = PerlValue::Undef;
                if is_do_block {
                    loop {
                        last = self.eval_expr(expr)?;
                        let cond = self.eval_expr(condition)?;
                        if cond.is_true() {
                            break;
                        }
                    }
                } else {
                    loop {
                        let cond = self.eval_expr(condition)?;
                        if cond.is_true() {
                            break;
                        }
                        last = self.eval_expr(expr)?;
                    }
                }
                Ok(last)
            }
            ExprKind::PostfixForeach { expr, list } => {
                let items = self.eval_expr(list)?.to_list();
                let mut last = PerlValue::Undef;
                for item in items {
                    self.scope.set_scalar("_", item);
                    last = self.eval_expr(expr)?;
                }
                Ok(last)
            }
        }
    }

    // ── Helpers ──

    #[inline]
    fn eval_binop(&self, op: BinOp, lv: &PerlValue, rv: &PerlValue, _line: usize) -> ExecResult {
        Ok(match op {
            // ── Integer fast paths: avoid f64 conversion when both operands are i64 ──
            BinOp::Add => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    PerlValue::Integer(a.wrapping_add(*b))
                }
                _ => PerlValue::Float(lv.to_number() + rv.to_number()),
            },
            BinOp::Sub => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    PerlValue::Integer(a.wrapping_sub(*b))
                }
                _ => PerlValue::Float(lv.to_number() - rv.to_number()),
            },
            BinOp::Mul => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    PerlValue::Integer(a.wrapping_mul(*b))
                }
                _ => PerlValue::Float(lv.to_number() * rv.to_number()),
            },
            BinOp::Div => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    if *b == 0 {
                        return Err(PerlError::runtime("Illegal division by zero", _line).into());
                    }
                    if a % b == 0 {
                        PerlValue::Integer(a / b)
                    } else {
                        PerlValue::Float(*a as f64 / *b as f64)
                    }
                }
                _ => {
                    let d = rv.to_number();
                    if d == 0.0 {
                        return Err(PerlError::runtime("Illegal division by zero", _line).into());
                    }
                    PerlValue::Float(lv.to_number() / d)
                }
            },
            BinOp::Mod => {
                let d = rv.to_int();
                if d == 0 {
                    return Err(PerlError::runtime("Illegal modulus zero", _line).into());
                }
                PerlValue::Integer(lv.to_int() % d)
            }
            BinOp::Pow => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) if *b >= 0 && *b <= 63 => {
                    PerlValue::Integer(a.wrapping_pow(*b as u32))
                }
                _ => PerlValue::Float(lv.to_number().powf(rv.to_number())),
            },
            BinOp::Concat => {
                // Optimized: avoid allocating rv.to_string() by appending directly
                let mut s = lv.to_string();
                rv.append_to(&mut s);
                PerlValue::String(s)
            }
            BinOp::NumEq => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    PerlValue::Integer(if a == b { 1 } else { 0 })
                }
                _ => PerlValue::Integer(if lv.to_number() == rv.to_number() {
                    1
                } else {
                    0
                }),
            },
            BinOp::NumNe => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    PerlValue::Integer(if a != b { 1 } else { 0 })
                }
                _ => PerlValue::Integer(if lv.to_number() != rv.to_number() {
                    1
                } else {
                    0
                }),
            },
            BinOp::NumLt => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    PerlValue::Integer(if a < b { 1 } else { 0 })
                }
                _ => PerlValue::Integer(if lv.to_number() < rv.to_number() {
                    1
                } else {
                    0
                }),
            },
            BinOp::NumGt => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    PerlValue::Integer(if a > b { 1 } else { 0 })
                }
                _ => PerlValue::Integer(if lv.to_number() > rv.to_number() {
                    1
                } else {
                    0
                }),
            },
            BinOp::NumLe => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    PerlValue::Integer(if a <= b { 1 } else { 0 })
                }
                _ => PerlValue::Integer(if lv.to_number() <= rv.to_number() {
                    1
                } else {
                    0
                }),
            },
            BinOp::NumGe => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => {
                    PerlValue::Integer(if a >= b { 1 } else { 0 })
                }
                _ => PerlValue::Integer(if lv.to_number() >= rv.to_number() {
                    1
                } else {
                    0
                }),
            },
            BinOp::Spaceship => match (lv, rv) {
                (PerlValue::Integer(a), PerlValue::Integer(b)) => PerlValue::Integer(if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }),
                _ => {
                    let a = lv.to_number();
                    let b = rv.to_number();
                    PerlValue::Integer(if a < b {
                        -1
                    } else if a > b {
                        1
                    } else {
                        0
                    })
                }
            },
            BinOp::StrEq => PerlValue::Integer(if lv.to_string() == rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrNe => PerlValue::Integer(if lv.to_string() != rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrLt => PerlValue::Integer(if lv.to_string() < rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrGt => PerlValue::Integer(if lv.to_string() > rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrLe => PerlValue::Integer(if lv.to_string() <= rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrGe => PerlValue::Integer(if lv.to_string() >= rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrCmp => {
                let cmp = lv.to_string().cmp(&rv.to_string());
                PerlValue::Integer(match cmp {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Greater => 1,
                    std::cmp::Ordering::Equal => 0,
                })
            }
            BinOp::BitAnd => {
                if let Some(s) = crate::value::set_intersection(lv, rv) {
                    s
                } else {
                    PerlValue::Integer(lv.to_int() & rv.to_int())
                }
            }
            BinOp::BitOr => {
                if let Some(s) = crate::value::set_union(lv, rv) {
                    s
                } else {
                    PerlValue::Integer(lv.to_int() | rv.to_int())
                }
            }
            BinOp::BitXor => PerlValue::Integer(lv.to_int() ^ rv.to_int()),
            BinOp::ShiftLeft => PerlValue::Integer(lv.to_int() << rv.to_int()),
            BinOp::ShiftRight => PerlValue::Integer(lv.to_int() >> rv.to_int()),
            // These should have been handled by short-circuit above
            BinOp::LogAnd
            | BinOp::LogOr
            | BinOp::DefinedOr
            | BinOp::LogAndWord
            | BinOp::LogOrWord => unreachable!(),
            BinOp::BindMatch | BinOp::BindNotMatch => unreachable!(),
        })
    }

    fn assign_value(&mut self, target: &Expr, val: PerlValue) -> ExecResult {
        match &target.kind {
            ExprKind::ScalarVar(name) => {
                if name == "_"
                    || name == "0"
                    || name == "!"
                    || name.starts_with(|c: char| c.is_ascii_digit())
                {
                    self.set_special_var(name, &val);
                } else {
                    self.scope.set_scalar(name, val);
                }
                Ok(PerlValue::Undef)
            }
            ExprKind::ArrayVar(name) => {
                self.scope.set_array(name, val.to_list());
                Ok(PerlValue::Undef)
            }
            ExprKind::HashVar(name) => {
                let items = val.to_list();
                let mut map = IndexMap::new();
                let mut i = 0;
                while i + 1 < items.len() {
                    map.insert(items[i].to_string(), items[i + 1].clone());
                    i += 2;
                }
                self.scope.set_hash(name, map);
                Ok(PerlValue::Undef)
            }
            ExprKind::ArrayElement { array, index } => {
                let idx = self.eval_expr(index)?.to_int();
                self.scope.set_array_element(array, idx, val);
                Ok(PerlValue::Undef)
            }
            ExprKind::HashElement { hash, key } => {
                let k = self.eval_expr(key)?.to_string();
                self.scope.set_hash_element(hash, &k, val);
                Ok(PerlValue::Undef)
            }
            _ => Ok(PerlValue::Undef),
        }
    }

    fn get_special_var(&self, name: &str) -> PerlValue {
        match name {
            "_" => self.scope.get_scalar("_"),
            "0" => PerlValue::String(self.program_name.clone()),
            "!" => PerlValue::String(self.errno.clone()),
            "@" => PerlValue::String(self.eval_error.clone()),
            "/" => PerlValue::String(self.irs.clone()),
            "\\" => PerlValue::String(self.ors.clone()),
            "," => PerlValue::String(self.ofs.clone()),
            "." => PerlValue::Integer(self.line_number),
            _ => self.scope.get_scalar(name),
        }
    }

    fn set_special_var(&mut self, name: &str, val: &PerlValue) {
        match name {
            "0" => self.program_name = val.to_string(),
            "/" => self.irs = val.to_string(),
            "\\" => self.ors = val.to_string(),
            "," => self.ofs = val.to_string(),
            _ => self.scope.set_scalar(name, val.clone()),
        }
    }

    fn extract_array_name(&self, expr: &Expr) -> Result<String, FlowOrError> {
        match &expr.kind {
            ExprKind::ArrayVar(name) => Ok(name.clone()),
            ExprKind::ScalarVar(name) => Ok(name.clone()), // @_ written as shift of implicit
            _ => Err(PerlError::runtime("Expected array", expr.line).into()),
        }
    }

    fn call_named_sub(&mut self, name: &str, args: Vec<PerlValue>, line: usize) -> ExecResult {
        if let Some(sub) = self.subs.get(name).cloned() {
            return self.call_sub(&sub, args, line);
        }
        match name {
            "deque" => {
                if !args.is_empty() {
                    return Err(
                        PerlError::runtime("deque() takes no arguments", line).into(),
                    );
                }
                Ok(PerlValue::Deque(Arc::new(Mutex::new(VecDeque::new()))))
            }
            "heap" => {
                if args.len() != 1 {
                    return Err(
                        PerlError::runtime("heap() expects one comparator sub", line).into(),
                    );
                }
                match &args[0] {
                    PerlValue::CodeRef(sub) => Ok(PerlValue::Heap(Arc::new(Mutex::new(
                        PerlHeap {
                            items: Vec::new(),
                            cmp: sub.clone(),
                        },
                    )))),
                    _ => Err(PerlError::runtime("heap() requires a code reference", line).into()),
                }
            }
            _ => Err(PerlError::runtime(format!("Undefined subroutine &{}", name), line).into()),
        }
    }

    /// `deque` / `heap` method dispatch (`$q->push_back`, `$pq->pop`, …).
    pub(crate) fn try_native_method(
        &mut self,
        receiver: &PerlValue,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> Option<PerlResult<PerlValue>> {
        match receiver {
            PerlValue::Deque(d) => Some(self.deque_method(Arc::clone(d), method, args, line)),
            PerlValue::Heap(h) => Some(self.heap_method(Arc::clone(h), method, args, line)),
            _ => None,
        }
    }

    fn deque_method(
        &mut self,
        d: Arc<Mutex<VecDeque<PerlValue>>>,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match method {
            "push_back" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime("push_back expects 1 argument", line));
                }
                d.lock().push_back(args[0].clone());
                Ok(PerlValue::Integer(d.lock().len() as i64))
            }
            "push_front" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime("push_front expects 1 argument", line));
                }
                d.lock().push_front(args[0].clone());
                Ok(PerlValue::Integer(d.lock().len() as i64))
            }
            "pop_back" => Ok(d.lock().pop_back().unwrap_or(PerlValue::Undef)),
            "pop_front" => Ok(d.lock().pop_front().unwrap_or(PerlValue::Undef)),
            "size" | "len" => Ok(PerlValue::Integer(d.lock().len() as i64)),
            _ => Err(PerlError::runtime(
                format!("Unknown method for deque: {}", method),
                line,
            )),
        }
    }

    fn heap_method(
        &mut self,
        h: Arc<Mutex<PerlHeap>>,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match method {
            "push" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime("heap push expects 1 argument", line));
                }
                let mut g = h.lock();
                let n = g.items.len();
                g.items.push(args[0].clone());
                let cmp = g.cmp.clone();
                drop(g);
                let mut g = h.lock();
                self.heap_sift_up(&mut g.items, &cmp, n);
                Ok(PerlValue::Integer(g.items.len() as i64))
            }
            "pop" => {
                let mut g = h.lock();
                if g.items.is_empty() {
                    return Ok(PerlValue::Undef);
                }
                let cmp = g.cmp.clone();
                let n = g.items.len();
                g.items.swap(0, n - 1);
                let v = g.items.pop().unwrap();
                if !g.items.is_empty() {
                    self.heap_sift_down(&mut g.items, &cmp, 0);
                }
                Ok(v)
            }
            "peek" => Ok(h
                .lock()
                .items
                .first()
                .cloned()
                .unwrap_or(PerlValue::Undef)),
            _ => Err(PerlError::runtime(
                format!("Unknown method for heap: {}", method),
                line,
            )),
        }
    }

    fn heap_compare(&mut self, cmp: &Arc<PerlSub>, a: &PerlValue, b: &PerlValue) -> Ordering {
        self.scope.push_frame();
        self.scope.set_scalar("a", a.clone());
        self.scope.set_scalar("b", b.clone());
        let ord = match self.exec_block_no_scope(&cmp.body) {
            Ok(v) => {
                let n = v.to_int();
                if n < 0 {
                    Ordering::Less
                } else if n > 0 {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            }
            Err(_) => Ordering::Equal,
        };
        self.scope.pop_frame();
        ord
    }

    fn heap_sift_up(&mut self, items: &mut Vec<PerlValue>, cmp: &Arc<PerlSub>, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            if self.heap_compare(cmp, &items[i], &items[p]) != Ordering::Less {
                break;
            }
            items.swap(i, p);
            i = p;
        }
    }

    fn heap_sift_down(&mut self, items: &mut Vec<PerlValue>, cmp: &Arc<PerlSub>, mut i: usize) {
        let n = items.len();
        loop {
            let mut sm = i;
            let l = 2 * i + 1;
            let r = 2 * i + 2;
            if l < n && self.heap_compare(cmp, &items[l], &items[sm]) == Ordering::Less {
                sm = l;
            }
            if r < n && self.heap_compare(cmp, &items[r], &items[sm]) == Ordering::Less {
                sm = r;
            }
            if sm == i {
                break;
            }
            items.swap(i, sm);
            i = sm;
        }
    }

    fn call_sub(&mut self, sub: &PerlSub, args: Vec<PerlValue>, _line: usize) -> ExecResult {
        // Single frame for both @_ and the block's local variables —
        // avoids the double push_frame/pop_frame overhead per call.
        self.scope.push_frame();
        self.scope.declare_array("_", args);
        if let Some(ref env) = sub.closure_env {
            self.scope.restore_capture(env);
        }
        let result = self.exec_block_no_scope(&sub.body);
        self.scope.pop_frame();
        match result {
            Ok(v) => Ok(v),
            Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
            Err(e) => Err(e),
        }
    }

    fn builtin_new(&mut self, class: &str, args: Vec<PerlValue>, _line: usize) -> ExecResult {
        if class == "Set" {
            return Ok(crate::value::set_from_elements(args.into_iter().skip(1)));
        }
        // Default OO constructor: Class->new(%args) → bless {%args}, class
        let mut map = IndexMap::new();
        let mut i = 1; // skip $self (first arg is class name)
        while i + 1 < args.len() {
            let k = args[i].to_string();
            let v = args[i + 1].clone();
            map.insert(k, v);
            i += 2;
        }
        Ok(PerlValue::Blessed(Arc::new(crate::value::BlessedRef {
            class: class.to_string(),
            data: RwLock::new(PerlValue::Hash(map)),
        })))
    }

    fn exec_print(
        &mut self,
        handle: Option<&str>,
        args: &[Expr],
        newline: bool,
        line: usize,
    ) -> ExecResult {
        let mut output = String::new();
        for (i, a) in args.iter().enumerate() {
            if i > 0 && !self.ofs.is_empty() {
                output.push_str(&self.ofs);
            }
            let val = self.eval_expr(a)?;
            output.push_str(&val.to_string());
        }
        if newline {
            output.push('\n');
        }
        output.push_str(&self.ors);

        let handle_name = handle.unwrap_or("STDOUT");
        match handle_name {
            "STDOUT" => {
                print!("{}", output);
                let _ = io::stdout().flush();
            }
            "STDERR" => {
                eprint!("{}", output);
                let _ = io::stderr().flush();
            }
            name => {
                if let Some(writer) = self.output_handles.get_mut(name) {
                    let _ = writer.write_all(output.as_bytes());
                } else {
                    return Err(PerlError::runtime(
                        format!("print on unopened filehandle {}", name),
                        line,
                    )
                    .into());
                }
            }
        }
        Ok(PerlValue::Integer(1))
    }

    fn exec_printf(&mut self, handle: Option<&str>, args: &[Expr], _line: usize) -> ExecResult {
        if args.is_empty() {
            return Ok(PerlValue::Integer(1));
        }
        let fmt = self.eval_expr(&args[0])?.to_string();
        let mut arg_vals = Vec::new();
        for a in &args[1..] {
            arg_vals.push(self.eval_expr(a)?);
        }
        let output = perl_sprintf(&fmt, &arg_vals);
        let handle_name = handle.unwrap_or("STDOUT");
        match handle_name {
            "STDOUT" => {
                print!("{}", output);
                let _ = io::stdout().flush();
            }
            "STDERR" => {
                eprint!("{}", output);
                let _ = io::stderr().flush();
            }
            name => {
                if let Some(writer) = self.output_handles.get_mut(name) {
                    let _ = writer.write_all(output.as_bytes());
                }
            }
        }
        Ok(PerlValue::Integer(1))
    }

    pub(crate) fn compile_regex(
        &mut self,
        pattern: &str,
        flags: &str,
        line: usize,
    ) -> Result<regex::Regex, FlowOrError> {
        // Cache key: flags + separator + pattern
        let key = format!("{}\x00{}", flags, pattern);
        if let Some(cached) = self.regex_cache.get(&key) {
            return Ok(cached.clone());
        }
        let mut re_str = String::new();
        if flags.contains('i') {
            re_str.push_str("(?i)");
        }
        if flags.contains('s') {
            re_str.push_str("(?s)");
        }
        if flags.contains('m') {
            re_str.push_str("(?m)");
        }
        if flags.contains('x') {
            re_str.push_str("(?x)");
        }
        re_str.push_str(pattern);
        let re = regex::Regex::new(&re_str).map_err(|e| {
            FlowOrError::Error(PerlError::runtime(
                format!("Invalid regex /{}/: {}", pattern, e),
                line,
            ))
        })?;
        self.regex_cache.insert(key, re.clone());
        Ok(re)
    }

    /// Process a line in -n/-p mode.
    pub fn process_line(
        &mut self,
        line_str: &str,
        program: &Program,
    ) -> PerlResult<Option<String>> {
        self.line_number += 1;
        self.scope
            .set_scalar("_", PerlValue::String(line_str.to_string()));

        if self.auto_split {
            let sep = self.field_separator.as_deref().unwrap_or(" ");
            let re = regex::Regex::new(sep).unwrap_or_else(|_| regex::Regex::new(" ").unwrap());
            let fields: Vec<PerlValue> = re
                .split(line_str.trim_end_matches('\n'))
                .map(|s| PerlValue::String(s.to_string()))
                .collect();
            self.scope.set_array("F", fields);
        }

        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::SubDecl { .. } | StmtKind::Begin(_) | StmtKind::End(_) => continue,
                _ => match self.exec_statement(stmt) {
                    Ok(_) => {}
                    Err(FlowOrError::Error(e)) => return Err(e),
                    Err(FlowOrError::Flow(_)) => {}
                },
            }
        }

        // Return current $_ for -p mode
        Ok(Some(self.scope.get_scalar("_").to_string()))
    }
}

/// Minimal sprintf implementation for Perl.
pub(crate) fn perl_sprintf(fmt: &str, args: &[PerlValue]) -> String {
    let mut result = String::new();
    let mut arg_idx = 0;
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '%' {
            i += 1;
            if i >= chars.len() {
                break;
            }
            if chars[i] == '%' {
                result.push('%');
                i += 1;
                continue;
            }

            // Parse format specifier
            let mut flags = String::new();
            while i < chars.len() && "-+ #0".contains(chars[i]) {
                flags.push(chars[i]);
                i += 1;
            }
            let mut width = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                width.push(chars[i]);
                i += 1;
            }
            let mut precision = String::new();
            if i < chars.len() && chars[i] == '.' {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    precision.push(chars[i]);
                    i += 1;
                }
            }
            if i >= chars.len() {
                break;
            }
            let spec = chars[i];
            i += 1;

            let arg = args.get(arg_idx).cloned().unwrap_or(PerlValue::Undef);
            arg_idx += 1;

            let w: usize = width.parse().unwrap_or(0);
            let p: usize = precision.parse().unwrap_or(6);

            let zero_pad = flags.contains('0') && !flags.contains('-');
            let left_align = flags.contains('-');
            let formatted = match spec {
                'd' | 'i' => {
                    if zero_pad {
                        format!("{:0width$}", arg.to_int(), width = w)
                    } else if left_align {
                        format!("{:<width$}", arg.to_int(), width = w)
                    } else {
                        format!("{:width$}", arg.to_int(), width = w)
                    }
                }
                'u' => {
                    if zero_pad {
                        format!("{:0width$}", arg.to_int() as u64, width = w)
                    } else {
                        format!("{:width$}", arg.to_int() as u64, width = w)
                    }
                }
                'f' => format!("{:width$.prec$}", arg.to_number(), width = w, prec = p),
                'e' => format!("{:width$.prec$e}", arg.to_number(), width = w, prec = p),
                'g' => {
                    let n = arg.to_number();
                    if n.abs() >= 1e-4 && n.abs() < 1e15 {
                        format!("{:width$.prec$}", n, width = w, prec = p)
                    } else {
                        format!("{:width$.prec$e}", n, width = w, prec = p)
                    }
                }
                's' => {
                    let s = arg.to_string();
                    if !precision.is_empty() {
                        let truncated: String = s.chars().take(p).collect();
                        if flags.contains('-') {
                            format!("{:<width$}", truncated, width = w)
                        } else {
                            format!("{:>width$}", truncated, width = w)
                        }
                    } else if flags.contains('-') {
                        format!("{:<width$}", s, width = w)
                    } else {
                        format!("{:>width$}", s, width = w)
                    }
                }
                'x' => format!("{:width$x}", arg.to_int(), width = w),
                'X' => format!("{:width$X}", arg.to_int(), width = w),
                'o' => format!("{:width$o}", arg.to_int(), width = w),
                'b' => format!("{:width$b}", arg.to_int(), width = w),
                'c' => char::from_u32(arg.to_int() as u32)
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
                _ => arg.to_string(),
            };

            result.push_str(&formatted);
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}
