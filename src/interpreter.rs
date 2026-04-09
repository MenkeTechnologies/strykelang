use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor, Read, Write as IoWrite};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Barrier};

use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;

use caseless::default_case_fold_str;

use crate::ast::*;
use crate::builtins::PerlSocket;
use crate::crypt_util::perl_crypt;
use crate::error::{ErrorKind, PerlError, PerlResult};
use crate::profiler::Profiler;
use crate::scope::Scope;
use crate::sort_fast::{detect_sort_block_fast, sort_magic_cmp};
use crate::value::{
    CaptureResult, PerlAsyncTask, PerlBarrier, PerlHeap, PerlPpool, PerlSub, PerlValue,
    PipelineInner, PipelineOp,
};

/// `use feature 'say'`
pub const FEAT_SAY: u64 = 1 << 0;
/// `use feature 'state'`
pub const FEAT_STATE: u64 = 1 << 1;
/// `use feature 'switch'` (given/when when fully wired)
pub const FEAT_SWITCH: u64 = 1 << 2;
/// `use feature 'unicode_strings'`
pub const FEAT_UNICODE_STRINGS: u64 = 1 << 3;

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

/// Context of the **current** subroutine call (`wantarray`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WantarrayCtx {
    #[default]
    Scalar,
    List,
    Void,
}

impl WantarrayCtx {
    #[inline]
    pub(crate) fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::List,
            2 => Self::Void,
            _ => Self::Scalar,
        }
    }

    #[inline]
    pub(crate) fn as_byte(self) -> u8 {
        match self {
            Self::Scalar => 0,
            Self::List => 1,
            Self::Void => 2,
        }
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
    /// -w warnings / `use warnings`
    pub warnings: bool,
    /// `use strict` / `use strict 'refs'` / `qw(refs subs vars)` (Perl names).
    pub strict_refs: bool,
    pub strict_subs: bool,
    pub strict_vars: bool,
    /// `use utf8` — source is UTF-8 (reserved for future lexer/string semantics).
    pub utf8_pragma: bool,
    /// `use feature` — bit flags (`FEAT_*`).
    pub feature_bits: u64,
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
    /// Raw `File` per handle for `sysread` / `syswrite` / `fileno` / `flock` (parallel to buffered I/O).
    pub(crate) io_file_slots: HashMap<String, File>,
    /// Child processes for `open(H, "-|", cmd)` / `open(H, "|-", cmd)`; waited on `close`.
    pub(crate) pipe_children: HashMap<String, Child>,
    /// Sockets from `socket` / `accept` / `connect`.
    pub(crate) socket_handles: HashMap<String, PerlSocket>,
    /// `wantarray()` inside the current subroutine (`WantarrayCtx`; VM threads it on `Call`/`MethodCall`/`ArrowCall`).
    pub(crate) wantarray_kind: WantarrayCtx,
    /// `struct Name { ... }` definitions (merged from VM chunks and tree-walker).
    pub struct_defs: HashMap<String, Arc<StructDef>>,
    /// When set, tree-walker records per-statement and per-sub timings (`pe --profile`).
    pub profiler: Option<Profiler>,
}

/// Shell command for `open(H, "-|", cmd)` / `open(H, "|-", cmd)` (list form not yet supported).
fn piped_shell_command(cmd: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

/// Expands Perl `\Q...\E` spans to escaped text for the Rust [`regex`] crate.
fn expand_perl_regex_quotemeta(pat: &str) -> String {
    let mut out = String::with_capacity(pat.len().saturating_mul(2));
    let mut it = pat.chars().peekable();
    let mut in_q = false;
    while let Some(c) = it.next() {
        if in_q {
            if c == '\\' && it.peek() == Some(&'E') {
                it.next();
                in_q = false;
                continue;
            }
            out.push_str(&regex::escape(&c.to_string()));
            continue;
        }
        if c == '\\' && it.peek() == Some(&'Q') {
            it.next();
            in_q = true;
            continue;
        }
        out.push(c);
    }
    out
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

        let mut scope = Scope::new();
        scope.declare_array("INC", vec![PerlValue::String(".".to_string())]);
        scope.declare_hash("INC", IndexMap::new());
        scope.declare_array("ARGV", vec![]);
        scope.declare_array("_", vec![]);
        scope.declare_hash("ENV", env.clone());

        Self {
            scope,
            subs: HashMap::new(),
            struct_defs: HashMap::new(),
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
            strict_refs: false,
            strict_subs: false,
            strict_vars: false,
            utf8_pragma: false,
            // Like Perl 5.10+, `say` is enabled by default; `no feature 'say'` disables it.
            feature_bits: FEAT_SAY,
            num_threads: rayon::current_num_threads(),
            regex_cache: HashMap::new(),
            regex_pos: HashMap::new(),
            rand_rng: StdRng::from_entropy(),
            dir_handles: HashMap::new(),
            io_file_slots: HashMap::new(),
            pipe_children: HashMap::new(),
            socket_handles: HashMap::new(),
            wantarray_kind: WantarrayCtx::Scalar,
            profiler: None,
        }
    }

    /// Paths from `@INC` for `require` / `use` (non-empty; defaults to `.` if unset).
    pub(crate) fn inc_directories(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .scope
            .get_array("INC")
            .into_iter()
            .map(|x| x.to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if v.is_empty() {
            v.push(".".to_string());
        }
        v
    }

    #[inline]
    fn strict_scalar_exempt(name: &str) -> bool {
        matches!(
            name,
            "_" | "0" | "!" | "@" | "/" | "\\" | "," | "." | "__PACKAGE__" | "$$"
        )
            || name.chars().all(|c| c.is_ascii_digit())
    }

    fn check_strict_scalar_var(&self, name: &str, line: usize) -> Result<(), FlowOrError> {
        if !self.strict_vars
            || Self::strict_scalar_exempt(name)
            || name.contains("::")
            || self.scope.scalar_binding_exists(name)
        {
            return Ok(());
        }
        Err(PerlError::runtime(
            format!(
                "Global symbol \"${}\" requires explicit package name (did you forget to declare \"my ${}\"?)",
                name, name
            ),
            line,
        )
        .into())
    }

    fn check_strict_array_var(&self, name: &str, line: usize) -> Result<(), FlowOrError> {
        if !self.strict_vars || name.contains("::") || self.scope.array_binding_exists(name) {
            return Ok(());
        }
        Err(PerlError::runtime(
            format!(
                "Global symbol \"@{}\" requires explicit package name (did you forget to declare \"my @{}\"?)",
                name, name
            ),
            line,
        )
        .into())
    }

    fn check_strict_hash_var(&self, name: &str, line: usize) -> Result<(), FlowOrError> {
        if !self.strict_vars || name.contains("::") || self.scope.hash_binding_exists(name) {
            return Ok(());
        }
        Err(PerlError::runtime(
            format!(
                "Global symbol \"%{}\" requires explicit package name (did you forget to declare \"my %{}\"?)",
                name, name
            ),
            line,
        )
        .into())
    }

    fn looks_like_version_only(spec: &str) -> bool {
        let t = spec.trim();
        !t.is_empty()
            && !t.contains('/')
            && !t.contains('\\')
            && !t.contains("::")
            && t.chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '_' || c == 'v')
            && t.chars().any(|c| c.is_ascii_digit())
    }

    fn module_spec_to_relpath(spec: &str) -> String {
        let t = spec.trim();
        if t.contains("::") {
            format!("{}.pm", t.replace("::", "/"))
        } else if t.ends_with(".pm") || t.ends_with(".pl") {
            t.replace('\\', "/")
        } else if t.contains('/') {
            t.replace('\\', "/")
        } else {
            format!("{}.pm", t)
        }
    }

    /// `sub name` in `package P` → stash key `P::name` (otherwise `name` in `main`).
    pub(crate) fn qualify_sub_key(&self, name: &str) -> String {
        let pkg = self.current_package();
        if pkg.is_empty() || pkg == "main" {
            name.to_string()
        } else {
            format!("{}::{}", pkg, name)
        }
    }

    /// Where `use` imports a symbol: `main` → short name; otherwise `Pkg::sym`.
    fn import_alias_key(&self, short: &str) -> String {
        self.qualify_sub_key(short)
    }

    /// `use Module qw()` — explicit empty list (not the same as `use Module`).
    fn is_explicit_empty_import_list(imports: &[Expr]) -> bool {
        if imports.len() == 1 {
            if let ExprKind::QW(ws) = &imports[0].kind {
                return ws.is_empty();
            }
        }
        false
    }

    /// After `require`, copy `Module::export` → caller stash per `use` list.
    fn apply_module_import(
        &mut self,
        module: &str,
        imports: &[Expr],
        line: usize,
    ) -> PerlResult<()> {
        if imports.is_empty() {
            return self.import_all_from_module(module, line);
        }
        if Self::is_explicit_empty_import_list(imports) {
            return Ok(());
        }
        let names = Self::pragma_import_strings(imports, line)?;
        if names.is_empty() {
            return Ok(());
        }
        for name in names {
            self.import_one_symbol(module, &name, line)?;
        }
        Ok(())
    }

    fn import_all_from_module(&mut self, module: &str, _line: usize) -> PerlResult<()> {
        let prefix = format!("{}::", module);
        let keys: Vec<String> = self
            .subs
            .keys()
            .filter(|k| k.starts_with(&prefix) && !k[prefix.len()..].contains("::"))
            .cloned()
            .collect();
        for k in keys {
            let short = k[prefix.len()..].to_string();
            if let Some(sub) = self.subs.get(&k).cloned() {
                let alias = self.import_alias_key(&short);
                self.subs.insert(alias, sub);
            }
        }
        Ok(())
    }

    fn import_one_symbol(&mut self, module: &str, export: &str, line: usize) -> PerlResult<()> {
        let qual = format!("{}::{}", module, export);
        let sub = self.subs.get(&qual).cloned().ok_or_else(|| {
            PerlError::runtime(
                format!(
                    "`{}` is not defined in module `{}` (expected `{}`)",
                    export, module, qual
                ),
                line,
            )
        })?;
        let alias = self.import_alias_key(export);
        self.subs.insert(alias, sub);
        Ok(())
    }

    /// Resolve `foo` or `Foo::bar` against the subroutine stash (package-aware).
    pub(crate) fn resolve_sub_by_name(&self, name: &str) -> Option<Arc<PerlSub>> {
        if let Some(s) = self.subs.get(name) {
            return Some(s.clone());
        }
        if !name.contains("::") {
            let pkg = self.current_package();
            if !pkg.is_empty() && pkg != "main" {
                let q = format!("{}::{}", pkg, name);
                return self.subs.get(&q).cloned();
            }
        }
        None
    }

    /// Compile-time pragma import list (`'refs'`, `qw(refs subs)`, version integers).
    fn pragma_import_strings(imports: &[Expr], default_line: usize) -> PerlResult<Vec<String>> {
        let mut out = Vec::new();
        for e in imports {
            match &e.kind {
                ExprKind::String(s) => out.push(s.clone()),
                ExprKind::QW(ws) => out.extend(ws.iter().cloned()),
                ExprKind::Integer(n) => out.push(n.to_string()),
                _ => {
                    return Err(PerlError::runtime(
                        "pragma import must be a compile-time string, qw(), or integer",
                        e.line.max(default_line),
                    ));
                }
            }
        }
        Ok(out)
    }

    fn apply_use_strict(&mut self, imports: &[Expr], line: usize) -> PerlResult<()> {
        if imports.is_empty() {
            self.strict_refs = true;
            self.strict_subs = true;
            self.strict_vars = true;
            return Ok(());
        }
        let names = Self::pragma_import_strings(imports, line)?;
        for name in names {
            match name.as_str() {
                "refs" => self.strict_refs = true,
                "subs" => self.strict_subs = true,
                "vars" => self.strict_vars = true,
                _ => {
                    return Err(PerlError::runtime(
                        format!("Unknown strict mode `{}`", name),
                        line,
                    ));
                }
            }
        }
        Ok(())
    }

    fn apply_no_strict(&mut self, imports: &[Expr], line: usize) -> PerlResult<()> {
        if imports.is_empty() {
            self.strict_refs = false;
            self.strict_subs = false;
            self.strict_vars = false;
            return Ok(());
        }
        let names = Self::pragma_import_strings(imports, line)?;
        for name in names {
            match name.as_str() {
                "refs" => self.strict_refs = false,
                "subs" => self.strict_subs = false,
                "vars" => self.strict_vars = false,
                _ => {
                    return Err(PerlError::runtime(
                        format!("Unknown strict mode `{}`", name),
                        line,
                    ));
                }
            }
        }
        Ok(())
    }

    fn apply_use_feature(&mut self, imports: &[Expr], line: usize) -> PerlResult<()> {
        let items = Self::pragma_import_strings(imports, line)?;
        if items.is_empty() {
            return Err(PerlError::runtime(
                "use feature requires a feature name or bundle (e.g. qw(say) or :5.10)",
                line,
            ));
        }
        for item in items {
            let s = item.trim();
            if let Some(rest) = s.strip_prefix(':') {
                self.apply_feature_bundle(rest, line)?;
            } else {
                self.apply_feature_name(s, true, line)?;
            }
        }
        Ok(())
    }

    fn apply_no_feature(&mut self, imports: &[Expr], line: usize) -> PerlResult<()> {
        if imports.is_empty() {
            self.feature_bits = 0;
            return Ok(());
        }
        let items = Self::pragma_import_strings(imports, line)?;
        for item in items {
            let s = item.trim();
            if let Some(rest) = s.strip_prefix(':') {
                self.clear_feature_bundle(rest);
            } else {
                self.apply_feature_name(s, false, line)?;
            }
        }
        Ok(())
    }

    fn apply_feature_bundle(&mut self, v: &str, line: usize) -> PerlResult<()> {
        let key = v.trim();
        match key {
            "5.10" | "5.010" | "5.10.0" => {
                self.feature_bits |= FEAT_SAY | FEAT_SWITCH | FEAT_STATE | FEAT_UNICODE_STRINGS;
            }
            "5.12" | "5.012" | "5.12.0" => {
                self.feature_bits |= FEAT_SAY | FEAT_SWITCH | FEAT_STATE | FEAT_UNICODE_STRINGS;
            }
            _ => {
                return Err(PerlError::runtime(
                    format!("unsupported feature bundle :{}", key),
                    line,
                ));
            }
        }
        Ok(())
    }

    fn clear_feature_bundle(&mut self, v: &str) {
        let key = v.trim();
        if matches!(
            key,
            "5.10" | "5.010" | "5.10.0" | "5.12" | "5.012" | "5.12.0"
        ) {
            self.feature_bits &= !(FEAT_SAY | FEAT_SWITCH | FEAT_STATE | FEAT_UNICODE_STRINGS);
        }
    }

    fn apply_feature_name(&mut self, name: &str, enable: bool, line: usize) -> PerlResult<()> {
        let bit = match name {
            "say" => FEAT_SAY,
            "state" => FEAT_STATE,
            "switch" => FEAT_SWITCH,
            "unicode_strings" => FEAT_UNICODE_STRINGS,
            _ => {
                return Err(PerlError::runtime(
                    format!("unknown feature `{}`", name),
                    line,
                ));
            }
        };
        if enable {
            self.feature_bits |= bit;
        } else {
            self.feature_bits &= !bit;
        }
        Ok(())
    }

    /// `require EXPR` — load once, record `%INC`, return `1` on success.
    pub(crate) fn require_execute(&mut self, spec: &str, line: usize) -> PerlResult<PerlValue> {
        let t = spec.trim();
        if t.is_empty() {
            return Err(PerlError::runtime("require: empty argument", line));
        }
        match t {
            "strict" => {
                self.apply_use_strict(&[], line)?;
                return Ok(PerlValue::Integer(1));
            }
            "utf8" => {
                self.utf8_pragma = true;
                return Ok(PerlValue::Integer(1));
            }
            "feature" | "v5" => {
                return Ok(PerlValue::Integer(1));
            }
            "warnings" => {
                self.warnings = true;
                return Ok(PerlValue::Integer(1));
            }
            "threads" | "Thread::Pool" | "Parallel::ForkManager" => {
                return Ok(PerlValue::Integer(1));
            }
            _ => {}
        }
        let p = Path::new(t);
        if p.is_absolute() {
            return self.require_absolute_path(p, line);
        }
        if Self::looks_like_version_only(t) {
            return Ok(PerlValue::Integer(1));
        }
        let relpath = Self::module_spec_to_relpath(t);
        self.require_from_inc(&relpath, line)
    }

    fn require_absolute_path(&mut self, path: &Path, line: usize) -> PerlResult<PerlValue> {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let key = canon.to_string_lossy().into_owned();
        if self.scope.exists_hash_element("INC", &key) {
            return Ok(PerlValue::Integer(1));
        }
        let code = std::fs::read_to_string(&canon).map_err(|e| {
            PerlError::runtime(
                format!("Can't open {} for reading: {}", canon.display(), e),
                line,
            )
        })?;
        self.scope
            .set_hash_element("INC", &key, PerlValue::String(key.clone()));
        let saved_pkg = self.scope.get_scalar("__PACKAGE__");
        let r = crate::parse_and_run_string(&code, self);
        let _ = self.scope.set_scalar("__PACKAGE__", saved_pkg);
        r?;
        Ok(PerlValue::Integer(1))
    }

    fn require_from_inc(&mut self, relpath: &str, line: usize) -> PerlResult<PerlValue> {
        if self.scope.exists_hash_element("INC", relpath) {
            return Ok(PerlValue::Integer(1));
        }
        for dir in self.inc_directories() {
            let full = Path::new(&dir).join(relpath);
            if full.is_file() {
                let code = std::fs::read_to_string(&full).map_err(|e| {
                    PerlError::runtime(
                        format!("Can't open {} for reading: {}", full.display(), e),
                        line,
                    )
                })?;
                let abs = full.canonicalize().unwrap_or(full);
                let abs_s = abs.to_string_lossy().into_owned();
                self.scope
                    .set_hash_element("INC", relpath, PerlValue::String(abs_s));
                let saved_pkg = self.scope.get_scalar("__PACKAGE__");
                let r = crate::parse_and_run_string(&code, self);
                let _ = self.scope.set_scalar("__PACKAGE__", saved_pkg);
                r?;
                return Ok(PerlValue::Integer(1));
            }
        }
        Err(PerlError::runtime(
            format!(
                "Can't locate {} in @INC (push paths onto @INC or use -I DIR)",
                relpath
            ),
            line,
        ))
    }

    /// Pragmas (`use strict 'refs'`, `use feature`) or load a `.pm` file (`use Foo::Bar`).
    pub(crate) fn exec_use_stmt(
        &mut self,
        module: &str,
        imports: &[Expr],
        line: usize,
    ) -> PerlResult<()> {
        match module {
            "strict" => self.apply_use_strict(imports, line),
            "utf8" => {
                if !imports.is_empty() {
                    return Err(PerlError::runtime("use utf8 takes no arguments", line));
                }
                self.utf8_pragma = true;
                Ok(())
            }
            "feature" => self.apply_use_feature(imports, line),
            "v5" => Ok(()),
            "warnings" => {
                self.warnings = true;
                Ok(())
            }
            "threads" | "Thread::Pool" | "Parallel::ForkManager" => Ok(()),
            _ => {
                self.require_execute(module, line)?;
                self.apply_module_import(module, imports, line)?;
                Ok(())
            }
        }
    }

    /// `no strict 'refs'`, `no warnings`, `no feature`, …
    pub(crate) fn exec_no_stmt(
        &mut self,
        module: &str,
        imports: &[Expr],
        line: usize,
    ) -> PerlResult<()> {
        match module {
            "strict" => self.apply_no_strict(imports, line),
            "utf8" => {
                if !imports.is_empty() {
                    return Err(PerlError::runtime("no utf8 takes no arguments", line));
                }
                self.utf8_pragma = false;
                Ok(())
            }
            "feature" => self.apply_no_feature(imports, line),
            "v5" => Ok(()),
            "warnings" => {
                self.warnings = false;
                Ok(())
            }
            "threads" | "Thread::Pool" | "Parallel::ForkManager" => Ok(()),
            _ => Ok(()),
        }
    }

    /// Register subs, run `use` in source order, collect `BEGIN`/`END` (before `BEGIN` execution).
    pub(crate) fn prepare_program_top_level(&mut self, program: &Program) -> PerlResult<()> {
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::Package { name } => {
                    let _ = self
                        .scope
                        .set_scalar("__PACKAGE__", PerlValue::String(name.clone()));
                }
                StmtKind::SubDecl {
                    name,
                    params,
                    body,
                    prototype,
                } => {
                    let key = self.qualify_sub_key(name);
                    self.subs.insert(
                        key,
                        Arc::new(PerlSub {
                            name: name.clone(),
                            params: params.clone(),
                            body: body.clone(),
                            closure_env: None,
                            prototype: prototype.clone(),
                        }),
                    );
                }
                StmtKind::Use { module, imports } => {
                    self.exec_use_stmt(module, imports, stmt.line)?;
                }
                StmtKind::No { module, imports } => {
                    self.exec_no_stmt(module, imports, stmt.line)?;
                }
                StmtKind::Begin(block) => self.begin_blocks.push(block.clone()),
                StmtKind::End(block) => self.end_blocks.push(block.clone()),
                _ => {}
            }
        }
        Ok(())
    }

    /// Install the `DATA` handle from a script `__DATA__` section (bytes after the marker line).
    pub fn install_data_handle(&mut self, data: Vec<u8>) {
        self.input_handles.insert(
            "DATA".to_string(),
            BufReader::new(Box::new(Cursor::new(data)) as Box<dyn Read + Send>),
        );
    }

    /// `open` and VM `BuiltinId::Open`. `file_opt` is the evaluated third argument when present.
    pub(crate) fn open_builtin_execute(
        &mut self,
        handle_name: String,
        mode_s: String,
        file_opt: Option<String>,
        line: usize,
    ) -> PerlResult<PerlValue> {
        let (actual_mode, path) = if let Some(f) = file_opt {
            (mode_s, f)
        } else if let Some(rest) = mode_s.strip_prefix(">>") {
            (">>".to_string(), rest.trim().to_string())
        } else if let Some(rest) = mode_s.strip_prefix('>') {
            (">".to_string(), rest.trim().to_string())
        } else if let Some(rest) = mode_s.strip_prefix('<') {
            ("<".to_string(), rest.trim().to_string())
        } else {
            ("<".to_string(), mode_s)
        };
        match actual_mode.as_str() {
            "-|" => {
                let mut cmd = piped_shell_command(&path);
                cmd.stdout(Stdio::piped());
                let mut child = cmd.spawn().map_err(|e| {
                    self.errno = e.to_string();
                    PerlError::runtime(format!("Can't open pipe from command: {}", e), line)
                })?;
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| PerlError::runtime("pipe: child has no stdout", line))?;
                self.input_handles
                    .insert(handle_name.clone(), BufReader::new(Box::new(stdout)));
                self.pipe_children.insert(handle_name, child);
            }
            "|-" => {
                let mut cmd = piped_shell_command(&path);
                cmd.stdin(Stdio::piped());
                let mut child = cmd.spawn().map_err(|e| {
                    self.errno = e.to_string();
                    PerlError::runtime(format!("Can't open pipe to command: {}", e), line)
                })?;
                let stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| PerlError::runtime("pipe: child has no stdin", line))?;
                self.output_handles
                    .insert(handle_name.clone(), Box::new(stdin));
                self.pipe_children.insert(handle_name, child);
            }
            "<" => {
                let file = std::fs::File::open(&path).map_err(|e| {
                    self.errno = e.to_string();
                    PerlError::runtime(format!("Can't open '{}': {}", path, e), line)
                })?;
                if let Ok(raw) = file.try_clone() {
                    self.io_file_slots.insert(handle_name.clone(), raw);
                }
                self.input_handles
                    .insert(handle_name, BufReader::new(Box::new(file)));
            }
            ">" => {
                let file = std::fs::File::create(&path).map_err(|e| {
                    self.errno = e.to_string();
                    PerlError::runtime(format!("Can't open '{}': {}", path, e), line)
                })?;
                if let Ok(raw) = file.try_clone() {
                    self.io_file_slots.insert(handle_name.clone(), raw);
                }
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
                if let Ok(raw) = file.try_clone() {
                    self.io_file_slots.insert(handle_name.clone(), raw);
                }
                self.output_handles.insert(handle_name, Box::new(file));
            }
            _ => {
                return Err(PerlError::runtime(
                    format!("Unknown open mode '{}'", actual_mode),
                    line,
                ));
            }
        }
        Ok(PerlValue::Integer(1))
    }

    pub(crate) fn close_builtin_execute(&mut self, name: String) -> PerlResult<PerlValue> {
        self.output_handles.remove(&name);
        self.input_handles.remove(&name);
        self.io_file_slots.remove(&name);
        if let Some(mut child) = self.pipe_children.remove(&name) {
            let _ = child.wait();
        }
        Ok(PerlValue::Integer(1))
    }

    pub(crate) fn has_input_handle(&self, name: &str) -> bool {
        self.input_handles.contains_key(name)
    }

    pub(crate) fn readline_builtin_execute(
        &mut self,
        handle: Option<&str>,
    ) -> PerlResult<PerlValue> {
        let handle_name = handle.unwrap_or("STDIN");
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

    pub(crate) fn opendir_handle(&mut self, handle: &str, path: &str) -> PerlValue {
        match std::fs::read_dir(path) {
            Ok(rd) => {
                let entries: Vec<String> = rd
                    .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
                    .collect();
                self.dir_handles
                    .insert(handle.to_string(), DirHandleState { entries, pos: 0 });
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

    /// Set `$1`…`$n` and `%+` from a successful match (`regex` crate named groups `(?<name>…)`).
    pub(crate) fn apply_regex_captures(
        &mut self,
        re: &regex::Regex,
        caps: &regex::Captures<'_>,
    ) -> Result<(), FlowOrError> {
        for i in 1..caps.len() {
            if let Some(m) = caps.get(i) {
                self.scope
                    .set_scalar(&i.to_string(), PerlValue::String(m.as_str().to_string()))?;
            }
        }
        let mut named = IndexMap::new();
        for name in re.capture_names().flatten() {
            if let Some(m) = caps.name(name) {
                named.insert(name.to_string(), PerlValue::String(m.as_str().to_string()));
            }
        }
        self.scope.set_hash("+", named);
        Ok(())
    }

    /// Shared `chomp` for tree-walker and VM (mutates `target`).
    pub(crate) fn chomp_inplace_execute(&mut self, val: PerlValue, target: &Expr) -> ExecResult {
        let mut s = val.to_string();
        let removed = if s.ends_with('\n') {
            s.pop();
            1i64
        } else {
            0i64
        };
        self.assign_value(target, PerlValue::String(s))?;
        Ok(PerlValue::Integer(removed))
    }

    /// Shared `chop` for tree-walker and VM (mutates `target`).
    pub(crate) fn chop_inplace_execute(&mut self, val: PerlValue, target: &Expr) -> ExecResult {
        let mut s = val.to_string();
        let chopped = s
            .pop()
            .map(|c| PerlValue::String(c.to_string()))
            .unwrap_or(PerlValue::Undef);
        self.assign_value(target, PerlValue::String(s))?;
        Ok(chopped)
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
                self.apply_regex_captures(&re, &caps)?;
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
            self.apply_regex_captures(&re, &caps)?;
            Ok(PerlValue::Integer(1))
        } else {
            Ok(PerlValue::Integer(0))
        }
    }

    /// Shared `s///` for tree-walker and VM.
    pub(crate) fn regex_subst_execute(
        &mut self,
        s: String,
        pattern: &str,
        replacement: &str,
        flags: &str,
        target: &Expr,
        line: usize,
    ) -> ExecResult {
        let re = self.compile_regex(pattern, flags, line)?;
        let last_caps = if flags.contains('g') {
            re.captures_iter(&s).last()
        } else {
            re.captures(&s)
        };
        if let Some(caps) = last_caps {
            self.apply_regex_captures(&re, &caps)?;
        }
        let (new_s, count) = if flags.contains('g') {
            let count = re.find_iter(&s).count();
            (re.replace_all(&s, replacement).to_string(), count)
        } else {
            let count = if re.is_match(&s) { 1 } else { 0 };
            (re.replace(&s, replacement).to_string(), count)
        };
        self.assign_value(target, PerlValue::String(new_s))?;
        Ok(PerlValue::Integer(count as i64))
    }

    /// Shared `tr///` for tree-walker and VM.
    pub(crate) fn regex_transliterate_execute(
        &mut self,
        s: String,
        from: &str,
        to: &str,
        flags: &str,
        target: &Expr,
        line: usize,
    ) -> ExecResult {
        let _ = line;
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
            self.assign_value(target, PerlValue::String(new_s))?;
        }
        Ok(PerlValue::Integer(count))
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

    /// Keywords, builtins, lexical names, and subroutine names for REPL tab-completion.
    pub fn repl_completion_names(&self) -> Vec<String> {
        let mut v = self.scope.repl_binding_names();
        v.extend(self.subs.keys().cloned());
        v.sort();
        v.dedup();
        v
    }

    pub fn execute(&mut self, program: &Program) -> PerlResult<PerlValue> {
        // Profiling uses the tree-walker only (see `try_vm_execute`).
        // Try bytecode VM first — falls back to tree-walker on unsupported features
        if let Some(result) = crate::try_vm_execute(program, self) {
            return result;
        }

        // Tree-walker fallback
        self.execute_tree(program)
    }

    /// Tree-walking execution (fallback when bytecode compilation fails).
    pub fn execute_tree(&mut self, program: &Program) -> PerlResult<PerlValue> {
        // First pass: subs, `use` (source order), BEGIN/END collection
        self.prepare_program_top_level(program)?;

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
                StmtKind::SubDecl { .. }
                | StmtKind::Begin(_)
                | StmtKind::End(_)
                | StmtKind::Use { .. }
                | StmtKind::No { .. } => continue,
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
        let uses_goto = block
            .iter()
            .any(|s| matches!(s.kind, StmtKind::Goto { .. }));
        if uses_goto {
            self.scope.push_frame();
            let r = self.exec_block_with_goto(block);
            self.scope.pop_frame();
            r
        } else {
            self.scope.push_frame();
            let result = self.exec_block_no_scope(block);
            self.scope.pop_frame();
            result
        }
    }

    fn exec_block_with_goto(&mut self, block: &Block) -> ExecResult {
        let mut map: HashMap<String, usize> = HashMap::new();
        for (i, s) in block.iter().enumerate() {
            if let Some(l) = &s.label {
                map.insert(l.clone(), i);
            }
        }
        let mut pc = 0usize;
        let mut last = PerlValue::Undef;
        while pc < block.len() {
            if let StmtKind::Goto { target } = &block[pc].kind {
                let line = block[pc].line;
                let name = self.eval_expr(target)?.to_string();
                pc = *map.get(&name).ok_or_else(|| {
                    FlowOrError::Error(PerlError::runtime(
                        format!("goto: unknown label {}", name),
                        line,
                    ))
                })?;
                continue;
            }
            match self.exec_statement(&block[pc]) {
                Ok(v) => last = v,
                Err(e) => return Err(e),
            }
            pc += 1;
        }
        Ok(last)
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

    /// `eval_timeout SECS { ... }` — run block on another thread; this thread waits (no Unix signals).
    fn eval_timeout_block(&mut self, body: &Block, secs: f64, line: usize) -> ExecResult {
        use std::sync::mpsc::channel;
        use std::time::Duration;

        let block = body.clone();
        let subs = self.subs.clone();
        let struct_defs = self.struct_defs.clone();
        let (scalars, aar, ahash) = self.scope.capture_with_atomics();
        let env = self.env.clone();
        let argv = self.argv.clone();
        let inc = self.scope.get_array("INC");
        let (tx, rx) = channel::<PerlResult<PerlValue>>();
        let _handle = std::thread::spawn(move || {
            let mut interp = Interpreter::new();
            interp.subs = subs;
            interp.struct_defs = struct_defs;
            interp.env = env.clone();
            interp.argv = argv.clone();
            interp.scope.declare_array(
                "ARGV",
                argv.iter().map(|s| PerlValue::String(s.clone())).collect(),
            );
            for (k, v) in env {
                let _ = interp.scope.set_hash_element("ENV", &k, v);
            }
            interp.scope.declare_array("INC", inc);
            interp.scope.restore_capture(&scalars);
            interp.scope.restore_atomics(&aar, &ahash);
            let out: PerlResult<PerlValue> = match interp.exec_block(&block) {
                Ok(v) => Ok(v),
                Err(FlowOrError::Error(e)) => Err(e),
                Err(FlowOrError::Flow(_)) => Ok(PerlValue::Undef),
            };
            let _ = tx.send(out);
        });
        let dur = Duration::from_secs_f64(secs.max(0.0));
        match rx.recv_timeout(dur) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(FlowOrError::Error(e)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(PerlError::runtime(
                format!(
                    "eval_timeout: exceeded {} second(s) (worker continues in background)",
                    secs
                ),
                line,
            )
            .into()),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(PerlError::runtime(
                "eval_timeout: worker thread panicked or disconnected",
                line,
            )
            .into()),
        }
    }

    fn exec_given_body(&mut self, body: &Block) -> ExecResult {
        let mut last = PerlValue::Undef;
        for stmt in body {
            match &stmt.kind {
                StmtKind::When { cond, body: wb } => {
                    if self.when_matches(cond)? {
                        return self.exec_block_smart(wb);
                    }
                }
                StmtKind::DefaultCase { body: db } => {
                    return self.exec_block_smart(db);
                }
                _ => {
                    last = self.exec_statement(stmt)?;
                }
            }
        }
        Ok(last)
    }

    fn exec_given(&mut self, topic: &Expr, body: &Block) -> ExecResult {
        let t = self.eval_expr(topic)?;
        self.scope.push_frame();
        self.scope.declare_scalar("_", t);
        let r = self.exec_given_body(body);
        self.scope.pop_frame();
        r
    }

    /// `when (COND)` — topic is `$_` (set by `given`).
    fn when_matches(&mut self, cond: &Expr) -> Result<bool, FlowOrError> {
        let topic = self.scope.get_scalar("_");
        let line = cond.line;
        match &cond.kind {
            ExprKind::Regex(pattern, flags) => {
                let re = self.compile_regex(pattern, flags, line)?;
                let s = topic.to_string();
                Ok(re.is_match(&s))
            }
            ExprKind::String(s) => Ok(topic.to_string() == *s),
            ExprKind::Integer(n) => Ok(topic.to_int() == *n),
            ExprKind::Float(f) => Ok((topic.to_number() - *f).abs() < 1e-9),
            _ => {
                let c = self.eval_expr(cond)?;
                Ok(self.smartmatch_when(&topic, &c))
            }
        }
    }

    fn smartmatch_when(&self, topic: &PerlValue, c: &PerlValue) -> bool {
        match c {
            PerlValue::Regex(re, _) => re.is_match(&topic.to_string()),
            _ => topic.to_string() == c.to_string(),
        }
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
        let t0 = self.profiler.is_some().then(std::time::Instant::now);
        let r = self.exec_statement_inner(stmt);
        if let (Some(prof), Some(t0)) = (&mut self.profiler, t0) {
            prof.on_line(&self.file, stmt.line, t0.elapsed());
        }
        r
    }

    fn exec_statement_inner(&mut self, stmt: &Statement) -> ExecResult {
        match &stmt.kind {
            StmtKind::Expression(expr) => self.eval_expr_ctx(expr, WantarrayCtx::Void),
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
                continue_block,
            } => {
                'outer: loop {
                    let cond = self.eval_expr(condition)?;
                    if !cond.is_true() {
                        break;
                    }
                    'inner: loop {
                        match self.exec_block_smart(body) {
                            Ok(_) => break 'inner,
                            Err(FlowOrError::Flow(Flow::Last(ref l)))
                                if l == label || l.is_none() =>
                            {
                                break 'outer;
                            }
                            Err(FlowOrError::Flow(Flow::Next(ref l)))
                                if l == label || l.is_none() =>
                            {
                                if let Some(cb) = continue_block {
                                    let _ = self.exec_block_smart(cb);
                                }
                                continue 'outer;
                            }
                            Err(FlowOrError::Flow(Flow::Redo(ref l)))
                                if l == label || l.is_none() =>
                            {
                                continue 'inner;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    if let Some(cb) = continue_block {
                        let _ = self.exec_block_smart(cb);
                    }
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::Until {
                condition,
                body,
                label,
                continue_block,
            } => {
                'outer: loop {
                    let cond = self.eval_expr(condition)?;
                    if cond.is_true() {
                        break;
                    }
                    'inner: loop {
                        match self.exec_block(body) {
                            Ok(_) => break 'inner,
                            Err(FlowOrError::Flow(Flow::Last(ref l)))
                                if l == label || l.is_none() =>
                            {
                                break 'outer;
                            }
                            Err(FlowOrError::Flow(Flow::Next(ref l)))
                                if l == label || l.is_none() =>
                            {
                                if let Some(cb) = continue_block {
                                    let _ = self.exec_block_smart(cb);
                                }
                                continue 'outer;
                            }
                            Err(FlowOrError::Flow(Flow::Redo(ref l)))
                                if l == label || l.is_none() =>
                            {
                                continue 'inner;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    if let Some(cb) = continue_block {
                        let _ = self.exec_block_smart(cb);
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
                continue_block,
            } => {
                self.scope.push_frame();
                if let Some(init) = init {
                    self.exec_statement(init)?;
                }
                'outer: loop {
                    if let Some(cond) = condition {
                        let cv = self.eval_expr(cond)?;
                        if !cv.is_true() {
                            break;
                        }
                    }
                    'inner: loop {
                        match self.exec_block_smart(body) {
                            Ok(_) => break 'inner,
                            Err(FlowOrError::Flow(Flow::Last(ref l)))
                                if l == label || l.is_none() =>
                            {
                                break 'outer;
                            }
                            Err(FlowOrError::Flow(Flow::Next(ref l)))
                                if l == label || l.is_none() =>
                            {
                                if let Some(cb) = continue_block {
                                    let _ = self.exec_block_smart(cb);
                                }
                                if let Some(step) = step {
                                    self.eval_expr(step)?;
                                }
                                continue 'outer;
                            }
                            Err(FlowOrError::Flow(Flow::Redo(ref l)))
                                if l == label || l.is_none() =>
                            {
                                continue 'inner;
                            }
                            Err(e) => {
                                self.scope.pop_frame();
                                return Err(e);
                            }
                        }
                    }
                    if let Some(cb) = continue_block {
                        let _ = self.exec_block_smart(cb);
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
                continue_block,
            } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                self.scope.push_frame();
                self.scope.declare_scalar(var, PerlValue::Undef);
                let mut i = 0usize;
                'outer: while i < items.len() {
                    self.scope
                        .set_scalar(var, items[i].clone())
                        .map_err(|e| FlowOrError::Error(e.at_line(stmt.line)))?;
                    'inner: loop {
                        match self.exec_block_smart(body) {
                            Ok(_) => break 'inner,
                            Err(FlowOrError::Flow(Flow::Last(ref l)))
                                if l == label || l.is_none() =>
                            {
                                break 'outer;
                            }
                            Err(FlowOrError::Flow(Flow::Next(ref l)))
                                if l == label || l.is_none() =>
                            {
                                if let Some(cb) = continue_block {
                                    let _ = self.exec_block_smart(cb);
                                }
                                i += 1;
                                continue 'outer;
                            }
                            Err(FlowOrError::Flow(Flow::Redo(ref l)))
                                if l == label || l.is_none() =>
                            {
                                continue 'inner;
                            }
                            Err(e) => {
                                self.scope.pop_frame();
                                return Err(e);
                            }
                        }
                    }
                    if let Some(cb) = continue_block {
                        let _ = self.exec_block_smart(cb);
                    }
                    i += 1;
                }
                self.scope.pop_frame();
                Ok(PerlValue::Undef)
            }
            StmtKind::SubDecl {
                name,
                params,
                body,
                prototype,
            } => {
                let key = self.qualify_sub_key(name);
                self.subs.insert(
                    key,
                    Arc::new(PerlSub {
                        name: name.clone(),
                        params: params.clone(),
                        body: body.clone(),
                        closure_env: None,
                        prototype: prototype.clone(),
                    }),
                );
                Ok(PerlValue::Undef)
            }
            StmtKind::StructDecl { def } => {
                if self.struct_defs.contains_key(&def.name) {
                    return Err(PerlError::runtime(
                        format!("duplicate struct `{}`", def.name),
                        stmt.line,
                    )
                    .into());
                }
                self.struct_defs
                    .insert(def.name.clone(), Arc::new(def.clone()));
                Ok(PerlValue::Undef)
            }
            StmtKind::My(decls) | StmtKind::Our(decls) => {
                // For list assignment my ($a, $b) = (10, 20), distribute elements.
                // All decls share the same initializer in the AST (parser clones it).
                if decls.len() > 1 && decls[0].initializer.is_some() {
                    let val = self.eval_expr_ctx(
                        decls[0].initializer.as_ref().unwrap(),
                        WantarrayCtx::List,
                    )?;
                    let items = val.to_list();
                    let mut idx = 0;
                    for decl in decls {
                        match decl.sigil {
                            Sigil::Scalar => {
                                let v = items.get(idx).cloned().unwrap_or(PerlValue::Undef);
                                self.scope.declare_scalar_frozen(
                                    &decl.name,
                                    v,
                                    decl.frozen,
                                    decl.type_annotation,
                                )?;
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
                            let ctx = match decl.sigil {
                                Sigil::Array | Sigil::Hash => WantarrayCtx::List,
                                Sigil::Scalar => WantarrayCtx::Scalar,
                            };
                            self.eval_expr_ctx(init, ctx)?
                        } else {
                            PerlValue::Undef
                        };
                        match decl.sigil {
                            Sigil::Scalar => {
                                self.scope.declare_scalar_frozen(
                                    &decl.name,
                                    val,
                                    decl.frozen,
                                    decl.type_annotation,
                                )?;
                            }
                            Sigil::Array => {
                                let items = val.to_list();
                                self.scope
                                    .declare_array_frozen(&decl.name, items, decl.frozen);
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
                                self.scope.declare_hash_frozen(&decl.name, map, decl.frozen);
                            }
                        }
                    }
                }
                Ok(PerlValue::Undef)
            }
            StmtKind::Local(decls) => {
                if decls.len() > 1 && decls[0].initializer.is_some() {
                    let val = self.eval_expr_ctx(
                        decls[0].initializer.as_ref().unwrap(),
                        WantarrayCtx::List,
                    )?;
                    let items = val.to_list();
                    let mut idx = 0;
                    for decl in decls {
                        match decl.sigil {
                            Sigil::Scalar => {
                                let v = items.get(idx).cloned().unwrap_or(PerlValue::Undef);
                                idx += 1;
                                self.scope.local_set_scalar(&decl.name, v)?;
                            }
                            Sigil::Array => {
                                let rest: Vec<PerlValue> = items[idx..].to_vec();
                                idx = items.len();
                                self.scope.local_set_array(&decl.name, rest)?;
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
                                self.scope.local_set_hash(&decl.name, map)?;
                            }
                        }
                    }
                } else {
                    for decl in decls {
                        let val = if let Some(init) = &decl.initializer {
                            let ctx = match decl.sigil {
                                Sigil::Array | Sigil::Hash => WantarrayCtx::List,
                                Sigil::Scalar => WantarrayCtx::Scalar,
                            };
                            self.eval_expr_ctx(init, ctx)?
                        } else {
                            PerlValue::Undef
                        };
                        match decl.sigil {
                            Sigil::Scalar => {
                                self.scope.local_set_scalar(&decl.name, val)?;
                            }
                            Sigil::Array => {
                                self.scope.local_set_array(&decl.name, val.to_list())?;
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
                                self.scope.local_set_hash(&decl.name, map)?;
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
                            // `deque()` / `heap(...)` are already `Arc<Mutex<…>>`; avoid a second
                            // mutex wrapper. Other scalars (including `Set->new`) use Atomic.
                            let stored = match val {
                                PerlValue::Deque(_) | PerlValue::Heap(_) => val,
                                v => PerlValue::Atomic(std::sync::Arc::new(
                                    parking_lot::Mutex::new(v),
                                )),
                            };
                            self.scope.declare_scalar(&decl.name, stored);
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
                let _ = self
                    .scope
                    .set_scalar("__PACKAGE__", PerlValue::String(name.clone()));
                Ok(PerlValue::Undef)
            }
            StmtKind::Use { .. } => {
                // Handled in `prepare_program_top_level` before BEGIN / main.
                Ok(PerlValue::Undef)
            }
            StmtKind::No { .. } => {
                // Handled in `prepare_program_top_level` (same phase as `use`).
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
            StmtKind::Goto { .. } => {
                Err(PerlError::runtime("goto reached outside goto-aware block", stmt.line).into())
            }
            StmtKind::EvalTimeout { timeout, body } => {
                let secs = self.eval_expr(timeout)?.to_number();
                self.eval_timeout_block(body, secs, stmt.line)
            }
            StmtKind::TryCatch {
                try_block,
                catch_var,
                catch_block,
            } => match self.exec_block(try_block) {
                Ok(v) => Ok(v),
                Err(FlowOrError::Error(e)) => {
                    if matches!(e.kind, ErrorKind::Exit(_)) {
                        return Err(FlowOrError::Error(e));
                    }
                    self.scope.push_frame();
                    self.scope
                        .declare_scalar(catch_var, PerlValue::String(e.to_string()));
                    let r = self.exec_block(catch_block);
                    self.scope.pop_frame();
                    r
                }
                Err(FlowOrError::Flow(f)) => Err(FlowOrError::Flow(f)),
            },
            StmtKind::Given { topic, body } => self.exec_given(topic, body),
            StmtKind::When { .. } | StmtKind::DefaultCase { .. } => Err(PerlError::runtime(
                "when/default may only appear inside a given block",
                stmt.line,
            )
            .into()),
            StmtKind::Continue(block) => self.exec_block_smart(block),
        }
    }

    #[inline]
    fn eval_expr(&mut self, expr: &Expr) -> ExecResult {
        self.eval_expr_ctx(expr, WantarrayCtx::Scalar)
    }

    fn eval_expr_ctx(&mut self, expr: &Expr, ctx: WantarrayCtx) -> ExecResult {
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
                            self.check_strict_scalar_var(name, line)?;
                            let val = self.get_special_var(name);
                            result.push_str(&val.to_string());
                        }
                        StringPart::ArrayVar(name) => {
                            self.check_strict_array_var(name, line)?;
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
            ExprKind::ScalarVar(name) => {
                self.check_strict_scalar_var(name, line)?;
                Ok(self.get_special_var(name))
            }
            ExprKind::ArrayVar(name) => {
                self.check_strict_array_var(name, line)?;
                Ok(PerlValue::Array(self.scope.get_array(name)))
            }
            ExprKind::HashVar(name) => {
                self.check_strict_hash_var(name, line)?;
                Ok(PerlValue::Hash(self.scope.get_hash(name)))
            }
            ExprKind::ArrayElement { array, index } => {
                self.check_strict_array_var(array, line)?;
                let idx = self.eval_expr(index)?.to_int();
                Ok(self.scope.get_array_element(array, idx))
            }
            ExprKind::HashElement { hash, key } => {
                self.check_strict_hash_var(hash, line)?;
                let k = self.eval_expr(key)?.to_string();
                Ok(self.scope.get_hash_element(hash, &k))
            }
            ExprKind::ArraySlice { array, indices } => {
                self.check_strict_array_var(array, line)?;
                let mut result = Vec::new();
                for idx_expr in indices {
                    let idx = self.eval_expr(idx_expr)?.to_int();
                    result.push(self.scope.get_array_element(array, idx));
                }
                Ok(PerlValue::Array(result))
            }
            ExprKind::HashSlice { hash, keys } => {
                self.check_strict_hash_var(hash, line)?;
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
                    prototype: None,
                })))
            }
            ExprKind::Deref { expr, kind } => {
                let val = self.eval_expr(expr)?;
                match kind {
                    Sigil::Scalar => match val {
                        PerlValue::ScalarRef(r) => Ok(r.read().clone()),
                        PerlValue::String(s) => {
                            if self.strict_refs {
                                return Err(PerlError::runtime(
                                    format!(
                                        "Can't use string (\"{}\") as a SCALAR ref while \"strict refs\" in use",
                                        s
                                    ),
                                    line,
                                )
                                .into());
                            }
                            Ok(self.get_special_var(&s))
                        }
                        _ => Err(PerlError::runtime(
                            "Can't dereference non-reference as scalar",
                            line,
                        )
                        .into()),
                    },
                    Sigil::Array => match val {
                        PerlValue::ArrayRef(r) => Ok(PerlValue::Array(r.read().clone())),
                        PerlValue::String(s) => {
                            if self.strict_refs {
                                return Err(PerlError::runtime(
                                    format!(
                                        "Can't use string (\"{}\") as an ARRAY ref while \"strict refs\" in use",
                                        s
                                    ),
                                    line,
                                )
                                .into());
                            }
                            Ok(PerlValue::Array(self.scope.get_array(&s)))
                        }
                        _ => Err(PerlError::runtime(
                            "Can't dereference non-reference as array",
                            line,
                        )
                        .into()),
                    },
                    Sigil::Hash => match val {
                        PerlValue::HashRef(r) => Ok(PerlValue::Hash(r.read().clone())),
                        PerlValue::String(s) => {
                            if self.strict_refs {
                                return Err(PerlError::runtime(
                                    format!(
                                        "Can't use string (\"{}\") as a HASH ref while \"strict refs\" in use",
                                        s
                                    ),
                                    line,
                                )
                                .into());
                            }
                            Ok(PerlValue::Hash(self.scope.get_hash(&s)))
                        }
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
                                PerlValue::CodeRef(sub) => self.call_sub(&sub, args, ctx, line),
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
                    BinOp::BindMatch => {
                        let rv = self.eval_expr(right)?;
                        let s = lv.to_string();
                        let pat = rv.to_string();
                        return self.regex_match_execute(s, &pat, "", false, "_", line);
                    }
                    BinOp::BindNotMatch => {
                        let rv = self.eval_expr(right)?;
                        let s = lv.to_string();
                        let pat = rv.to_string();
                        let m = self.regex_match_execute(s, &pat, "", false, "_", line)?;
                        return Ok(PerlValue::Integer(if m.is_true() { 0 } else { 1 }));
                    }
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
                        self.check_strict_scalar_var(name, line)?;
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
                        self.check_strict_scalar_var(name, line)?;
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
                    self.check_strict_scalar_var(name, line)?;
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
                    self.check_strict_scalar_var(name, line)?;
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
                    self.check_strict_hash_var(hash, line)?;
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
                    self.check_strict_array_var(array, line)?;
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
                if let Some(r) = crate::builtins::try_builtin(self, name.as_str(), &arg_vals, line)
                {
                    return r.map_err(Into::into);
                }
                self.call_named_sub(name, arg_vals, line, ctx)
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
                    self.call_sub(&sub, arg_vals, ctx, line)
                } else if method == "new" {
                    // Default constructor
                    self.builtin_new(&class, arg_vals, line)
                } else if let Some(r) = self.try_autoload_call(&full_name, arg_vals, line, ctx) {
                    r
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
                self.regex_subst_execute(
                    s,
                    pattern,
                    replacement.as_str(),
                    flags.as_str(),
                    expr,
                    line,
                )
            }
            ExprKind::Transliterate {
                expr,
                from,
                to,
                flags,
            } => {
                let val = self.eval_expr(expr)?;
                let s = val.to_string();
                self.regex_transliterate_execute(
                    s,
                    from.as_str(),
                    to.as_str(),
                    flags.as_str(),
                    expr,
                    line,
                )
            }

            // List operations
            ExprKind::MapExpr { block, list } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let mut result = Vec::new();
                for item in items {
                    let _ = self.scope.set_scalar("_", item);
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
                    let _ = self.scope.set_scalar("_", item.clone());
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
                    if let Some(mode) = detect_sort_block_fast(cmp_block) {
                        items.sort_by(|a, b| sort_magic_cmp(a, b, mode));
                    } else {
                        let cmp_block = cmp_block.clone();
                        items.sort_by(|a, b| {
                            let _ = self.scope.set_scalar("a", a.clone());
                            let _ = self.scope.set_scalar("b", b.clone());
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
                    }
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
            ExprKind::ParLinesExpr { path, callback } => {
                let path_s = self.eval_expr(path)?.to_string();
                let cb_val = self.eval_expr(callback)?;
                let sub = match cb_val {
                    PerlValue::CodeRef(s) => s,
                    _ => {
                        return Err(PerlError::runtime(
                            "par_lines: second argument must be a code reference",
                            line,
                        )
                        .into());
                    }
                };
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();
                let file = std::fs::File::open(std::path::Path::new(&path_s)).map_err(|e| {
                    FlowOrError::Error(PerlError::runtime(format!("par_lines: {}", e), line))
                })?;
                let mmap = unsafe {
                    memmap2::Mmap::map(&file).map_err(|e| {
                        FlowOrError::Error(PerlError::runtime(
                            format!("par_lines: mmap: {}", e),
                            line,
                        ))
                    })?
                };
                let data: &[u8] = &mmap;
                if data.is_empty() {
                    return Ok(PerlValue::Undef);
                }
                let num_chunks = self.num_threads.saturating_mul(8).max(1);
                let chunks = crate::par_lines::line_aligned_chunks(data, num_chunks);
                chunks.into_par_iter().try_for_each(|(start, end)| {
                    let slice = &data[start..end];
                    let mut s = 0usize;
                    while s < slice.len() {
                        let e = slice[s..]
                            .iter()
                            .position(|&b| b == b'\n')
                            .map(|p| s + p)
                            .unwrap_or(slice.len());
                        let line_bytes = &slice[s..e];
                        let line_str = crate::par_lines::line_to_perl_string(line_bytes);
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        local_interp
                            .scope
                            .restore_atomics(&atomic_arrays, &atomic_hashes);
                        let _ = local_interp
                            .scope
                            .set_scalar("_", PerlValue::String(line_str));
                        match local_interp.call_sub(&sub, vec![], WantarrayCtx::Void, line) {
                            Ok(_) => {}
                            Err(e) => return Err(e),
                        }
                        if e >= slice.len() {
                            break;
                        }
                        s = e + 1;
                    }
                    Ok(())
                })?;
                Ok(PerlValue::Undef)
            }
            ExprKind::PwatchExpr { path, callback } => {
                let pattern_s = self.eval_expr(path)?.to_string();
                let cb_val = self.eval_expr(callback)?;
                let sub = match cb_val {
                    PerlValue::CodeRef(s) => s,
                    _ => {
                        return Err(PerlError::runtime(
                            "pwatch: second argument must be a code reference",
                            line,
                        )
                        .into());
                    }
                };
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();
                Ok(crate::pwatch::run_pwatch(
                    &pattern_s,
                    sub,
                    subs,
                    scope_capture,
                    atomic_arrays,
                    atomic_hashes,
                    line,
                )?)
            }
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
                        let _ = local_interp.scope.set_scalar("_", item);
                        match local_interp.exec_block(&block) {
                            Ok(val) => val,
                            Err(_) => PerlValue::Undef,
                        }
                    })
                    .collect();
                Ok(PerlValue::Array(results))
            }
            ExprKind::PMapChunkedExpr {
                chunk_size,
                block,
                list,
            } => {
                let chunk_n = self.eval_expr(chunk_size)?.to_int().max(1) as usize;
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();

                let indexed_chunks: Vec<(usize, Vec<PerlValue>)> = items
                    .chunks(chunk_n)
                    .enumerate()
                    .map(|(i, c)| (i, c.to_vec()))
                    .collect();

                let mut chunk_results: Vec<(usize, Vec<PerlValue>)> = indexed_chunks
                    .into_par_iter()
                    .map(|(chunk_idx, chunk)| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        local_interp
                            .scope
                            .restore_atomics(&atomic_arrays, &atomic_hashes);
                        let mut out = Vec::with_capacity(chunk.len());
                        for item in chunk {
                            let _ = local_interp.scope.set_scalar("_", item);
                            match local_interp.exec_block(&block) {
                                Ok(val) => out.push(val),
                                Err(_) => out.push(PerlValue::Undef),
                            }
                        }
                        (chunk_idx, out)
                    })
                    .collect();

                chunk_results.sort_by_key(|(i, _)| *i);
                let results: Vec<PerlValue> =
                    chunk_results.into_iter().flat_map(|(_, v)| v).collect();
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
                        let _ = local_interp.scope.set_scalar("_", item.clone());
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
                    let _ = local_interp.scope.set_scalar("_", item);
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
                    let _ = local_interp
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
            ExprKind::Timer { body } => {
                let start = std::time::Instant::now();
                self.exec_block(body)?;
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                Ok(PerlValue::Float(ms))
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
            ExprKind::Capture(e) => {
                let cmd = self.eval_expr(e)?.to_string();
                crate::capture::run_capture(&cmd, line).map_err(Into::into)
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
                    if let Some(mode) = detect_sort_block_fast(cmp_block) {
                        items.par_sort_by(|a, b| sort_magic_cmp(a, b, mode));
                    } else {
                        let cmp_block = cmp_block.clone();
                        let subs = self.subs.clone();
                        let scope_capture = self.scope.capture();
                        items.par_sort_by(|a, b| {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            let _ = local_interp.scope.set_scalar("a", a.clone());
                            let _ = local_interp.scope.set_scalar("b", b.clone());
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
                    }
                } else {
                    items.par_sort_by(|a, b| a.to_string().cmp(&b.to_string()));
                }
                Ok(PerlValue::Array(items))
            }

            ExprKind::ReduceExpr { block, list } => {
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
                let mut acc = items[0].clone();
                for b in items.into_iter().skip(1) {
                    let mut local_interp = Interpreter::new();
                    local_interp.subs = subs.clone();
                    local_interp.scope.restore_capture(&scope_capture);
                    let _ = local_interp.scope.set_scalar("a", acc);
                    let _ = local_interp.scope.set_scalar("b", b);
                    acc = match local_interp.exec_block(&block) {
                        Ok(val) => val,
                        Err(_) => PerlValue::Undef,
                    };
                }
                Ok(acc)
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
                    let _ = local_interp.scope.set_scalar("a", a);
                    let _ = local_interp.scope.set_scalar("b", b);
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
                if self.scope.is_array_frozen(&arr_name) {
                    return Err(PerlError::runtime(
                        format!("Modification of a frozen value: @{}", arr_name),
                        line,
                    )
                    .into());
                }
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
                self.chomp_inplace_execute(val, expr)
            }
            ExprKind::Chop(expr) => {
                let val = self.eval_expr(expr)?;
                self.chop_inplace_execute(val, expr)
            }
            ExprKind::Length(expr) => {
                let val = self.eval_expr(expr)?;
                match val {
                    PerlValue::Array(a) => Ok(PerlValue::Integer(a.len() as i64)),
                    PerlValue::Hash(h) => Ok(PerlValue::Integer(h.len() as i64)),
                    PerlValue::Bytes(b) => Ok(PerlValue::Integer(b.len() as i64)),
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
                let file_opt = if let Some(f) = file {
                    Some(self.eval_expr(f)?.to_string())
                } else {
                    None
                };
                self.open_builtin_execute(handle_name, mode_s, file_opt, line)
                    .map_err(Into::into)
            }
            ExprKind::Close(expr) => {
                let name = self.eval_expr(expr)?.to_string();
                self.close_builtin_execute(name).map_err(Into::into)
            }
            ExprKind::ReadLine(handle) => self
                .readline_builtin_execute(handle.as_deref())
                .map_err(Into::into),
            ExprKind::Eof(expr) => {
                if let Some(e) = expr {
                    let name = self.eval_expr(e)?.to_string();
                    let at_eof = !self.has_input_handle(&name);
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
                    't' => crate::perl_fs::filetest_is_tty(&path),
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
                let spec = self.eval_expr(expr)?.to_string();
                self.require_execute(&spec, line)
                    .map_err(FlowOrError::Error)
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
            ExprKind::Rename { old, new } => {
                let o = self.eval_expr(old)?.to_string();
                let n = self.eval_expr(new)?.to_string();
                Ok(crate::perl_fs::rename_paths(&o, &n))
            }
            ExprKind::Chmod(args) => {
                let mode = self.eval_expr(&args[0])?.to_int();
                let mut paths = Vec::new();
                for a in &args[1..] {
                    paths.push(self.eval_expr(a)?.to_string());
                }
                Ok(PerlValue::Integer(crate::perl_fs::chmod_paths(
                    &paths, mode,
                )))
            }
            ExprKind::Chown(args) => {
                let uid = self.eval_expr(&args[0])?.to_int();
                let gid = self.eval_expr(&args[1])?.to_int();
                let mut paths = Vec::new();
                for a in &args[2..] {
                    paths.push(self.eval_expr(a)?.to_string());
                }
                Ok(PerlValue::Integer(crate::perl_fs::chown_paths(
                    &paths, uid, gid,
                )))
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
                Ok(crate::perl_fs::glob_par_patterns(&pats))
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
            ExprKind::Wantarray => Ok(match self.wantarray_kind {
                WantarrayCtx::Void => PerlValue::Undef,
                WantarrayCtx::Scalar => PerlValue::Integer(0),
                WantarrayCtx::List => PerlValue::Integer(1),
            }),

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
                    let _ = self.scope.set_scalar("_", item);
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
            BinOp::BindMatch | BinOp::BindNotMatch => {
                unreachable!("regex bind handled in eval_expr BinOp arm")
            }
        })
    }

    fn assign_value(&mut self, target: &Expr, val: PerlValue) -> ExecResult {
        match &target.kind {
            ExprKind::ScalarVar(name) => {
                if self.scope.is_scalar_frozen(name) {
                    return Err(PerlError::runtime(
                        format!("Modification of a frozen value: ${}", name),
                        target.line,
                    )
                    .into());
                }
                if name == "_"
                    || name == "0"
                    || name == "!"
                    || name.starts_with(|c: char| c.is_ascii_digit())
                {
                    self.set_special_var(name, &val)?;
                } else {
                    if self.strict_vars
                        && !Self::strict_scalar_exempt(name)
                        && !name.contains("::")
                        && !self.scope.scalar_binding_exists(name)
                    {
                        return Err(PerlError::runtime(
                            format!(
                                "Global symbol \"${}\" requires explicit package name (did you forget to declare \"my ${}\"?)",
                                name, name
                            ),
                            target.line,
                        )
                        .into());
                    }
                    self.scope
                        .set_scalar(name, val)
                        .map_err(|e| FlowOrError::Error(e.at_line(target.line)))?;
                }
                Ok(PerlValue::Undef)
            }
            ExprKind::ArrayVar(name) => {
                if self.scope.is_array_frozen(name) {
                    return Err(PerlError::runtime(
                        format!("Modification of a frozen value: @{}", name),
                        target.line,
                    )
                    .into());
                }
                if self.strict_vars
                    && !name.contains("::")
                    && !self.scope.array_binding_exists(name)
                {
                    return Err(PerlError::runtime(
                        format!(
                            "Global symbol \"@{}\" requires explicit package name (did you forget to declare \"my @{}\"?)",
                            name, name
                        ),
                        target.line,
                    )
                    .into());
                }
                self.scope.set_array(name, val.to_list());
                Ok(PerlValue::Undef)
            }
            ExprKind::HashVar(name) => {
                if self.strict_vars
                    && !name.contains("::")
                    && !self.scope.hash_binding_exists(name)
                {
                    return Err(PerlError::runtime(
                        format!(
                            "Global symbol \"%{}\" requires explicit package name (did you forget to declare \"my %{}\"?)",
                            name, name
                        ),
                        target.line,
                    )
                    .into());
                }
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
                if self.strict_vars
                    && !array.contains("::")
                    && !self.scope.array_binding_exists(array)
                {
                    return Err(PerlError::runtime(
                        format!(
                            "Global symbol \"@{}\" requires explicit package name (did you forget to declare \"my @{}\"?)",
                            array, array
                        ),
                        target.line,
                    )
                    .into());
                }
                if self.scope.is_array_frozen(array) {
                    return Err(PerlError::runtime(
                        format!("Modification of a frozen value: @{}", array),
                        target.line,
                    )
                    .into());
                }
                let idx = self.eval_expr(index)?.to_int();
                self.scope.set_array_element(array, idx, val);
                Ok(PerlValue::Undef)
            }
            ExprKind::HashElement { hash, key } => {
                if self.strict_vars
                    && !hash.contains("::")
                    && !self.scope.hash_binding_exists(hash)
                {
                    return Err(PerlError::runtime(
                        format!(
                            "Global symbol \"%{}\" requires explicit package name (did you forget to declare \"my %{}\"?)",
                            hash, hash
                        ),
                        target.line,
                    )
                    .into());
                }
                if self.scope.is_hash_frozen(hash) {
                    return Err(PerlError::runtime(
                        format!("Modification of a frozen value: %%{}", hash),
                        target.line,
                    )
                    .into());
                }
                let k = self.eval_expr(key)?.to_string();
                self.scope.set_hash_element(hash, &k, val);
                Ok(PerlValue::Undef)
            }
            _ => Ok(PerlValue::Undef),
        }
    }

    fn get_special_var(&self, name: &str) -> PerlValue {
        match name {
            "$$" => PerlValue::Integer(std::process::id() as i64),
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

    fn set_special_var(&mut self, name: &str, val: &PerlValue) -> Result<(), PerlError> {
        match name {
            "0" => self.program_name = val.to_string(),
            "/" => self.irs = val.to_string(),
            "\\" => self.ors = val.to_string(),
            "," => self.ofs = val.to_string(),
            _ => self.scope.set_scalar(name, val.clone())?,
        }
        Ok(())
    }

    fn extract_array_name(&self, expr: &Expr) -> Result<String, FlowOrError> {
        match &expr.kind {
            ExprKind::ArrayVar(name) => Ok(name.clone()),
            ExprKind::ScalarVar(name) => Ok(name.clone()), // @_ written as shift of implicit
            _ => Err(PerlError::runtime("Expected array", expr.line).into()),
        }
    }

    /// Current package (`main` when `__PACKAGE__` is unset or empty).
    fn current_package(&self) -> String {
        let s = self.scope.get_scalar("__PACKAGE__").to_string();
        if s.is_empty() {
            "main".to_string()
        } else {
            s
        }
    }

    /// If `sub AUTOLOAD` exists, set `$AUTOLOAD` to the fully qualified missing sub or method name
    /// and invoke the handler (same argument list as the missing call).
    pub(crate) fn try_autoload_call(
        &mut self,
        missing_name: &str,
        args: Vec<PerlValue>,
        line: usize,
        want: WantarrayCtx,
    ) -> Option<ExecResult> {
        let sub = self.subs.get("AUTOLOAD")?.clone();
        let pkg = self.current_package();
        let full = if missing_name.contains("::") {
            missing_name.to_string()
        } else {
            format!("{}::{}", pkg, missing_name)
        };
        if let Err(e) = self.scope.set_scalar("AUTOLOAD", PerlValue::String(full)) {
            return Some(Err(e.into()));
        }
        Some(self.call_sub(&sub, args, want, line))
    }

    fn call_named_sub(
        &mut self,
        name: &str,
        args: Vec<PerlValue>,
        line: usize,
        want: WantarrayCtx,
    ) -> ExecResult {
        if let Some(sub) = self.resolve_sub_by_name(name) {
            return self.call_sub(&sub, args, want, line);
        }
        match name {
            "deque" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("deque() takes no arguments", line).into());
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
                    PerlValue::CodeRef(sub) => {
                        Ok(PerlValue::Heap(Arc::new(Mutex::new(PerlHeap {
                            items: Vec::new(),
                            cmp: sub.clone(),
                        }))))
                    }
                    _ => Err(PerlError::runtime("heap() requires a code reference", line).into()),
                }
            }
            "pipeline" => Ok(PerlValue::Pipeline(Arc::new(Mutex::new(PipelineInner {
                source: args,
                ops: Vec::new(),
            })))),
            "ppool" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "ppool() expects one argument (worker count)",
                        line,
                    )
                    .into());
                }
                crate::ppool::create_pool(args[0].to_int().max(0) as usize).map_err(Into::into)
            }
            "barrier" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "barrier() expects one argument (party count)",
                        line,
                    )
                    .into());
                }
                let n = args[0].to_int().max(1) as usize;
                Ok(PerlValue::Barrier(PerlBarrier(Arc::new(Barrier::new(n)))))
            }
            _ => {
                if let Some(r) = self.try_autoload_call(name, args, line, want) {
                    return r;
                }
                let mut msg = format!("Undefined subroutine &{}", name);
                if self.strict_subs {
                    msg.push_str(
                        " (strict subs: declare the sub or use a fully qualified name before calling)",
                    );
                }
                Err(PerlError::runtime(msg, line).into())
            }
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
            PerlValue::SqliteConn(c) => {
                Some(crate::native_data::sqlite_dispatch(c, method, args, line))
            }
            PerlValue::StructInst(s) => {
                if let Some(idx) = s.def.field_index(method) {
                    if !args.is_empty() {
                        return Some(Err(PerlError::runtime(
                            format!("struct field `{}` takes no arguments", method),
                            line,
                        )));
                    }
                    Some(Ok(s.values[idx].clone()))
                } else {
                    None
                }
            }
            PerlValue::Deque(d) => Some(self.deque_method(Arc::clone(d), method, args, line)),
            PerlValue::Heap(h) => Some(self.heap_method(Arc::clone(h), method, args, line)),
            PerlValue::Pipeline(p) => Some(self.pipeline_method(Arc::clone(p), method, args, line)),
            PerlValue::Capture(c) => Some(self.capture_method(Arc::clone(c), method, args, line)),
            PerlValue::Ppool(p) => Some(self.ppool_method(p.clone(), method, args, line)),
            PerlValue::Barrier(b) => Some(self.barrier_method(b.clone(), method, args, line)),
            PerlValue::Atomic(arc) => match &*arc.lock() {
                PerlValue::Deque(d) => Some(self.deque_method(Arc::clone(d), method, args, line)),
                PerlValue::Heap(h) => Some(self.heap_method(Arc::clone(h), method, args, line)),
                _ => None,
            },
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
            "peek" => Ok(h.lock().items.first().cloned().unwrap_or(PerlValue::Undef)),
            _ => Err(PerlError::runtime(
                format!("Unknown method for heap: {}", method),
                line,
            )),
        }
    }

    fn ppool_method(
        &mut self,
        pool: PerlPpool,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match method {
            "submit" => pool.submit(self, args, line),
            "collect" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("collect() takes no arguments", line));
                }
                pool.collect(line)
            }
            _ => Err(PerlError::runtime(
                format!("Unknown method for ppool: {}", method),
                line,
            )),
        }
    }

    fn barrier_method(
        &self,
        barrier: PerlBarrier,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match method {
            "wait" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("wait() takes no arguments", line));
                }
                let _ = barrier.0.wait();
                Ok(PerlValue::Integer(1))
            }
            _ => Err(PerlError::runtime(
                format!("Unknown method for barrier: {}", method),
                line,
            )),
        }
    }

    fn capture_method(
        &self,
        c: Arc<CaptureResult>,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        if !args.is_empty() {
            return Err(PerlError::runtime(
                format!("capture: {} takes no arguments", method),
                line,
            ));
        }
        match method {
            "stdout" => Ok(PerlValue::String(c.stdout.clone())),
            "stderr" => Ok(PerlValue::String(c.stderr.clone())),
            "exitcode" => Ok(PerlValue::Integer(c.exitcode)),
            "failed" => Ok(PerlValue::Integer(if c.exitcode != 0 { 1 } else { 0 })),
            _ => Err(PerlError::runtime(
                format!("Unknown method for capture: {}", method),
                line,
            )),
        }
    }

    fn pipeline_method(
        &mut self,
        p: Arc<Mutex<PipelineInner>>,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match method {
            "filter" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "pipeline filter expects 1 argument (sub)",
                        line,
                    ));
                }
                let PerlValue::CodeRef(sub) = &args[0] else {
                    return Err(PerlError::runtime(
                        "pipeline filter expects a code reference",
                        line,
                    ));
                };
                p.lock().ops.push(PipelineOp::Filter(sub.clone()));
                Ok(PerlValue::Pipeline(Arc::clone(&p)))
            }
            "map" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "pipeline map expects 1 argument (sub)",
                        line,
                    ));
                }
                let PerlValue::CodeRef(sub) = &args[0] else {
                    return Err(PerlError::runtime(
                        "pipeline map expects a code reference",
                        line,
                    ));
                };
                p.lock().ops.push(PipelineOp::Map(sub.clone()));
                Ok(PerlValue::Pipeline(Arc::clone(&p)))
            }
            "take" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime("pipeline take expects 1 argument", line));
                }
                let n = args[0].to_int();
                p.lock().ops.push(PipelineOp::Take(n));
                Ok(PerlValue::Pipeline(Arc::clone(&p)))
            }
            "collect" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime(
                        "pipeline collect takes no arguments",
                        line,
                    ));
                }
                self.pipeline_collect(&p)
            }
            _ => Err(PerlError::runtime(
                format!("Unknown method for pipeline: {}", method),
                line,
            )),
        }
    }

    fn pipeline_collect(&mut self, p: &Arc<Mutex<PipelineInner>>) -> PerlResult<PerlValue> {
        let (mut v, ops) = {
            let g = p.lock();
            (g.source.clone(), g.ops.clone())
        };
        for op in ops {
            match op {
                PipelineOp::Filter(sub) => {
                    let mut out = Vec::new();
                    for item in v {
                        self.scope.push_frame();
                        let _ = self.scope.set_scalar("_", item.clone());
                        if let Some(ref env) = sub.closure_env {
                            self.scope.restore_capture(env);
                        }
                        let keep = match self.exec_block_no_scope(&sub.body) {
                            Ok(val) => val.is_true(),
                            Err(_) => false,
                        };
                        self.scope.pop_frame();
                        if keep {
                            out.push(item);
                        }
                    }
                    v = out;
                }
                PipelineOp::Map(sub) => {
                    let mut out = Vec::new();
                    for item in v {
                        self.scope.push_frame();
                        let _ = self.scope.set_scalar("_", item);
                        if let Some(ref env) = sub.closure_env {
                            self.scope.restore_capture(env);
                        }
                        let mapped = match self.exec_block_no_scope(&sub.body) {
                            Ok(val) => val,
                            Err(_) => PerlValue::Undef,
                        };
                        self.scope.pop_frame();
                        out.push(mapped);
                    }
                    v = out;
                }
                PipelineOp::Take(n) => {
                    let n = n.max(0) as usize;
                    if v.len() > n {
                        v.truncate(n);
                    }
                }
            }
        }
        Ok(PerlValue::Array(v))
    }

    fn heap_compare(&mut self, cmp: &Arc<PerlSub>, a: &PerlValue, b: &PerlValue) -> Ordering {
        self.scope.push_frame();
        let _ = self.scope.set_scalar("a", a.clone());
        let _ = self.scope.set_scalar("b", b.clone());
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

    fn heap_sift_up(&mut self, items: &mut [PerlValue], cmp: &Arc<PerlSub>, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            if self.heap_compare(cmp, &items[i], &items[p]) != Ordering::Less {
                break;
            }
            items.swap(i, p);
            i = p;
        }
    }

    fn heap_sift_down(&mut self, items: &mut [PerlValue], cmp: &Arc<PerlSub>, mut i: usize) {
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

    pub(crate) fn call_sub(
        &mut self,
        sub: &PerlSub,
        args: Vec<PerlValue>,
        want: WantarrayCtx,
        _line: usize,
    ) -> ExecResult {
        // Single frame for both @_ and the block's local variables —
        // avoids the double push_frame/pop_frame overhead per call.
        self.scope.push_frame();
        self.scope.declare_array("_", args);
        if let Some(ref env) = sub.closure_env {
            self.scope.restore_capture(env);
        }
        let saved = self.wantarray_kind;
        self.wantarray_kind = want;
        let t0 = self.profiler.is_some().then(std::time::Instant::now);
        if let Some(p) = &mut self.profiler {
            p.enter_sub(&sub.name);
        }
        let result = self.exec_block_no_scope(&sub.body);
        if let (Some(p), Some(t0)) = (&mut self.profiler, t0) {
            p.exit_sub(t0.elapsed());
        }
        self.wantarray_kind = saved;
        self.scope.pop_frame();
        match result {
            Ok(v) => Ok(v),
            Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
            Err(e) => Err(e),
        }
    }

    fn builtin_new(&mut self, class: &str, args: Vec<PerlValue>, line: usize) -> ExecResult {
        if class == "Set" {
            return Ok(crate::value::set_from_elements(args.into_iter().skip(1)));
        }
        if let Some(def) = self.struct_defs.get(class) {
            return Ok(crate::native_data::struct_new(def, &args, line)?);
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
        if newline && (self.feature_bits & FEAT_SAY) == 0 {
            return Err(PerlError::runtime(
                "say() is disabled (enable with use feature 'say' or use feature ':5.10')",
                line,
            )
            .into());
        }
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
        let expanded = expand_perl_regex_quotemeta(pattern);
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
        re_str.push_str(&expanded);
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
        let _ = self
            .scope
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

#[cfg(test)]
mod regex_expand_tests {
    use super::Interpreter;

    #[test]
    fn compile_regex_quotemeta_qe_matches_literal() {
        let mut i = Interpreter::new();
        let re = i.compile_regex(r"\Qa.c\E", "", 1).expect("regex");
        assert!(re.is_match("a.c"));
        assert!(!re.is_match("abc"));
    }
}
