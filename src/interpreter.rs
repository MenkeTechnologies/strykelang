use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor, Read, Write as IoWrite};
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Barrier, OnceLock};

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
use crate::mro::linearize_c3;
use crate::perl_regex::{PerlCaptures, PerlCompiledRegex};
use crate::pmap_progress::{FanProgress, PmapProgress};
use crate::profiler::Profiler;
use crate::scope::Scope;
use crate::sort_fast::{detect_sort_block_fast, sort_magic_cmp};
use crate::value::{
    CaptureResult, PerlAsyncTask, PerlBarrier, PerlDataFrame, PerlHeap, PerlPpool, PerlSub,
    PerlValue, PipelineInner, PipelineOp,
};

/// Merge two counting-hash accumulators (parallel `preduce_init` partials).
/// Returns a hashref so arrow deref (`$acc->{k}`) stays valid after parallel merge.
pub(crate) fn preduce_init_merge_maps(
    mut acc: IndexMap<String, PerlValue>,
    b: IndexMap<String, PerlValue>,
) -> PerlValue {
    for (k, v2) in b {
        acc.entry(k)
            .and_modify(|v1| *v1 = PerlValue::float(v1.to_number() + v2.to_number()))
            .or_insert(v2);
    }
    PerlValue::hash_ref(Arc::new(RwLock::new(acc)))
}

/// Combine two partial results from `preduce_init`: hash/hashref maps add per-key counts; otherwise
/// the fold block is invoked with `$a` / `$b` as the two partial accumulators (associative combine).
pub(crate) fn merge_preduce_init_partials(
    a: PerlValue,
    b: PerlValue,
    block: &Block,
    subs: &HashMap<String, Arc<PerlSub>>,
    scope_capture: &[(String, PerlValue)],
) -> PerlValue {
    if let (Some(m1), Some(m2)) = (a.as_hash_map(), b.as_hash_map()) {
        return preduce_init_merge_maps(m1, m2);
    }
    if let (Some(r1), Some(r2)) = (a.as_hash_ref(), b.as_hash_ref()) {
        let m1 = r1.read().clone();
        let m2 = r2.read().clone();
        return preduce_init_merge_maps(m1, m2);
    }
    if let Some(m1) = a.as_hash_map() {
        if let Some(r2) = b.as_hash_ref() {
            let m2 = r2.read().clone();
            return preduce_init_merge_maps(m1, m2);
        }
    }
    if let Some(r1) = a.as_hash_ref() {
        if let Some(m2) = b.as_hash_map() {
            let m1 = r1.read().clone();
            return preduce_init_merge_maps(m1, m2);
        }
    }
    let mut local_interp = Interpreter::new();
    local_interp.subs = subs.clone();
    local_interp.scope.restore_capture(scope_capture);
    local_interp.enable_parallel_guard();
    local_interp
        .scope
        .declare_array("_", vec![a.clone(), b.clone()]);
    let _ = local_interp.scope.set_scalar("a", a);
    let _ = local_interp.scope.set_scalar("b", b);
    match local_interp.exec_block(block) {
        Ok(val) => val,
        Err(_) => PerlValue::UNDEF,
    }
}

/// Seed each parallel chunk from `init` without sharing mutable hashref storage (plain `clone` on
/// `HashRef` reuses the same `Arc<RwLock<…>>`).
pub(crate) fn preduce_init_fold_identity(init: &PerlValue) -> PerlValue {
    if let Some(m) = init.as_hash_map() {
        return PerlValue::hash(m.clone());
    }
    if let Some(r) = init.as_hash_ref() {
        return PerlValue::hash_ref(Arc::new(RwLock::new(r.read().clone())));
    }
    init.clone()
}

pub(crate) fn fold_preduce_init_step(
    subs: &HashMap<String, Arc<PerlSub>>,
    scope_capture: &[(String, PerlValue)],
    block: &Block,
    acc: PerlValue,
    item: PerlValue,
) -> PerlValue {
    let mut local_interp = Interpreter::new();
    local_interp.subs = subs.clone();
    local_interp.scope.restore_capture(scope_capture);
    local_interp.enable_parallel_guard();
    local_interp
        .scope
        .declare_array("_", vec![acc.clone(), item.clone()]);
    let _ = local_interp.scope.set_scalar("a", acc);
    let _ = local_interp.scope.set_scalar("b", item);
    match local_interp.exec_block(block) {
        Ok(val) => val,
        Err(_) => PerlValue::UNDEF,
    }
}

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

/// Perl `$]` — numeric language level (`5 + minor/1000 + patch/1_000_000`).
/// Emulated Perl 5.x level (not the `perlrs` crate semver).
pub fn perl_bracket_version() -> f64 {
    const PERL_EMUL_MINOR: u32 = 38;
    const PERL_EMUL_PATCH: u32 = 0;
    5.0 + (PERL_EMUL_MINOR as f64) / 1000.0 + (PERL_EMUL_PATCH as f64) / 1_000_000.0
}

/// Cheap seed for [`StdRng`] at startup (avoids `getentropy` / blocking sources).
#[inline]
fn fast_rng_seed() -> u64 {
    let local: u8 = 0;
    let addr = &local as *const u8 as u64;
    (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ addr
}

/// `$^X` — cache `current_exe()` once per process (tiny win on repeated `Interpreter::new`).
fn cached_executable_path() -> String {
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            std::env::current_exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "perlrs".to_string())
        })
        .clone()
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
    /// Numeric errno for `$!` dualvar (`raw_os_error()`), `0` when unset.
    pub errno_code: i32,
    /// $@ — last eval error (string)
    pub eval_error: String,
    /// Numeric side of `$@` dualvar (`0` when cleared; `1` for typical exception strings; or explicit code from assignment / dualvar).
    pub eval_error_code: i32,
    /// @ARGV
    pub argv: Vec<String>,
    /// %ENV (mirrors `scope` hash `"ENV"` after [`Self::materialize_env_if_needed`])
    pub env: IndexMap<String, PerlValue>,
    /// False until first [`Self::materialize_env_if_needed`] (defers `std::env::vars()` cost).
    pub env_materialized: bool,
    /// $0
    pub program_name: String,
    /// Current line number $. (global increment; see `handle_line_numbers` for per-handle)
    pub line_number: i64,
    /// Last handle key used for `$.` (e.g. `STDIN`, `FH`, `ARGV:path`).
    pub last_readline_handle: String,
    /// Line count per handle for `$.` when keyed (Perl-style last-read handle).
    pub handle_line_numbers: HashMap<String, i64>,
    /// `$^C` — set when SIGINT is pending before handler runs (cleared on read).
    pub sigint_pending_caret: Cell<bool>,
    /// Auto-split mode (-a)
    pub auto_split: bool,
    /// Field separator for -F
    pub field_separator: Option<String>,
    /// BEGIN blocks
    begin_blocks: Vec<Block>,
    /// `UNITCHECK` blocks (LIFO at run)
    unit_check_blocks: Vec<Block>,
    /// `CHECK` blocks (LIFO at run)
    check_blocks: Vec<Block>,
    /// `INIT` blocks (FIFO at run)
    init_blocks: Vec<Block>,
    /// END blocks
    end_blocks: Vec<Block>,
    /// -w warnings / `use warnings` / `$^W`
    pub warnings: bool,
    /// Output autoflush (`$|`).
    pub output_autoflush: bool,
    /// Suppress stdout output (fan workers with progress bars).
    pub suppress_stdout: bool,
    /// Child wait status (`$?`) — POSIX-style (exit code in high byte, etc.).
    pub child_exit_status: i64,
    /// Last successful match (`$&`, `${^MATCH}`).
    pub last_match: String,
    /// Before match (`` $` ``, `${^PREMATCH}`).
    pub prematch: String,
    /// After match (`$'`, `${^POSTMATCH}`).
    pub postmatch: String,
    /// Last bracket match (`$+`, `${^LAST_SUBMATCH_RESULT}`).
    pub last_paren_match: String,
    /// List separator for array stringification in concatenation / interpolation (`$"`).
    pub list_separator: String,
    /// Script start time (`$^T`) — seconds since Unix epoch.
    pub script_start_time: i64,
    /// `$^H` — compile-time hints (bit flags; pragma / `BEGIN` may update).
    pub compile_hints: i64,
    /// `${^WARNING_BITS}` — warnings bitmask (Perl internal; surfaced for compatibility).
    pub warning_bits: i64,
    /// `${^GLOBAL_PHASE}` — interpreter phase (`RUN`, …).
    pub global_phase: String,
    /// `$;` — hash subscript separator (multi-key join); Perl default `\034`.
    pub subscript_sep: String,
    /// `$^I` — in-place edit backup suffix (empty when no backup; also unset when `-i` was not passed).
    /// The `pe` driver sets this from `-i` / `-i.ext`.
    pub inplace_edit: String,
    /// `$^D` — debugging flags (integer; mostly ignored).
    pub debug_flags: i64,
    /// `$^P` — debugging / profiling flags (integer; mostly ignored).
    pub perl_debug_flags: i64,
    /// Nesting depth for `eval` / `evalblock` (`$^S` is non-zero while inside eval).
    pub eval_nesting: u32,
    /// `$ARGV` — name of the file last opened by `<>` (empty for stdin or before first file).
    pub argv_current_file: String,
    /// Next `@ARGV` index to open for `<>` (after `ARGV` is exhausted, `<>` returns undef).
    pub(crate) diamond_next_idx: usize,
    /// Buffered reader for the current `<>` file (stdin uses the existing stdin path).
    pub(crate) diamond_reader: Option<BufReader<File>>,
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
    /// Compiled regex cache: "flags///pattern" → [`PerlCompiledRegex`] (Rust `regex` or `fancy-regex`).
    regex_cache: HashMap<String, Arc<PerlCompiledRegex>>,
    /// Last compiled regex — fast-path to avoid format! + HashMap lookup in tight loops.
    /// Third flag: `$*` multiline (prepends `(?s)` when true).
    regex_last: Option<(String, String, bool, Arc<PerlCompiledRegex>)>,
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
    /// Per-module `our @EXPORT` / `our @EXPORT_OK` (Exporter-style). Absent key → legacy import-all.
    pub(crate) module_export_lists: HashMap<String, ModuleExportLists>,
    /// `tie %name, ...` — object that implements FETCH/STORE for that hash.
    pub(crate) tied_hashes: HashMap<String, PerlValue>,
    /// `tie $name` — TIESCALAR object for FETCH/STORE.
    pub(crate) tied_scalars: HashMap<String, PerlValue>,
    /// `tie @name` — TIEARRAY object for FETCH/STORE (indexed).
    pub(crate) tied_arrays: HashMap<String, PerlValue>,
    /// `use overload` — class → Perl overload key → short method name in that package.
    pub(crate) overload_table: HashMap<String, HashMap<String, String>>,
    /// `format NAME =` bodies (parsed) keyed `Package::NAME`.
    pub(crate) format_templates: HashMap<String, Arc<crate::format::FormatTemplate>>,
    /// `${^NAME}` scalars not stored in dedicated fields (default `undef`; assign may stash).
    pub(crate) special_caret_scalars: HashMap<String, PerlValue>,
    /// `$%` — format output page number.
    pub format_page_number: i64,
    /// `$=` — format lines per page.
    pub format_lines_per_page: i64,
    /// `$-` — lines remaining on format page.
    pub format_lines_left: i64,
    /// `$:` — characters to break format lines (Perl default `\n`).
    pub format_line_break_chars: String,
    /// `$^` — top-of-form format name.
    pub format_top_name: String,
    /// `$^A` — format write accumulator.
    pub accumulator_format: String,
    /// `$^F` — max system file descriptor (Perl default 2).
    pub max_system_fd: i64,
    /// `$^M` — emergency memory buffer (no-op pool in perlrs).
    pub emergency_memory: String,
    /// `$^N` — last opened named regexp capture name.
    pub last_subpattern_name: String,
    /// `$INC` — `@INC` hook iterator (Perl 5.37+).
    pub inc_hook_index: i64,
    /// `$*` — multiline matching (deprecated in Perl); when true, `compile_regex` prepends `(?s)`.
    pub multiline_match: bool,
    /// `$^X` — path to this executable (cached).
    pub executable_path: String,
    /// `$^L` — formfeed string for formats (Perl default `\f`).
    pub formfeed_string: String,
    /// Limited typeglob: I/O handle alias (`*FOO` → underlying handle name).
    pub(crate) glob_handle_alias: HashMap<String, String>,
    /// Parallel to [`Scope`] frames: `local *GLOB` entries to restore on [`Self::scope_pop_hook`].
    glob_restore_frames: Vec<Vec<(String, Option<String>)>>,
    /// `use English` — long names ([`crate::english::scalar_alias`]) map to short special scalars.
    pub(crate) english_enabled: bool,
    /// Lexical scalar names (`my`/`our`/`foreach`/`given`/`match`/`try` catch) per scope frame (parallel to [`Scope`] depth).
    english_lexical_scalars: Vec<HashSet<String>>,
    /// When false, the bytecode VM runs without Cranelift (see [`crate::try_vm_execute`]). Disabled by
    /// `PERLRS_NO_JIT=1` / `true` / `yes`, or `pe --no-jit` after [`Self::new`].
    pub vm_jit_enabled: bool,
    /// When true, [`crate::try_vm_execute`] prints bytecode disassembly to stderr before running the VM.
    pub disasm_bytecode: bool,
}

/// Snapshot of stash + `@ISA` for REPL `$obj->method` tab-completion (no `Interpreter` handle needed).
#[derive(Debug, Clone)]
pub struct ReplCompletionSnapshot {
    pub subs: Vec<String>,
    pub blessed_scalars: HashMap<String, String>,
    pub isa_for_class: HashMap<String, Vec<String>>,
}

impl Default for ReplCompletionSnapshot {
    fn default() -> Self {
        Self {
            subs: Vec::new(),
            blessed_scalars: HashMap::new(),
            isa_for_class: HashMap::new(),
        }
    }
}

impl ReplCompletionSnapshot {
    /// Method names (short names) visible for `class->` from [`Self::subs`] and C3 MRO.
    pub fn methods_for_class(&self, class: &str) -> Vec<String> {
        let parents = |c: &str| self.isa_for_class.get(c).cloned().unwrap_or_default();
        let mro = linearize_c3(class, &parents, 0);
        let mut names = HashSet::new();
        for pkg in &mro {
            if pkg == "UNIVERSAL" {
                continue;
            }
            let prefix = format!("{}::", pkg);
            for k in &self.subs {
                if k.starts_with(&prefix) {
                    let rest = &k[prefix.len()..];
                    if !rest.contains("::") {
                        names.insert(rest.to_string());
                    }
                }
            }
        }
        for k in &self.subs {
            if let Some(rest) = k.strip_prefix("UNIVERSAL::") {
                if !rest.contains("::") {
                    names.insert(rest.to_string());
                }
            }
        }
        let mut v: Vec<String> = names.into_iter().collect();
        v.sort();
        v
    }
}

fn repl_resolve_class_for_arrow(state: &ReplCompletionSnapshot, left: &str) -> Option<String> {
    let left = left.trim_end();
    if left.is_empty() {
        return None;
    }
    if let Some(i) = left.rfind('$') {
        let name = left[i + 1..].trim();
        if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty() {
            return state.blessed_scalars.get(name).cloned();
        }
    }
    let tok = left.split_whitespace().last()?;
    if tok.contains("::") {
        return Some(tok.to_string());
    }
    if tok.chars().all(|c| c.is_alphanumeric() || c == '_') && !tok.starts_with('$') {
        return Some(tok.to_string());
    }
    None
}

/// Tab-complete method name after `->` when the invocant resolves to a class (see [`ReplCompletionSnapshot`]).
pub fn repl_arrow_method_completions(
    state: &ReplCompletionSnapshot,
    line: &str,
    pos: usize,
) -> Option<(usize, Vec<String>)> {
    let pos = pos.min(line.len());
    let before = &line[..pos];
    let arrow_idx = before.rfind("->")?;
    let after_arrow = &before[arrow_idx + 2..];
    let rest = after_arrow.trim_start();
    let ws_len = after_arrow.len() - rest.len();
    let method_start = arrow_idx + 2 + ws_len;
    let method_prefix = &line[method_start..pos];
    if !method_prefix
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    let left = line[..arrow_idx].trim_end();
    let class = repl_resolve_class_for_arrow(state, left)?;
    let mut methods = state.methods_for_class(&class);
    methods.retain(|m| m.starts_with(method_prefix));
    Some((method_start, methods))
}

/// `Exporter`-style lists for `use Module` / `use Module qw(...)`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ModuleExportLists {
    /// Default imports for `use Module` with no list.
    pub export: Vec<String>,
    /// Extra symbols allowed in `use Module qw(name)`.
    pub export_ok: Vec<String>,
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

/// Perl-style `$^O`: map Rust [`std::env::consts::OS`] to common Perl names (`linux`, `darwin`, `MSWin32`, …).
pub(crate) fn perl_osname() -> String {
    match std::env::consts::OS {
        "linux" => "linux".to_string(),
        "macos" => "darwin".to_string(),
        "windows" => "MSWin32".to_string(),
        other => other.to_string(),
    }
}

fn perl_version_v_string() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

fn extended_os_error_string() -> String {
    std::io::Error::last_os_error().to_string()
}

#[cfg(unix)]
fn unix_real_effective_ids() -> (i64, i64, i64, i64) {
    unsafe {
        (
            libc::getuid() as i64,
            libc::geteuid() as i64,
            libc::getgid() as i64,
            libc::getegid() as i64,
        )
    }
}

#[cfg(not(unix))]
fn unix_real_effective_ids() -> (i64, i64, i64, i64) {
    (0, 0, 0, 0)
}

fn unix_id_for_special(name: &str) -> i64 {
    let (r, e, _, _) = unix_real_effective_ids();
    match name {
        "<" => r,
        ">" => e,
        _ => 0,
    }
}

#[cfg(unix)]
fn unix_group_list_string(primary: libc::gid_t) -> String {
    let mut buf = vec![0 as libc::gid_t; 256];
    let n = unsafe { libc::getgroups(256, buf.as_mut_ptr()) };
    if n <= 0 {
        return format!("{}", primary);
    }
    let mut parts = vec![format!("{}", primary)];
    for g in buf.iter().take(n as usize) {
        parts.push(format!("{}", g));
    }
    parts.join(" ")
}

/// Perl `$(` / `$)` — space-separated group id list (real / effective set).
#[cfg(unix)]
fn unix_group_list_for_special(name: &str) -> String {
    let (_, _, gid, egid) = unix_real_effective_ids();
    match name {
        "(" => unix_group_list_string(gid as libc::gid_t),
        ")" => unix_group_list_string(egid as libc::gid_t),
        _ => String::new(),
    }
}

#[cfg(not(unix))]
fn unix_group_list_for_special(_name: &str) -> String {
    String::new()
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

/// How [`Interpreter::apply_regex_captures`] updates `@^CAPTURE_ALL`.
#[derive(Clone, Copy)]
pub(crate) enum CaptureAllMode {
    /// Non-`g` match: clear `@^CAPTURE_ALL` (matches Perl 5.42+ empty `@^CAPTURE_ALL` when not using `/g`).
    Empty,
    /// Scalar-context `m//g`: append one row (numbered groups) per successful iteration.
    Append,
    /// List `m//g` / `s///g` with rows already stored — do not overwrite `@^CAPTURE_ALL`.
    Skip,
}

impl Interpreter {
    pub fn new() -> Self {
        let mut scope = Scope::new();
        scope.declare_array("INC", vec![PerlValue::string(".".to_string())]);
        scope.declare_hash("INC", IndexMap::new());
        scope.declare_array("ARGV", vec![]);
        scope.declare_array("_", vec![]);
        scope.declare_hash("ENV", IndexMap::new());
        scope.declare_hash("SIG", IndexMap::new());
        scope.declare_array("-", vec![]);
        scope.declare_array("+", vec![]);
        scope.declare_array("^CAPTURE", vec![]);
        scope.declare_array("^CAPTURE_ALL", vec![]);
        scope.declare_hash("^HOOK", IndexMap::new());
        scope.declare_scalar("~", PerlValue::string("STDOUT".to_string()));

        let script_start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let executable_path = cached_executable_path();

        let mut special_caret_scalars: HashMap<String, PerlValue> = HashMap::new();
        for name in crate::special_vars::PERL5_DOCUMENTED_CARET_NAMES {
            special_caret_scalars.insert(format!("^{}", name), PerlValue::UNDEF);
        }

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
            errno_code: 0,
            eval_error: String::new(),
            eval_error_code: 0,
            argv: Vec::new(),
            env: IndexMap::new(),
            env_materialized: false,
            program_name: "perlrs".to_string(),
            line_number: 0,
            last_readline_handle: String::new(),
            handle_line_numbers: HashMap::new(),
            sigint_pending_caret: Cell::new(false),
            auto_split: false,
            field_separator: None,
            begin_blocks: Vec::new(),
            unit_check_blocks: Vec::new(),
            check_blocks: Vec::new(),
            init_blocks: Vec::new(),
            end_blocks: Vec::new(),
            warnings: false,
            output_autoflush: false,
            suppress_stdout: false,
            child_exit_status: 0,
            last_match: String::new(),
            prematch: String::new(),
            postmatch: String::new(),
            last_paren_match: String::new(),
            list_separator: " ".to_string(),
            script_start_time,
            compile_hints: 0,
            warning_bits: 0,
            global_phase: "RUN".to_string(),
            subscript_sep: "\x1c".to_string(),
            inplace_edit: String::new(),
            debug_flags: 0,
            perl_debug_flags: 0,
            eval_nesting: 0,
            argv_current_file: String::new(),
            diamond_next_idx: 0,
            diamond_reader: None,
            strict_refs: false,
            strict_subs: false,
            strict_vars: false,
            utf8_pragma: false,
            // Like Perl 5.10+, `say` is enabled by default; `no feature 'say'` disables it.
            feature_bits: FEAT_SAY,
            num_threads: 0, // lazily read from rayon on first parallel op
            regex_cache: HashMap::new(),
            regex_last: None,
            regex_pos: HashMap::new(),
            rand_rng: StdRng::seed_from_u64(fast_rng_seed()),
            dir_handles: HashMap::new(),
            io_file_slots: HashMap::new(),
            pipe_children: HashMap::new(),
            socket_handles: HashMap::new(),
            wantarray_kind: WantarrayCtx::Scalar,
            profiler: None,
            module_export_lists: HashMap::new(),
            tied_hashes: HashMap::new(),
            tied_scalars: HashMap::new(),
            tied_arrays: HashMap::new(),
            overload_table: HashMap::new(),
            format_templates: HashMap::new(),
            special_caret_scalars,
            format_page_number: 0,
            format_lines_per_page: 60,
            format_lines_left: 0,
            format_line_break_chars: "\n".to_string(),
            format_top_name: String::new(),
            accumulator_format: String::new(),
            max_system_fd: 2,
            emergency_memory: String::new(),
            last_subpattern_name: String::new(),
            inc_hook_index: 0,
            multiline_match: false,
            executable_path,
            formfeed_string: "\x0c".to_string(),
            glob_handle_alias: HashMap::new(),
            glob_restore_frames: vec![Vec::new()],
            english_enabled: false,
            english_lexical_scalars: vec![HashSet::new()],
            vm_jit_enabled: !matches!(
                std::env::var("PERLRS_NO_JIT"),
                Ok(v)
                    if v == "1"
                        || v.eq_ignore_ascii_case("true")
                        || v.eq_ignore_ascii_case("yes")
            ),
            disasm_bytecode: false,
        }
    }

    /// Fork interpreter state for `-n`/`-p` over multiple `@ARGV` files in parallel (rayon).
    /// Clears file descriptors and I/O handles (each worker only runs the line loop).
    pub fn line_mode_worker_clone(&self) -> Interpreter {
        Interpreter {
            scope: self.scope.clone(),
            subs: self.subs.clone(),
            struct_defs: self.struct_defs.clone(),
            file: self.file.clone(),
            output_handles: HashMap::new(),
            input_handles: HashMap::new(),
            ofs: self.ofs.clone(),
            ors: self.ors.clone(),
            irs: self.irs.clone(),
            errno: self.errno.clone(),
            errno_code: self.errno_code,
            eval_error: self.eval_error.clone(),
            eval_error_code: self.eval_error_code,
            argv: self.argv.clone(),
            env: self.env.clone(),
            env_materialized: self.env_materialized,
            program_name: self.program_name.clone(),
            line_number: 0,
            last_readline_handle: String::new(),
            handle_line_numbers: HashMap::new(),
            sigint_pending_caret: Cell::new(false),
            auto_split: self.auto_split,
            field_separator: self.field_separator.clone(),
            begin_blocks: self.begin_blocks.clone(),
            unit_check_blocks: self.unit_check_blocks.clone(),
            check_blocks: self.check_blocks.clone(),
            init_blocks: self.init_blocks.clone(),
            end_blocks: self.end_blocks.clone(),
            warnings: self.warnings,
            output_autoflush: self.output_autoflush,
            suppress_stdout: self.suppress_stdout,
            child_exit_status: self.child_exit_status,
            last_match: self.last_match.clone(),
            prematch: self.prematch.clone(),
            postmatch: self.postmatch.clone(),
            last_paren_match: self.last_paren_match.clone(),
            list_separator: self.list_separator.clone(),
            script_start_time: self.script_start_time,
            compile_hints: self.compile_hints,
            warning_bits: self.warning_bits,
            global_phase: self.global_phase.clone(),
            subscript_sep: self.subscript_sep.clone(),
            inplace_edit: self.inplace_edit.clone(),
            debug_flags: self.debug_flags,
            perl_debug_flags: self.perl_debug_flags,
            eval_nesting: self.eval_nesting,
            argv_current_file: String::new(),
            diamond_next_idx: 0,
            diamond_reader: None,
            strict_refs: self.strict_refs,
            strict_subs: self.strict_subs,
            strict_vars: self.strict_vars,
            utf8_pragma: self.utf8_pragma,
            feature_bits: self.feature_bits,
            num_threads: 0,
            regex_cache: self.regex_cache.clone(),
            regex_last: self.regex_last.clone(),
            regex_pos: self.regex_pos.clone(),
            rand_rng: self.rand_rng.clone(),
            dir_handles: HashMap::new(),
            io_file_slots: HashMap::new(),
            pipe_children: HashMap::new(),
            socket_handles: HashMap::new(),
            wantarray_kind: self.wantarray_kind,
            profiler: None,
            module_export_lists: self.module_export_lists.clone(),
            tied_hashes: self.tied_hashes.clone(),
            tied_scalars: self.tied_scalars.clone(),
            tied_arrays: self.tied_arrays.clone(),
            overload_table: self.overload_table.clone(),
            format_templates: self.format_templates.clone(),
            special_caret_scalars: self.special_caret_scalars.clone(),
            format_page_number: self.format_page_number,
            format_lines_per_page: self.format_lines_per_page,
            format_lines_left: self.format_lines_left,
            format_line_break_chars: self.format_line_break_chars.clone(),
            format_top_name: self.format_top_name.clone(),
            accumulator_format: self.accumulator_format.clone(),
            max_system_fd: self.max_system_fd,
            emergency_memory: self.emergency_memory.clone(),
            last_subpattern_name: self.last_subpattern_name.clone(),
            inc_hook_index: self.inc_hook_index,
            multiline_match: self.multiline_match,
            executable_path: self.executable_path.clone(),
            formfeed_string: self.formfeed_string.clone(),
            glob_handle_alias: self.glob_handle_alias.clone(),
            glob_restore_frames: self.glob_restore_frames.clone(),
            english_enabled: self.english_enabled,
            english_lexical_scalars: self.english_lexical_scalars.clone(),
            vm_jit_enabled: self.vm_jit_enabled,
            disasm_bytecode: self.disasm_bytecode,
        }
    }

    /// Rayon pool size (`pe -j`); lazily initialized from `rayon::current_num_threads()`.
    pub(crate) fn parallel_thread_count(&mut self) -> usize {
        if self.num_threads == 0 {
            self.num_threads = rayon::current_num_threads();
        }
        self.num_threads
    }

    fn encode_exit_status(&self, s: std::process::ExitStatus) -> i64 {
        #[cfg(unix)]
        if let Some(sig) = s.signal() {
            return sig as i64 & 0x7f;
        }
        let code = s.code().unwrap_or(0) as i64;
        code << 8
    }

    pub(crate) fn record_child_exit_status(&mut self, s: std::process::ExitStatus) {
        self.child_exit_status = self.encode_exit_status(s);
    }

    /// Update `$!` / `errno_code` from a [`std::io::Error`] (dualvar numeric + string).
    pub(crate) fn apply_io_error_to_errno(&mut self, e: &std::io::Error) {
        self.errno = e.to_string();
        self.errno_code = e.raw_os_error().unwrap_or(0);
    }

    /// Set `$@` message; numeric side is `0` if empty, else `1`.
    pub(crate) fn set_eval_error(&mut self, msg: String) {
        self.eval_error = msg;
        self.eval_error_code = if self.eval_error.is_empty() { 0 } else { 1 };
    }

    pub(crate) fn clear_eval_error(&mut self) {
        self.eval_error = String::new();
        self.eval_error_code = 0;
    }

    /// Advance `$.` bookkeeping for the handle that produced the last `readline` line.
    fn bump_line_for_handle(&mut self, handle_key: &str) {
        self.last_readline_handle = handle_key.to_string();
        *self
            .handle_line_numbers
            .entry(handle_key.to_string())
            .or_insert(0) += 1;
    }

    /// `@ISA` / `@EXPORT` storage uses `Pkg::NAME` outside `main`.
    pub(crate) fn stash_array_name_for_package(&self, name: &str) -> String {
        if name.starts_with('^') {
            return name.to_string();
        }
        if matches!(name, "ISA" | "EXPORT" | "EXPORT_OK") {
            let pkg = self.current_package();
            if !pkg.is_empty() && pkg != "main" {
                return format!("{}::{}", pkg, name);
            }
        }
        name.to_string()
    }

    pub(crate) fn parents_of_class(&self, class: &str) -> Vec<String> {
        let key = format!("{}::ISA", class);
        self.scope
            .get_array(&key)
            .into_iter()
            .map(|v| v.to_string())
            .collect()
    }

    fn mro_linearize(&self, class: &str) -> Vec<String> {
        let p = |c: &str| self.parents_of_class(c);
        linearize_c3(class, &p, 0)
    }

    /// Returns fully qualified sub name for [`Self::subs`], or a candidate for [`Self::try_autoload_call`].
    pub(crate) fn resolve_method_full_name(
        &self,
        invocant_class: &str,
        method: &str,
        super_mode: bool,
    ) -> Option<String> {
        let mro = self.mro_linearize(invocant_class);
        // SUPER:: — skip the invocant's class in C3 order (same as Perl: start at the parent of
        // the blessed class). Do not use `__PACKAGE__` here: it may be `main` after `package main`
        // even when running `C::meth`.
        let start = if super_mode {
            mro.iter()
                .position(|p| p == invocant_class)
                .map(|i| i + 1)
                // If the class string does not appear in MRO (should be rare), skip the first
                // entry so we still search parents before giving up.
                .unwrap_or(1)
        } else {
            0
        };
        for pkg in mro.iter().skip(start) {
            if pkg == "UNIVERSAL" {
                continue;
            }
            let fq = format!("{}::{}", pkg, method);
            if self.subs.contains_key(&fq) {
                return Some(fq);
            }
        }
        mro.iter()
            .skip(start)
            .find(|p| *p != "UNIVERSAL")
            .map(|pkg| format!("{}::{}", pkg, method))
    }

    pub(crate) fn resolve_io_handle_name(&self, name: &str) -> String {
        self.glob_handle_alias
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// Stash key for `sub name` / `&name` when `name` is a typeglob basename (`*foo`, `*Pkg::foo`).
    pub(crate) fn qualify_typeglob_sub_key(&self, name: &str) -> String {
        if name.contains("::") {
            name.to_string()
        } else {
            self.qualify_sub_key(name)
        }
    }

    /// `*lhs = *rhs` — copy subroutine, scalar, array, hash, and IO-handle alias slots (Perl-style).
    pub(crate) fn copy_typeglob_slots(&mut self, lhs: &str, rhs: &str, line: usize) -> PerlResult<()> {
        let lhs_sub = self.qualify_typeglob_sub_key(lhs);
        let rhs_sub = self.qualify_typeglob_sub_key(rhs);
        match self.subs.get(&rhs_sub).cloned() {
            Some(s) => {
                self.subs.insert(lhs_sub, s);
            }
            None => {
                self.subs.remove(&lhs_sub);
            }
        }
        let sv = self.scope.get_scalar(rhs);
        self.scope
            .set_scalar(lhs, sv.clone())
            .map_err(|e| e.at_line(line))?;
        let lhs_an = self.stash_array_name_for_package(lhs);
        let rhs_an = self.stash_array_name_for_package(rhs);
        let av = self.scope.get_array(&rhs_an);
        self.scope
            .set_array(&lhs_an, av.clone())
            .map_err(|e| e.at_line(line))?;
        let hv = self.scope.get_hash(rhs);
        self.scope
            .set_hash(lhs, hv.clone())
            .map_err(|e| e.at_line(line))?;
        match self.glob_handle_alias.get(rhs).cloned() {
            Some(t) => {
                self.glob_handle_alias.insert(lhs.to_string(), t);
            }
            None => {
                self.glob_handle_alias.remove(lhs);
            }
        }
        Ok(())
    }

    pub(crate) fn scope_push_hook(&mut self) {
        self.scope.push_frame();
        self.glob_restore_frames.push(Vec::new());
        self.english_lexical_scalars.push(HashSet::new());
    }

    #[inline]
    pub(crate) fn english_note_lexical_scalar(&mut self, name: &str) {
        if let Some(s) = self.english_lexical_scalars.last_mut() {
            s.insert(name.to_string());
        }
    }

    pub(crate) fn scope_pop_hook(&mut self) {
        if !self.scope.can_pop_frame() {
            return;
        }
        if let Some(entries) = self.glob_restore_frames.pop() {
            for (name, old) in entries.into_iter().rev() {
                match old {
                    Some(s) => {
                        self.glob_handle_alias.insert(name, s);
                    }
                    None => {
                        self.glob_handle_alias.remove(&name);
                    }
                }
            }
        }
        self.scope.pop_frame();
        let _ = self.english_lexical_scalars.pop();
    }

    /// After [`Scope::restore_capture`] / [`Scope::restore_atomics`] on a parallel or async worker,
    /// reject writes to non-`mysync` outer captured lexicals (block locals use `scope_push_hook`).
    #[inline]
    pub(crate) fn enable_parallel_guard(&mut self) {
        self.scope.set_parallel_guard(true);
    }

    /// BEGIN/END are lowered into the VM chunk; clear interpreter queues so a later tree-walker
    /// run does not execute them again.
    pub(crate) fn clear_begin_end_blocks_after_vm_compile(&mut self) {
        self.begin_blocks.clear();
        self.unit_check_blocks.clear();
        self.check_blocks.clear();
        self.init_blocks.clear();
        self.end_blocks.clear();
    }

    /// Pop scope frames until [`Scope::depth`] == `target_depth`, running [`Self::scope_pop_hook`]
    /// each time so `glob_restore_frames` / `english_lexical_scalars` stay aligned with
    /// [`Self::scope_push_hook`]. The bytecode VM must use this after [`Op::Call`] /
    /// [`Op::PushFrame`] (which call `scope_push_hook`); [`Scope::pop_to_depth`] alone is wrong
    /// there because it only calls [`Scope::pop_frame`].
    pub(crate) fn pop_scope_to_depth(&mut self, target_depth: usize) {
        while self.scope.depth() > target_depth && self.scope.can_pop_frame() {
            self.scope_pop_hook();
        }
    }

    /// `%SIG` hook — code refs run between statements (`perl_signal` module).
    pub(crate) fn invoke_sig_handler(&mut self, sig: &str) -> PerlResult<()> {
        self.touch_env_hash("SIG");
        let v = self.scope.get_hash_element("SIG", sig);
        if v.is_undef() {
            return Ok(());
        }
        if let Some(s) = v.as_str() {
            if s == "IGNORE" || s == "DEFAULT" {
                return Ok(());
            }
        }
        if let Some(sub) = v.as_code_ref() {
            match self.call_sub(&sub, vec![], WantarrayCtx::Scalar, 0) {
                Ok(_) => Ok(()),
                Err(FlowOrError::Flow(_)) => Ok(()),
                Err(FlowOrError::Error(e)) => Err(e),
            }
        } else {
            Ok(())
        }
    }

    /// Populate [`Self::env`] and the `%ENV` hash from [`std::env::vars`] once.
    /// Deferred from [`Self::new`] to reduce interpreter startup when `%ENV` is unused.
    pub fn materialize_env_if_needed(&mut self) {
        if self.env_materialized {
            return;
        }
        self.env = std::env::vars()
            .map(|(k, v)| (k, PerlValue::string(v)))
            .collect();
        self.scope
            .set_hash("ENV", self.env.clone())
            .expect("set %ENV");
        self.env_materialized = true;
    }

    #[inline]
    pub(crate) fn touch_env_hash(&mut self, hash_name: &str) {
        if hash_name == "ENV" {
            self.materialize_env_if_needed();
        }
    }

    /// `exists $href->{k}` / `exists $obj->{k}` — container is a hash ref or blessed hash-like value.
    fn exists_arrow_hash_element(
        &self,
        container: PerlValue,
        key: &str,
        line: usize,
    ) -> PerlResult<bool> {
        if let Some(r) = container.as_hash_ref() {
            return Ok(r.read().contains_key(key));
        }
        if let Some(b) = container.as_blessed_ref() {
            let data = b.data.read();
            if let Some(r) = data.as_hash_ref() {
                return Ok(r.read().contains_key(key));
            }
            if let Some(hm) = data.as_hash_map() {
                return Ok(hm.contains_key(key));
            }
            return Err(PerlError::runtime(
                "exists argument is not a HASH reference",
                line,
            ));
        }
        Err(PerlError::runtime(
            "exists argument is not a HASH reference",
            line,
        ))
    }

    /// `delete $href->{k}` / `delete $obj->{k}` — same container rules as [`Self::exists_arrow_hash_element`].
    fn delete_arrow_hash_element(
        &self,
        container: PerlValue,
        key: &str,
        line: usize,
    ) -> PerlResult<PerlValue> {
        if let Some(r) = container.as_hash_ref() {
            return Ok(r.write().shift_remove(key).unwrap_or(PerlValue::UNDEF));
        }
        if let Some(b) = container.as_blessed_ref() {
            let mut data = b.data.write();
            if let Some(r) = data.as_hash_ref() {
                return Ok(r.write().shift_remove(key).unwrap_or(PerlValue::UNDEF));
            }
            if let Some(mut map) = data.as_hash_map() {
                let v = map.shift_remove(key).unwrap_or(PerlValue::UNDEF);
                *data = PerlValue::hash(map);
                return Ok(v);
            }
            return Err(PerlError::runtime(
                "delete argument is not a HASH reference",
                line,
            ));
        }
        Err(PerlError::runtime(
            "delete argument is not a HASH reference",
            line,
        ))
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
            "_" | "0"
                | "!"
                | "@"
                | "/"
                | "\\"
                | ","
                | "."
                | "__PACKAGE__"
                | "$$"
                | "|"
                | "?"
                | "\""
                | "&"
                | "`"
                | "'"
                | "+"
                | "<"
                | ">"
                | "("
                | ")"
                | "]"
                | ";"
                | "ARGV"
                | "%"
                | "="
                | "-"
                | ":"
                | "*"
                | "INC"
        ) || name.chars().all(|c| c.is_ascii_digit())
            || name.starts_with('^')
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
        } else if t.ends_with(".pm") || t.ends_with(".pl") || t.contains('/') {
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

    fn import_all_from_module(&mut self, module: &str, line: usize) -> PerlResult<()> {
        if module == "List::Util" {
            crate::list_util::ensure_list_util(self);
        }
        if let Some(lists) = self.module_export_lists.get(module) {
            let export: Vec<String> = lists.export.clone();
            for short in export {
                self.import_named_sub(module, &short, line)?;
            }
            return Ok(());
        }
        // No `our @EXPORT` recorded (legacy): import every top-level sub in the package.
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

    /// Copy `Module::name` into the caller stash (`name` must exist as a sub).
    fn import_named_sub(&mut self, module: &str, short: &str, line: usize) -> PerlResult<()> {
        if module == "List::Util" {
            crate::list_util::ensure_list_util(self);
        }
        let qual = format!("{}::{}", module, short);
        let sub = self.subs.get(&qual).cloned().ok_or_else(|| {
            PerlError::runtime(
                format!(
                    "`{}` is not defined in module `{}` (expected `{}`)",
                    short, module, qual
                ),
                line,
            )
        })?;
        let alias = self.import_alias_key(short);
        self.subs.insert(alias, sub);
        Ok(())
    }

    fn import_one_symbol(&mut self, module: &str, export: &str, line: usize) -> PerlResult<()> {
        if let Some(lists) = self.module_export_lists.get(module) {
            let allowed: HashSet<&str> = lists
                .export
                .iter()
                .map(|s| s.as_str())
                .chain(lists.export_ok.iter().map(|s| s.as_str()))
                .collect();
            if !allowed.contains(export) {
                return Err(PerlError::runtime(
                    format!(
                        "`{}` is not exported by `{}` (not in @EXPORT or @EXPORT_OK)",
                        export, module
                    ),
                    line,
                ));
            }
        }
        self.import_named_sub(module, export, line)
    }

    /// After `our @EXPORT` / `our @EXPORT_OK` in a package, record lists for `use`.
    fn record_exporter_our_array_name(&mut self, name: &str, items: &[PerlValue]) {
        if name != "EXPORT" && name != "EXPORT_OK" {
            return;
        }
        let pkg = self.current_package();
        if pkg.is_empty() || pkg == "main" {
            return;
        }
        let names: Vec<String> = items.iter().map(|v| v.to_string()).collect();
        let ent = self.module_export_lists.entry(pkg).or_default();
        if name == "EXPORT" {
            ent.export = names;
        } else {
            ent.export_ok = names;
        }
    }

    /// Resolve `foo` or `Foo::bar` against the subroutine stash (package-aware).
    /// Refresh [`PerlSub::closure_env`] for `name` from [`Scope::capture`] at the current stack
    /// (top-level `sub` at runtime and [`Op::BindSubClosure`] after preceding `my`/etc.).
    pub(crate) fn rebind_sub_closure(&mut self, name: &str) {
        let key = self.qualify_sub_key(name);
        let Some(sub) = self.subs.get(&key).cloned() else {
            return;
        };
        let captured = self.scope.capture();
        let closure_env = if captured.is_empty() {
            None
        } else {
            Some(captured)
        };
        let mut new_sub = (*sub).clone();
        new_sub.closure_env = closure_env;
        new_sub.fib_like = crate::fib_like_tail::detect_fib_like_recursive_add(&new_sub);
        self.subs.insert(key, Arc::new(new_sub));
    }

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
                return Ok(PerlValue::integer(1));
            }
            "utf8" => {
                self.utf8_pragma = true;
                return Ok(PerlValue::integer(1));
            }
            "feature" | "v5" => {
                return Ok(PerlValue::integer(1));
            }
            "warnings" => {
                self.warnings = true;
                return Ok(PerlValue::integer(1));
            }
            "threads" | "Thread::Pool" | "Parallel::ForkManager" => {
                return Ok(PerlValue::integer(1));
            }
            _ => {}
        }
        let p = Path::new(t);
        if p.is_absolute() {
            return self.require_absolute_path(p, line);
        }
        if Self::looks_like_version_only(t) {
            return Ok(PerlValue::integer(1));
        }
        let relpath = Self::module_spec_to_relpath(t);
        self.require_from_inc(&relpath, line)
    }

    /// `%^HOOK` entries `require__before` / `require__after` (Perl 5.37+): coderef `(filename)`.
    fn invoke_require_hook(&mut self, key: &str, path: &str, line: usize) -> PerlResult<()> {
        let v = self.scope.get_hash_element("^HOOK", key);
        if v.is_undef() {
            return Ok(());
        }
        let Some(sub) = v.as_code_ref() else {
            return Ok(());
        };
        let r = self.call_sub(
            sub.as_ref(),
            vec![PerlValue::string(path.to_string())],
            WantarrayCtx::Scalar,
            line,
        );
        match r {
            Ok(_) => Ok(()),
            Err(FlowOrError::Error(e)) => Err(e),
            Err(FlowOrError::Flow(Flow::Return(_))) => Ok(()),
            Err(FlowOrError::Flow(other)) => Err(PerlError::runtime(
                format!(
                    "require hook {:?} returned unexpected control flow: {:?}",
                    key, other
                ),
                line,
            )),
        }
    }

    fn require_absolute_path(&mut self, path: &Path, line: usize) -> PerlResult<PerlValue> {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let key = canon.to_string_lossy().into_owned();
        if self.scope.exists_hash_element("INC", &key) {
            return Ok(PerlValue::integer(1));
        }
        self.invoke_require_hook("require__before", &key, line)?;
        let code = std::fs::read_to_string(&canon).map_err(|e| {
            PerlError::runtime(
                format!("Can't open {} for reading: {}", canon.display(), e),
                line,
            )
        })?;
        self.scope
            .set_hash_element("INC", &key, PerlValue::string(key.clone()))?;
        let saved_pkg = self.scope.get_scalar("__PACKAGE__");
        let r = crate::parse_and_run_string(&code, self);
        let _ = self.scope.set_scalar("__PACKAGE__", saved_pkg);
        r?;
        self.invoke_require_hook("require__after", &key, line)?;
        Ok(PerlValue::integer(1))
    }

    fn require_from_inc(&mut self, relpath: &str, line: usize) -> PerlResult<PerlValue> {
        if self.scope.exists_hash_element("INC", relpath) {
            return Ok(PerlValue::integer(1));
        }
        self.invoke_require_hook("require__before", relpath, line)?;
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
                    .set_hash_element("INC", relpath, PerlValue::string(abs_s))?;
                let saved_pkg = self.scope.get_scalar("__PACKAGE__");
                let r = crate::parse_and_run_string(&code, self);
                let _ = self.scope.set_scalar("__PACKAGE__", saved_pkg);
                r?;
                self.invoke_require_hook("require__after", relpath, line)?;
                return Ok(PerlValue::integer(1));
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
            "English" => {
                self.english_enabled = true;
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
            "English" => {
                self.english_enabled = false;
                Ok(())
            }
            "threads" | "Thread::Pool" | "Parallel::ForkManager" => Ok(()),
            _ => Ok(()),
        }
    }

    /// Register subs, run `use` in source order, collect `BEGIN`/`END` (before `BEGIN` execution).
    pub(crate) fn prepare_program_top_level(&mut self, program: &Program) -> PerlResult<()> {
        if crate::list_util::program_needs_list_util(program) {
            crate::list_util::ensure_list_util(self);
        }
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::Package { name } => {
                    let _ = self
                        .scope
                        .set_scalar("__PACKAGE__", PerlValue::string(name.clone()));
                }
                StmtKind::SubDecl {
                    name,
                    params,
                    body,
                    prototype,
                } => {
                    let key = self.qualify_sub_key(name);
                    let mut sub = PerlSub {
                        name: name.clone(),
                        params: params.clone(),
                        body: body.clone(),
                        closure_env: None,
                        prototype: prototype.clone(),
                        fib_like: None,
                    };
                    sub.fib_like = crate::fib_like_tail::detect_fib_like_recursive_add(&sub);
                    self.subs.insert(key, Arc::new(sub));
                }
                StmtKind::Use { module, imports } => {
                    self.exec_use_stmt(module, imports, stmt.line)?;
                }
                StmtKind::UseOverload { pairs } => {
                    let pkg = self.current_package();
                    let ent = self.overload_table.entry(pkg).or_default();
                    for (k, v) in pairs {
                        ent.insert(k.clone(), v.clone());
                    }
                }
                StmtKind::FormatDecl { name, lines } => {
                    let pkg = self.current_package();
                    let key = format!("{}::{}", pkg, name);
                    let tmpl = crate::format::parse_format_template(lines)?;
                    self.format_templates.insert(key, Arc::new(tmpl));
                }
                StmtKind::No { module, imports } => {
                    self.exec_no_stmt(module, imports, stmt.line)?;
                }
                StmtKind::Begin(block) => self.begin_blocks.push(block.clone()),
                StmtKind::UnitCheck(block) => self.unit_check_blocks.push(block.clone()),
                StmtKind::Check(block) => self.check_blocks.push(block.clone()),
                StmtKind::Init(block) => self.init_blocks.push(block.clone()),
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
        let handle_return = handle_name.clone();
        match actual_mode.as_str() {
            "-|" => {
                let mut cmd = piped_shell_command(&path);
                cmd.stdout(Stdio::piped());
                let mut child = cmd.spawn().map_err(|e| {
                    self.apply_io_error_to_errno(&e);
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
                    self.apply_io_error_to_errno(&e);
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
                    self.apply_io_error_to_errno(&e);
                    PerlError::runtime(format!("Can't open '{}': {}", path, e), line)
                })?;
                if let Ok(raw) = file.try_clone() {
                    self.io_file_slots.insert(handle_name.clone(), raw);
                }
                self.input_handles
                    .insert(handle_name.clone(), BufReader::new(Box::new(file)));
            }
            ">" => {
                let file = std::fs::File::create(&path).map_err(|e| {
                    self.apply_io_error_to_errno(&e);
                    PerlError::runtime(format!("Can't open '{}': {}", path, e), line)
                })?;
                if let Ok(raw) = file.try_clone() {
                    self.io_file_slots.insert(handle_name.clone(), raw);
                }
                self.output_handles
                    .insert(handle_name.clone(), Box::new(file));
            }
            ">>" => {
                let file = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&path)
                    .map_err(|e| {
                        self.apply_io_error_to_errno(&e);
                        PerlError::runtime(format!("Can't open '{}': {}", path, e), line)
                    })?;
                if let Ok(raw) = file.try_clone() {
                    self.io_file_slots.insert(handle_name.clone(), raw);
                }
                self.output_handles
                    .insert(handle_name.clone(), Box::new(file));
            }
            _ => {
                return Err(PerlError::runtime(
                    format!("Unknown open mode '{}'", actual_mode),
                    line,
                ));
            }
        }
        Ok(PerlValue::io_handle(handle_return))
    }

    pub(crate) fn close_builtin_execute(&mut self, name: String) -> PerlResult<PerlValue> {
        self.output_handles.remove(&name);
        self.input_handles.remove(&name);
        self.io_file_slots.remove(&name);
        if let Some(mut child) = self.pipe_children.remove(&name) {
            if let Ok(st) = child.wait() {
                self.record_child_exit_status(st);
            }
        }
        Ok(PerlValue::integer(1))
    }

    pub(crate) fn has_input_handle(&self, name: &str) -> bool {
        self.input_handles.contains_key(name)
    }

    pub(crate) fn readline_builtin_execute(
        &mut self,
        handle: Option<&str>,
    ) -> PerlResult<PerlValue> {
        // `<>` / `readline` with no handle: iterate `@ARGV` files, else stdin.
        if handle.is_none() {
            let argv = self.scope.get_array("ARGV");
            if !argv.is_empty() {
                loop {
                    if self.diamond_reader.is_none() {
                        while self.diamond_next_idx < argv.len() {
                            let path = argv[self.diamond_next_idx].to_string();
                            self.diamond_next_idx += 1;
                            match File::open(&path) {
                                Ok(f) => {
                                    self.argv_current_file = path;
                                    self.diamond_reader = Some(BufReader::new(f));
                                    break;
                                }
                                Err(e) => {
                                    self.apply_io_error_to_errno(&e);
                                }
                            }
                        }
                        if self.diamond_reader.is_none() {
                            return Ok(PerlValue::UNDEF);
                        }
                    }
                    let mut line_str = String::new();
                    let read_result = if let Some(reader) = self.diamond_reader.as_mut() {
                        reader.read_line(&mut line_str)
                    } else {
                        unreachable!()
                    };
                    match read_result {
                        Ok(0) => {
                            self.diamond_reader = None;
                            continue;
                        }
                        Ok(_) => {
                            self.bump_line_for_handle(&self.argv_current_file.clone());
                            return Ok(PerlValue::string(line_str));
                        }
                        Err(e) => {
                            self.apply_io_error_to_errno(&e);
                            self.diamond_reader = None;
                            continue;
                        }
                    }
                }
            } else {
                self.argv_current_file.clear();
            }
        }

        let handle_name = handle.unwrap_or("STDIN");
        let mut line_str = String::new();
        if handle_name == "STDIN" {
            match io::stdin().lock().read_line(&mut line_str) {
                Ok(0) => Ok(PerlValue::UNDEF),
                Ok(_) => {
                    self.bump_line_for_handle("STDIN");
                    Ok(PerlValue::string(line_str))
                }
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    Ok(PerlValue::UNDEF)
                }
            }
        } else if let Some(reader) = self.input_handles.get_mut(handle_name) {
            match reader.read_line(&mut line_str) {
                Ok(0) => Ok(PerlValue::UNDEF),
                Ok(_) => {
                    self.bump_line_for_handle(handle_name);
                    Ok(PerlValue::string(line_str))
                }
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    Ok(PerlValue::UNDEF)
                }
            }
        } else {
            Ok(PerlValue::UNDEF)
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
                PerlValue::integer(1)
            }
            Err(e) => {
                self.apply_io_error_to_errno(&e);
                PerlValue::integer(0)
            }
        }
    }

    pub(crate) fn readdir_handle(&mut self, handle: &str) -> PerlValue {
        if let Some(dh) = self.dir_handles.get_mut(handle) {
            if dh.pos < dh.entries.len() {
                let s = dh.entries[dh.pos].clone();
                dh.pos += 1;
                PerlValue::string(s)
            } else {
                PerlValue::UNDEF
            }
        } else {
            PerlValue::UNDEF
        }
    }

    pub(crate) fn closedir_handle(&mut self, handle: &str) -> PerlValue {
        PerlValue::integer(if self.dir_handles.remove(handle).is_some() {
            1
        } else {
            0
        })
    }

    pub(crate) fn rewinddir_handle(&mut self, handle: &str) -> PerlValue {
        if let Some(dh) = self.dir_handles.get_mut(handle) {
            dh.pos = 0;
            PerlValue::integer(1)
        } else {
            PerlValue::integer(0)
        }
    }

    pub(crate) fn telldir_handle(&mut self, handle: &str) -> PerlValue {
        self.dir_handles
            .get(handle)
            .map(|dh| PerlValue::integer(dh.pos as i64))
            .unwrap_or(PerlValue::UNDEF)
    }

    pub(crate) fn seekdir_handle(&mut self, handle: &str, pos: usize) -> PerlValue {
        if let Some(dh) = self.dir_handles.get_mut(handle) {
            dh.pos = pos.min(dh.entries.len());
            PerlValue::integer(1)
        } else {
            PerlValue::integer(0)
        }
    }

    /// Set `$&`, `` $` ``, `$'`, `$+`, `$1`…`$n`, `@-`, `@+`, `%+`, and `${^MATCH}` / … fields from a successful match.
    pub(crate) fn apply_regex_captures(
        &mut self,
        haystack: &str,
        offset: usize,
        re: &PerlCompiledRegex,
        caps: &PerlCaptures<'_>,
        capture_all: CaptureAllMode,
    ) -> Result<(), FlowOrError> {
        let m0 = caps.get(0).expect("regex capture 0");
        let s0 = offset + m0.start;
        let e0 = offset + m0.end;
        self.last_match = haystack.get(s0..e0).unwrap_or("").to_string();
        self.prematch = haystack.get(..s0).unwrap_or("").to_string();
        self.postmatch = haystack.get(e0..).unwrap_or("").to_string();
        let mut last_paren = String::new();
        for i in 1..caps.len() {
            if let Some(m) = caps.get(i) {
                last_paren = m.text.to_string();
            }
        }
        self.last_paren_match = last_paren;
        self.last_subpattern_name = String::new();
        for n in re.capture_names().flatten() {
            if caps.name(n).is_some() {
                self.last_subpattern_name = n.to_string();
            }
        }
        self.scope
            .set_scalar("&", PerlValue::string(self.last_match.clone()))?;
        self.scope
            .set_scalar("`", PerlValue::string(self.prematch.clone()))?;
        self.scope
            .set_scalar("'", PerlValue::string(self.postmatch.clone()))?;
        self.scope
            .set_scalar("+", PerlValue::string(self.last_paren_match.clone()))?;
        for i in 1..caps.len() {
            if let Some(m) = caps.get(i) {
                self.scope
                    .set_scalar(&i.to_string(), PerlValue::string(m.text.to_string()))?;
            }
        }
        let mut start_arr = vec![PerlValue::integer(s0 as i64)];
        let mut end_arr = vec![PerlValue::integer(e0 as i64)];
        for i in 1..caps.len() {
            if let Some(m) = caps.get(i) {
                start_arr.push(PerlValue::integer((offset + m.start) as i64));
                end_arr.push(PerlValue::integer((offset + m.end) as i64));
            } else {
                start_arr.push(PerlValue::integer(-1));
                end_arr.push(PerlValue::integer(-1));
            }
        }
        self.scope.set_array("-", start_arr)?;
        self.scope.set_array("+", end_arr)?;
        let mut named = IndexMap::new();
        for name in re.capture_names().flatten() {
            if let Some(m) = caps.name(name) {
                named.insert(name.to_string(), PerlValue::string(m.text.to_string()));
            }
        }
        self.scope.set_hash("+", named)?;
        let cap_flat = crate::perl_regex::numbered_capture_flat(caps);
        self.scope.set_array("^CAPTURE", cap_flat.clone())?;
        match capture_all {
            CaptureAllMode::Empty => {
                self.scope.set_array("^CAPTURE_ALL", vec![])?;
            }
            CaptureAllMode::Append => {
                let mut rows = self.scope.get_array("^CAPTURE_ALL");
                rows.push(PerlValue::array(cap_flat));
                self.scope.set_array("^CAPTURE_ALL", rows)?;
            }
            CaptureAllMode::Skip => {}
        }
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
        self.assign_value(target, PerlValue::string(s))?;
        Ok(PerlValue::integer(removed))
    }

    /// Shared `chop` for tree-walker and VM (mutates `target`).
    pub(crate) fn chop_inplace_execute(&mut self, val: PerlValue, target: &Expr) -> ExecResult {
        let mut s = val.to_string();
        let chopped = s
            .pop()
            .map(|c| PerlValue::string(c.to_string()))
            .unwrap_or(PerlValue::UNDEF);
        self.assign_value(target, PerlValue::string(s))?;
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
            if start == 0 {
                self.scope.set_array("^CAPTURE_ALL", vec![])?;
            }
            if start > s.len() {
                self.regex_pos.insert(key, None);
                return Ok(PerlValue::integer(0));
            }
            let sub = s.get(start..).unwrap_or("");
            if let Some(caps) = re.captures(sub) {
                let overall = caps.get(0).expect("capture 0");
                let abs_end = start + overall.end;
                self.regex_pos.insert(key, Some(abs_end));
                self.apply_regex_captures(&s, start, &re, &caps, CaptureAllMode::Append)?;
                Ok(PerlValue::integer(1))
            } else {
                self.regex_pos.insert(key, None);
                Ok(PerlValue::integer(0))
            }
        } else if flags.contains('g') {
            let mut rows = Vec::new();
            let mut last_caps: Option<PerlCaptures<'_>> = None;
            for caps in re.captures_iter(&s) {
                rows.push(PerlValue::array(crate::perl_regex::numbered_capture_flat(
                    &caps,
                )));
                last_caps = Some(caps);
            }
            self.scope.set_array("^CAPTURE_ALL", rows)?;
            let matches: Vec<PerlValue> = match &*re {
                PerlCompiledRegex::Rust(r) => r
                    .find_iter(&s)
                    .map(|m| PerlValue::string(m.as_str().to_string()))
                    .collect(),
                PerlCompiledRegex::Fancy(r) => r
                    .find_iter(&s)
                    .filter_map(|m| m.ok())
                    .map(|m| PerlValue::string(m.as_str().to_string()))
                    .collect(),
                PerlCompiledRegex::Pcre2(r) => r
                    .find_iter(s.as_bytes())
                    .filter_map(|m| m.ok())
                    .map(|m| {
                        let t = s.get(m.start()..m.end()).unwrap_or("");
                        PerlValue::string(t.to_string())
                    })
                    .collect(),
            };
            if matches.is_empty() {
                Ok(PerlValue::integer(0))
            } else {
                if let Some(caps) = last_caps {
                    self.apply_regex_captures(&s, 0, &re, &caps, CaptureAllMode::Skip)?;
                }
                Ok(PerlValue::array(matches))
            }
        } else if let Some(caps) = re.captures(&s) {
            self.apply_regex_captures(&s, 0, &re, &caps, CaptureAllMode::Empty)?;
            Ok(PerlValue::integer(1))
        } else {
            Ok(PerlValue::integer(0))
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
            let mut rows = Vec::new();
            let mut last = None;
            for caps in re.captures_iter(&s) {
                rows.push(PerlValue::array(crate::perl_regex::numbered_capture_flat(
                    &caps,
                )));
                last = Some(caps);
            }
            self.scope.set_array("^CAPTURE_ALL", rows)?;
            last
        } else {
            re.captures(&s)
        };
        if let Some(caps) = last_caps {
            let mode = if flags.contains('g') {
                CaptureAllMode::Skip
            } else {
                CaptureAllMode::Empty
            };
            self.apply_regex_captures(&s, 0, &re, &caps, mode)?;
        }
        let (new_s, count) = if flags.contains('g') {
            let count = re.find_iter_count(&s);
            (re.replace_all(&s, replacement), count)
        } else {
            let count = if re.is_match(&s) { 1 } else { 0 };
            (re.replace(&s, replacement), count)
        };
        self.assign_value(target, PerlValue::string(new_s))?;
        Ok(PerlValue::integer(count as i64))
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
            self.assign_value(target, PerlValue::string(new_s))?;
        }
        Ok(PerlValue::integer(count))
    }

    /// `splice @array, offset, length, LIST` — used by the VM `CallBuiltin(Splice)` path.
    pub(crate) fn splice_builtin_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        if args.is_empty() {
            return Err(PerlError::runtime("splice: missing array", line));
        }
        let arr_name = args[0].to_string();
        let off = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
        let len = match args.get(2) {
            None => {
                let arr = self.scope.get_array(&arr_name);
                arr.len().saturating_sub(off)
            }
            Some(v) if v.is_undef() => {
                let arr = self.scope.get_array(&arr_name);
                arr.len().saturating_sub(off)
            }
            Some(v) => v.to_int().max(0) as usize,
        };
        let rep_vals: Vec<PerlValue> = args.iter().skip(3).cloned().collect();
        let arr = self.scope.get_array_mut(&arr_name)?;
        let end = (off + len).min(arr.len());
        let removed: Vec<PerlValue> = arr.drain(off..end).collect();
        for (i, v) in rep_vals.into_iter().enumerate() {
            arr.insert(off + i, v);
        }
        Ok(PerlValue::array(removed))
    }

    /// `unshift @array, LIST` — VM `CallBuiltin(Unshift)`.
    pub(crate) fn unshift_builtin_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        if args.is_empty() {
            return Err(PerlError::runtime("unshift: missing array", line));
        }
        let arr_name = args[0].to_string();
        let vals: Vec<PerlValue> = args.iter().skip(1).cloned().collect();
        let arr = self.scope.get_array_mut(&arr_name)?;
        for (i, v) in vals.into_iter().enumerate() {
            arr.insert(i, v);
        }
        Ok(PerlValue::integer(arr.len() as i64))
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

    /// Subroutine keys, blessed scalar classes, and `@ISA` edges for REPL `$obj->` completion.
    pub fn repl_completion_snapshot(&self) -> ReplCompletionSnapshot {
        let mut subs: Vec<String> = self.subs.keys().cloned().collect();
        subs.sort();
        let mut classes: HashSet<String> = HashSet::new();
        for k in &subs {
            if let Some((pkg, rest)) = k.split_once("::") {
                if !rest.contains("::") {
                    classes.insert(pkg.to_string());
                }
            }
        }
        let mut blessed_scalars: HashMap<String, String> = HashMap::new();
        for bn in self.scope.repl_binding_names() {
            if let Some(r) = bn.strip_prefix('$') {
                let v = self.scope.get_scalar(r);
                if let Some(b) = v.as_blessed_ref() {
                    blessed_scalars.insert(r.to_string(), b.class.clone());
                    classes.insert(b.class.clone());
                }
            }
        }
        let mut isa_for_class: HashMap<String, Vec<String>> = HashMap::new();
        for c in classes {
            isa_for_class.insert(c.clone(), self.parents_of_class(&c));
        }
        ReplCompletionSnapshot {
            subs,
            blessed_scalars,
            isa_for_class,
        }
    }

    pub(crate) fn run_bench_block(&mut self, body: &Block, n: usize, line: usize) -> ExecResult {
        if n == 0 {
            return Err(FlowOrError::Error(PerlError::runtime(
                "bench: iteration count must be positive",
                line,
            )));
        }
        let warmup = (n / 10).min(10).max(1);
        for _ in 0..warmup {
            self.exec_block(body)?;
        }
        let mut samples = Vec::with_capacity(n);
        for _ in 0..n {
            let start = std::time::Instant::now();
            self.exec_block(body)?;
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let mut sorted = samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let min_ms = sorted[0];
        let mean = samples.iter().sum::<f64>() / n as f64;
        let p99_idx = ((n as f64 * 0.99).ceil() as usize)
            .saturating_sub(1)
            .min(n - 1);
        let p99_ms = sorted[p99_idx];
        Ok(PerlValue::string(format!(
            "bench: n={} warmup={} min={:.6}ms mean={:.6}ms p99={:.6}ms",
            n, warmup, min_ms, mean, p99_ms
        )))
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
        // `${^GLOBAL_PHASE}` — each program starts in `RUN` (Perl before any `BEGIN` runs).
        self.global_phase = "RUN".to_string();
        // First pass: subs, `use` (source order), BEGIN/END collection
        self.prepare_program_top_level(program)?;

        // Execute BEGIN blocks (Perl uses phase `START` here).
        let begins = std::mem::take(&mut self.begin_blocks);
        if !begins.is_empty() {
            self.global_phase = "START".to_string();
        }
        for block in &begins {
            self.exec_block(block).map_err(|e| match e {
                FlowOrError::Error(e) => e,
                FlowOrError::Flow(_) => PerlError::runtime("Unexpected flow control in BEGIN", 0),
            })?;
        }

        // UNITCHECK — reverse order of compilation (end of unit, before CHECK).
        let ucs = std::mem::take(&mut self.unit_check_blocks);
        if !ucs.is_empty() {
            self.global_phase = "UNITCHECK".to_string();
        }
        for block in ucs.iter().rev() {
            self.exec_block(block).map_err(|e| match e {
                FlowOrError::Error(e) => e,
                FlowOrError::Flow(_) => PerlError::runtime("Unexpected flow control in UNITCHECK", 0),
            })?;
        }

        // CHECK — reverse order (end of compile phase).
        let checks = std::mem::take(&mut self.check_blocks);
        if !checks.is_empty() {
            self.global_phase = "CHECK".to_string();
        }
        for block in checks.iter().rev() {
            self.exec_block(block).map_err(|e| match e {
                FlowOrError::Error(e) => e,
                FlowOrError::Flow(_) => PerlError::runtime("Unexpected flow control in CHECK", 0),
            })?;
        }

        // INIT — forward order (before main runtime).
        let inits = std::mem::take(&mut self.init_blocks);
        if !inits.is_empty() {
            self.global_phase = "INIT".to_string();
        }
        for block in &inits {
            self.exec_block(block).map_err(|e| match e {
                FlowOrError::Error(e) => e,
                FlowOrError::Flow(_) => PerlError::runtime("Unexpected flow control in INIT", 0),
            })?;
        }

        self.global_phase = "RUN".to_string();

        // Execute main program
        let mut last = PerlValue::UNDEF;
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::Begin(_)
                | StmtKind::UnitCheck(_)
                | StmtKind::Check(_)
                | StmtKind::Init(_)
                | StmtKind::End(_)
                | StmtKind::Use { .. }
                | StmtKind::No { .. }
                | StmtKind::FormatDecl { .. } => continue,
                _ => {
                    match self.exec_statement(stmt) {
                        Ok(val) => last = val,
                        Err(FlowOrError::Error(e)) => {
                            // Execute END blocks before propagating (all exit codes, including 0)
                            self.global_phase = "END".to_string();
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

        // Execute END blocks (Perl uses phase `END` here).
        self.global_phase = "END".to_string();
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
            self.scope_push_hook();
            let r = self.exec_block_with_goto(block);
            self.scope_pop_hook();
            r
        } else {
            self.scope_push_hook();
            let result = self.exec_block_no_scope(block);
            self.scope_pop_hook();
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
        let mut last = PerlValue::UNDEF;
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
        let mut last = PerlValue::UNDEF;
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
            interp.enable_parallel_guard();
            let r = match interp.exec_block(&block) {
                Ok(v) => Ok(v),
                Err(FlowOrError::Error(e)) => Err(e),
                Err(FlowOrError::Flow(_)) => Ok(PerlValue::UNDEF),
            };
            *result2.lock() = Some(r);
        });
        *join.lock() = Some(h);
        PerlValue::async_task(Arc::new(PerlAsyncTask { result, join }))
    }

    /// `eval_timeout SECS { ... }` — run block on another thread; this thread waits (no Unix signals).
    pub(crate) fn eval_timeout_block(
        &mut self,
        body: &Block,
        secs: f64,
        line: usize,
    ) -> ExecResult {
        use std::sync::mpsc::channel;
        use std::time::Duration;

        let block = body.clone();
        let subs = self.subs.clone();
        let struct_defs = self.struct_defs.clone();
        let (scalars, aar, ahash) = self.scope.capture_with_atomics();
        self.materialize_env_if_needed();
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
                argv.iter().map(|s| PerlValue::string(s.clone())).collect(),
            );
            for (k, v) in env {
                interp
                    .scope
                    .set_hash_element("ENV", &k, v)
                    .expect("set ENV in timeout thread");
            }
            interp.scope.declare_array("INC", inc);
            interp.scope.restore_capture(&scalars);
            interp.scope.restore_atomics(&aar, &ahash);
            interp.enable_parallel_guard();
            let out: PerlResult<PerlValue> = match interp.exec_block(&block) {
                Ok(v) => Ok(v),
                Err(FlowOrError::Error(e)) => Err(e),
                Err(FlowOrError::Flow(_)) => Ok(PerlValue::UNDEF),
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
        let mut last = PerlValue::UNDEF;
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

    pub(crate) fn exec_given(&mut self, topic: &Expr, body: &Block) -> ExecResult {
        let t = self.eval_expr(topic)?;
        self.scope_push_hook();
        self.scope.declare_scalar("_", t);
        self.english_note_lexical_scalar("_");
        let r = self.exec_given_body(body);
        self.scope_pop_hook();
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
        if let Some(re) = c.as_regex() {
            return re.is_match(&topic.to_string());
        }
        topic.to_string() == c.to_string()
    }

    /// Boolean condition for postfix `if` / `unless` / `while` / `until`: bare `/.../` is `$_ =~ /.../`
    /// (Perl), not “is the regex object truthy”.
    fn eval_postfix_condition(&mut self, cond: &Expr) -> Result<bool, FlowOrError> {
        match &cond.kind {
            ExprKind::Regex(pattern, flags) => {
                let topic = self.scope.get_scalar("_");
                let line = cond.line;
                let re = self.compile_regex(pattern, flags, line)?;
                Ok(re.is_match(&topic.to_string()))
            }
            _ => {
                let v = self.eval_expr(cond)?;
                Ok(v.is_true())
            }
        }
    }

    pub(crate) fn eval_algebraic_match(
        &mut self,
        subject: &Expr,
        arms: &[MatchArm],
        line: usize,
    ) -> ExecResult {
        let val = self.eval_expr(subject)?;
        for arm in arms {
            if let Some(bindings) = self.match_pattern_try(&val, &arm.pattern, line)? {
                self.scope_push_hook();
                for (name, v) in bindings {
                    self.scope.declare_scalar(&name, v);
                    self.english_note_lexical_scalar(&name);
                }
                let out = self.eval_expr(&arm.body);
                self.scope_pop_hook();
                return out;
            }
        }
        Err(PerlError::runtime(
            "match: no arm matched the value (add a `_` catch-all)",
            line,
        )
        .into())
    }

    fn match_pattern_try(
        &mut self,
        subject: &PerlValue,
        pattern: &MatchPattern,
        line: usize,
    ) -> Result<Option<Vec<(String, PerlValue)>>, FlowOrError> {
        match pattern {
            MatchPattern::Any => Ok(Some(vec![])),
            MatchPattern::Regex { pattern, flags } => {
                let re = self.compile_regex(pattern, flags, line)?;
                let s = subject.to_string();
                if re.is_match(&s) {
                    Ok(Some(vec![]))
                } else {
                    Ok(None)
                }
            }
            MatchPattern::Value(expr) => {
                let pv = self.eval_expr(expr)?;
                if self.smartmatch_when(subject, &pv) {
                    Ok(Some(vec![]))
                } else {
                    Ok(None)
                }
            }
            MatchPattern::Array(elems) => {
                let Some(arr) = Self::match_value_as_array(subject) else {
                    return Ok(None);
                };
                self.match_array_pattern_elems(&arr, elems, line)
            }
            MatchPattern::Hash(pairs) => {
                let Some(h) = Self::match_value_as_hash(subject) else {
                    return Ok(None);
                };
                self.match_hash_pattern_pairs(&h, pairs, line)
            }
        }
    }

    fn match_value_as_array(v: &PerlValue) -> Option<Vec<PerlValue>> {
        if let Some(a) = v.as_array_vec() {
            return Some(a);
        }
        if let Some(r) = v.as_array_ref() {
            return Some(r.read().clone());
        }
        None
    }

    fn match_value_as_hash(v: &PerlValue) -> Option<IndexMap<String, PerlValue>> {
        if let Some(h) = v.as_hash_map() {
            return Some(h);
        }
        if let Some(r) = v.as_hash_ref() {
            return Some(r.read().clone());
        }
        None
    }

    fn match_array_pattern_elems(
        &mut self,
        arr: &[PerlValue],
        elems: &[MatchArrayElem],
        line: usize,
    ) -> Result<Option<Vec<(String, PerlValue)>>, FlowOrError> {
        let has_rest = elems.iter().any(|e| matches!(e, MatchArrayElem::Rest));
        let mut idx = 0usize;
        for (i, elem) in elems.iter().enumerate() {
            match elem {
                MatchArrayElem::Rest => {
                    if i != elems.len() - 1 {
                        return Err(PerlError::runtime(
                            "internal: `*` must be last in array match pattern",
                            line,
                        )
                        .into());
                    }
                    return Ok(Some(vec![]));
                }
                MatchArrayElem::Expr(e) => {
                    if idx >= arr.len() {
                        return Ok(None);
                    }
                    let expected = self.eval_expr(e)?;
                    if !self.smartmatch_when(&arr[idx], &expected) {
                        return Ok(None);
                    }
                    idx += 1;
                }
            }
        }
        if !has_rest && idx != arr.len() {
            return Ok(None);
        }
        Ok(Some(vec![]))
    }

    fn match_hash_pattern_pairs(
        &mut self,
        h: &IndexMap<String, PerlValue>,
        pairs: &[MatchHashPair],
        _line: usize,
    ) -> Result<Option<Vec<(String, PerlValue)>>, FlowOrError> {
        let mut binds = Vec::new();
        for pair in pairs {
            match pair {
                MatchHashPair::KeyOnly { key } => {
                    let ks = self.eval_expr(key)?.to_string();
                    if !h.contains_key(&ks) {
                        return Ok(None);
                    }
                }
                MatchHashPair::Capture { key, name } => {
                    let ks = self.eval_expr(key)?.to_string();
                    let Some(v) = h.get(&ks) else {
                        return Ok(None);
                    };
                    binds.push((name.clone(), v.clone()));
                }
            }
        }
        Ok(Some(binds))
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
        if let Err(e) = crate::perl_signal::poll(self) {
            return Err(FlowOrError::Error(e));
        }
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
                Ok(PerlValue::UNDEF)
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
                Ok(PerlValue::UNDEF)
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
                Ok(PerlValue::UNDEF)
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
                Ok(PerlValue::UNDEF)
            }
            StmtKind::DoWhile { body, condition } => {
                loop {
                    self.exec_block(body)?;
                    let cond = self.eval_expr(condition)?;
                    if !cond.is_true() {
                        break;
                    }
                }
                Ok(PerlValue::UNDEF)
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
                label,
                continue_block,
            } => {
                self.scope_push_hook();
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
                                self.scope_pop_hook();
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
                self.scope_pop_hook();
                Ok(PerlValue::UNDEF)
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
                self.scope_push_hook();
                self.scope.declare_scalar(var, PerlValue::UNDEF);
                self.english_note_lexical_scalar(var);
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
                                self.scope_pop_hook();
                                return Err(e);
                            }
                        }
                    }
                    if let Some(cb) = continue_block {
                        let _ = self.exec_block_smart(cb);
                    }
                    i += 1;
                }
                self.scope_pop_hook();
                Ok(PerlValue::UNDEF)
            }
            StmtKind::SubDecl {
                name,
                params,
                body,
                prototype,
            } => {
                let key = self.qualify_sub_key(name);
                let captured = self.scope.capture();
                let closure_env = if captured.is_empty() {
                    None
                } else {
                    Some(captured)
                };
                let mut sub = PerlSub {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    closure_env,
                    prototype: prototype.clone(),
                    fib_like: None,
                };
                sub.fib_like = crate::fib_like_tail::detect_fib_like_recursive_add(&sub);
                self.subs.insert(key, Arc::new(sub));
                Ok(PerlValue::UNDEF)
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
                Ok(PerlValue::UNDEF)
            }
            StmtKind::My(decls) | StmtKind::Our(decls) => {
                let is_our = matches!(&stmt.kind, StmtKind::Our(_));
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
                                let v = items.get(idx).cloned().unwrap_or(PerlValue::UNDEF);
                                self.scope.declare_scalar_frozen(
                                    &decl.name,
                                    v,
                                    decl.frozen,
                                    decl.type_annotation,
                                )?;
                                self.english_note_lexical_scalar(&decl.name);
                                idx += 1;
                            }
                            Sigil::Array => {
                                // Array slurps remaining elements
                                let rest: Vec<PerlValue> = items[idx..].to_vec();
                                idx = items.len();
                                if is_our {
                                    self.record_exporter_our_array_name(&decl.name, &rest);
                                }
                                let aname = self.stash_array_name_for_package(&decl.name);
                                self.scope.declare_array(&aname, rest);
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
                            Sigil::Typeglob => {
                                return Err(PerlError::runtime(
                                    "list assignment to typeglob (`my (*a,*b)=...`) is not supported",
                                    stmt.line,
                                )
                                .into());
                            }
                        }
                    }
                } else {
                    // Single decl or no initializer
                    for decl in decls {
                        let val = if let Some(init) = &decl.initializer {
                            let ctx = match decl.sigil {
                                Sigil::Array | Sigil::Hash => WantarrayCtx::List,
                                Sigil::Scalar | Sigil::Typeglob => WantarrayCtx::Scalar,
                            };
                            self.eval_expr_ctx(init, ctx)?
                        } else {
                            PerlValue::UNDEF
                        };
                        match decl.sigil {
                            Sigil::Typeglob => {
                                return Err(PerlError::runtime(
                                    "`my *FH` / typeglob declaration is not supported",
                                    stmt.line,
                                )
                                .into());
                            }
                            Sigil::Scalar => {
                                self.scope.declare_scalar_frozen(
                                    &decl.name,
                                    val,
                                    decl.frozen,
                                    decl.type_annotation,
                                )?;
                                self.english_note_lexical_scalar(&decl.name);
                            }
                            Sigil::Array => {
                                let items = val.to_list();
                                if is_our {
                                    self.record_exporter_our_array_name(&decl.name, &items);
                                }
                                let aname = self.stash_array_name_for_package(&decl.name);
                                self.scope.declare_array_frozen(&aname, items, decl.frozen);
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
                Ok(PerlValue::UNDEF)
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
                                let v = items.get(idx).cloned().unwrap_or(PerlValue::UNDEF);
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
                                if decl.name == "ENV" {
                                    self.materialize_env_if_needed();
                                }
                                let mut map = IndexMap::new();
                                let mut i = 0;
                                while i + 1 < rest.len() {
                                    map.insert(rest[i].to_string(), rest[i + 1].clone());
                                    i += 2;
                                }
                                self.scope.local_set_hash(&decl.name, map)?;
                            }
                            Sigil::Typeglob => {
                                return Err(PerlError::runtime(
                                    "list assignment to typeglob (`local (*a,*b)=...`) is not supported",
                                    stmt.line,
                                )
                                .into());
                            }
                        }
                    }
                } else {
                    for decl in decls {
                        let val = if let Some(init) = &decl.initializer {
                            let ctx = match decl.sigil {
                                Sigil::Array | Sigil::Hash => WantarrayCtx::List,
                                Sigil::Scalar | Sigil::Typeglob => WantarrayCtx::Scalar,
                            };
                            self.eval_expr_ctx(init, ctx)?
                        } else {
                            PerlValue::UNDEF
                        };
                        match decl.sigil {
                            Sigil::Typeglob => {
                                let old = self.glob_handle_alias.remove(&decl.name);
                                if let Some(frame) = self.glob_restore_frames.last_mut() {
                                    frame.push((decl.name.clone(), old));
                                }
                                if let Some(init) = &decl.initializer {
                                    if let ExprKind::Typeglob(rhs) = &init.kind {
                                        self.glob_handle_alias
                                            .insert(decl.name.clone(), rhs.clone());
                                    } else {
                                        return Err(PerlError::runtime(
                                            "local *GLOB = *OTHER — right side must be a typeglob",
                                            stmt.line,
                                        )
                                        .into());
                                    }
                                }
                            }
                            Sigil::Scalar => {
                                self.scope.local_set_scalar(&decl.name, val)?;
                            }
                            Sigil::Array => {
                                self.scope.local_set_array(&decl.name, val.to_list())?;
                            }
                            Sigil::Hash => {
                                if decl.name == "ENV" {
                                    self.materialize_env_if_needed();
                                }
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
                Ok(PerlValue::UNDEF)
            }
            StmtKind::MySync(decls) => {
                for decl in decls {
                    let val = if let Some(init) = &decl.initializer {
                        self.eval_expr(init)?
                    } else {
                        PerlValue::UNDEF
                    };
                    match decl.sigil {
                        Sigil::Typeglob => {
                            return Err(PerlError::runtime(
                                "`mysync` does not support typeglob variables",
                                stmt.line,
                            )
                            .into());
                        }
                        Sigil::Scalar => {
                            // `deque()` / `heap(...)` are already `Arc<Mutex<…>>`; avoid a second
                            // mutex wrapper. Other scalars (including `Set->new`) use Atomic.
                            let stored = if val.is_mysync_deque_or_heap() {
                                val
                            } else {
                                PerlValue::atomic(std::sync::Arc::new(parking_lot::Mutex::new(val)))
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
                Ok(PerlValue::UNDEF)
            }
            StmtKind::Package { name } => {
                // Minimal package support — just set a variable
                let _ = self
                    .scope
                    .set_scalar("__PACKAGE__", PerlValue::string(name.clone()));
                Ok(PerlValue::UNDEF)
            }
            StmtKind::Use { .. } => {
                // Handled in `prepare_program_top_level` before BEGIN / main.
                Ok(PerlValue::UNDEF)
            }
            StmtKind::UseOverload { pairs } => {
                let mut pkg = self.scope.get_scalar("__PACKAGE__").to_string();
                if pkg.is_empty() {
                    pkg = "main".to_string();
                }
                let table = self.overload_table.entry(pkg).or_default();
                for (k, v) in pairs {
                    table.insert(k.clone(), v.clone());
                }
                Ok(PerlValue::UNDEF)
            }
            StmtKind::No { .. } => {
                // Handled in `prepare_program_top_level` (same phase as `use`).
                Ok(PerlValue::UNDEF)
            }
            StmtKind::Return(val) => {
                let v = if let Some(e) = val {
                    self.eval_expr(e)?
                } else {
                    PerlValue::UNDEF
                };
                Err(Flow::Return(v).into())
            }
            StmtKind::Last(label) => Err(Flow::Last(label.clone()).into()),
            StmtKind::Next(label) => Err(Flow::Next(label.clone()).into()),
            StmtKind::Redo(label) => Err(Flow::Redo(label.clone()).into()),
            StmtKind::Block(block) => self.exec_block(block),
            StmtKind::Begin(_)
            | StmtKind::UnitCheck(_)
            | StmtKind::Check(_)
            | StmtKind::Init(_)
            | StmtKind::End(_) => Ok(PerlValue::UNDEF),
            StmtKind::Empty => Ok(PerlValue::UNDEF),
            StmtKind::Goto { .. } => {
                Err(PerlError::runtime("goto reached outside goto-aware block", stmt.line).into())
            }
            StmtKind::EvalTimeout { timeout, body } => {
                let secs = self.eval_expr(timeout)?.to_number();
                self.eval_timeout_block(body, secs, stmt.line)
            }
            StmtKind::Tie {
                target,
                class,
                args,
            } => {
                let pkg = self.eval_expr(class)?.to_string();
                let pkg = pkg.trim_matches(|c| c == '\'' || c == '"').to_string();
                let tie_ctor = match target {
                    TieTarget::Hash(_) => "TIEHASH",
                    TieTarget::Array(_) => "TIEARRAY",
                    TieTarget::Scalar(_) => "TIESCALAR",
                };
                let tie_fn = format!("{}::{}", pkg, tie_ctor);
                let sub = self.subs.get(&tie_fn).cloned().ok_or_else(|| {
                    PerlError::runtime(format!("tie: cannot find &{}", tie_fn), stmt.line)
                })?;
                let mut call_args = vec![PerlValue::string(pkg.clone())];
                for a in args {
                    call_args.push(self.eval_expr(a)?);
                }
                let obj = match self.call_sub(&sub, call_args, WantarrayCtx::Scalar, stmt.line) {
                    Ok(v) => v,
                    Err(FlowOrError::Flow(_)) => PerlValue::UNDEF,
                    Err(FlowOrError::Error(e)) => return Err(FlowOrError::Error(e)),
                };
                match target {
                    TieTarget::Hash(h) => {
                        self.tied_hashes.insert(h.clone(), obj);
                    }
                    TieTarget::Array(a) => {
                        let key = self.stash_array_name_for_package(a);
                        self.tied_arrays.insert(key, obj);
                    }
                    TieTarget::Scalar(s) => {
                        self.tied_scalars.insert(s.clone(), obj);
                    }
                }
                Ok(PerlValue::UNDEF)
            }
            StmtKind::TryCatch {
                try_block,
                catch_var,
                catch_block,
                finally_block,
            } => match self.exec_block(try_block) {
                Ok(v) => {
                    if let Some(fb) = finally_block {
                        self.exec_block(fb)?;
                    }
                    Ok(v)
                }
                Err(FlowOrError::Error(e)) => {
                    if matches!(e.kind, ErrorKind::Exit(_)) {
                        return Err(FlowOrError::Error(e));
                    }
                    self.scope_push_hook();
                    self.scope
                        .declare_scalar(catch_var, PerlValue::string(e.to_string()));
                    self.english_note_lexical_scalar(catch_var);
                    let r = self.exec_block(catch_block);
                    self.scope_pop_hook();
                    if let Some(fb) = finally_block {
                        self.exec_block(fb)?;
                    }
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
            StmtKind::FormatDecl { .. } => {
                // Registered in `prepare_program_top_level`; no per-statement runtime effect.
                Ok(PerlValue::UNDEF)
            }
            StmtKind::Continue(block) => self.exec_block_smart(block),
        }
    }

    #[inline]
    pub(crate) fn eval_expr(&mut self, expr: &Expr) -> ExecResult {
        self.eval_expr_ctx(expr, WantarrayCtx::Scalar)
    }

    /// Scalar `$x OP= $rhs` — single [`Scope::atomic_mutate`] so `mysync` is RMW-safe.
    pub(crate) fn scalar_compound_assign_scalar_target(
        &mut self,
        name: &str,
        op: BinOp,
        rhs: PerlValue,
    ) -> PerlValue {
        self.scope
            .atomic_mutate(name, |old| Self::compound_scalar_binop(old, op, &rhs))
    }

    fn compound_scalar_binop(old: &PerlValue, op: BinOp, rhs: &PerlValue) -> PerlValue {
        match op {
            BinOp::Add => {
                if let (Some(a), Some(b)) = (old.as_integer(), rhs.as_integer()) {
                    PerlValue::integer(a.wrapping_add(b))
                } else {
                    PerlValue::float(old.to_number() + rhs.to_number())
                }
            }
            BinOp::Sub => {
                if let (Some(a), Some(b)) = (old.as_integer(), rhs.as_integer()) {
                    PerlValue::integer(a.wrapping_sub(b))
                } else {
                    PerlValue::float(old.to_number() - rhs.to_number())
                }
            }
            BinOp::Mul => {
                if let (Some(a), Some(b)) = (old.as_integer(), rhs.as_integer()) {
                    PerlValue::integer(a.wrapping_mul(b))
                } else {
                    PerlValue::float(old.to_number() * rhs.to_number())
                }
            }
            BinOp::Concat => {
                let mut s = old.to_string();
                rhs.append_to(&mut s);
                PerlValue::string(s)
            }
            BinOp::BitAnd => {
                if let Some(s) = crate::value::set_intersection(old, rhs) {
                    s
                } else {
                    PerlValue::integer(old.to_int() & rhs.to_int())
                }
            }
            BinOp::BitOr => {
                if let Some(s) = crate::value::set_union(old, rhs) {
                    s
                } else {
                    PerlValue::integer(old.to_int() | rhs.to_int())
                }
            }
            BinOp::BitXor => PerlValue::integer(old.to_int() ^ rhs.to_int()),
            BinOp::ShiftLeft => PerlValue::integer(old.to_int() << rhs.to_int()),
            BinOp::ShiftRight => PerlValue::integer(old.to_int() >> rhs.to_int()),
            BinOp::Div => PerlValue::float(old.to_number() / rhs.to_number()),
            BinOp::Mod => PerlValue::float(old.to_number() % rhs.to_number()),
            BinOp::Pow => PerlValue::float(old.to_number().powf(rhs.to_number())),
            _ => PerlValue::float(old.to_number() + rhs.to_number()),
        }
    }

    fn eval_expr_ctx(&mut self, expr: &Expr, ctx: WantarrayCtx) -> ExecResult {
        let line = expr.line;
        match &expr.kind {
            ExprKind::Integer(n) => Ok(PerlValue::integer(*n)),
            ExprKind::Float(f) => Ok(PerlValue::float(*f)),
            ExprKind::String(s) => Ok(PerlValue::string(s.clone())),
            ExprKind::Undef => Ok(PerlValue::UNDEF),
            ExprKind::MagicConst(MagicConstKind::File) => Ok(PerlValue::string(self.file.clone())),
            ExprKind::MagicConst(MagicConstKind::Line) => Ok(PerlValue::integer(expr.line as i64)),
            ExprKind::Regex(pattern, flags) => {
                let re = self.compile_regex(pattern, flags, line)?;
                Ok(PerlValue::regex(re, pattern.clone()))
            }
            ExprKind::QW(words) => Ok(PerlValue::array(
                words.iter().map(|w| PerlValue::string(w.clone())).collect(),
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
                            let s = self.stringify_value(val, line)?;
                            result.push_str(&s);
                        }
                        StringPart::ArrayVar(name) => {
                            self.check_strict_array_var(name, line)?;
                            let aname = self.stash_array_name_for_package(name);
                            let arr = self.scope.get_array(&aname);
                            let mut parts = Vec::with_capacity(arr.len());
                            for v in &arr {
                                parts.push(self.stringify_value(v.clone(), line)?);
                            }
                            let sep = self.list_separator.clone();
                            result.push_str(&parts.join(&sep));
                        }
                        StringPart::Expr(e) => {
                            let val = self.eval_expr(e)?;
                            let s = self.stringify_value(val, line)?;
                            result.push_str(&s);
                        }
                    }
                }
                Ok(PerlValue::string(result))
            }

            // Variables
            ExprKind::ScalarVar(name) => {
                self.check_strict_scalar_var(name, line)?;
                if let Some(obj) = self.tied_scalars.get(name).cloned() {
                    let class = obj
                        .as_blessed_ref()
                        .map(|b| b.class.clone())
                        .unwrap_or_default();
                    let full = format!("{}::FETCH", class);
                    if let Some(sub) = self.subs.get(&full).cloned() {
                        return self.call_sub(&sub, vec![obj], ctx, line);
                    }
                }
                Ok(self.get_special_var(name))
            }
            ExprKind::ArrayVar(name) => {
                self.check_strict_array_var(name, line)?;
                let aname = self.stash_array_name_for_package(name);
                Ok(PerlValue::array(self.scope.get_array(&aname)))
            }
            ExprKind::HashVar(name) => {
                self.check_strict_hash_var(name, line)?;
                self.touch_env_hash(name);
                Ok(PerlValue::hash(self.scope.get_hash(name)))
            }
            ExprKind::Typeglob(name) => {
                let n = self.resolve_io_handle_name(name);
                Ok(PerlValue::string(n))
            }
            ExprKind::ArrayElement { array, index } => {
                self.check_strict_array_var(array, line)?;
                let idx = self.eval_expr(index)?.to_int();
                let aname = self.stash_array_name_for_package(array);
                if let Some(obj) = self.tied_arrays.get(&aname).cloned() {
                    let class = obj
                        .as_blessed_ref()
                        .map(|b| b.class.clone())
                        .unwrap_or_default();
                    let full = format!("{}::FETCH", class);
                    if let Some(sub) = self.subs.get(&full).cloned() {
                        let arg_vals = vec![obj, PerlValue::integer(idx)];
                        return self.call_sub(&sub, arg_vals, ctx, line);
                    }
                }
                Ok(self.scope.get_array_element(&aname, idx))
            }
            ExprKind::HashElement { hash, key } => {
                self.check_strict_hash_var(hash, line)?;
                let k = self.eval_expr(key)?.to_string();
                self.touch_env_hash(hash);
                if let Some(obj) = self.tied_hashes.get(hash).cloned() {
                    let class = obj
                        .as_blessed_ref()
                        .map(|b| b.class.clone())
                        .unwrap_or_default();
                    let full = format!("{}::FETCH", class);
                    if let Some(sub) = self.subs.get(&full).cloned() {
                        let arg_vals = vec![obj, PerlValue::string(k)];
                        return self.call_sub(&sub, arg_vals, ctx, line);
                    }
                }
                Ok(self.scope.get_hash_element(hash, &k))
            }
            ExprKind::ArraySlice { array, indices } => {
                self.check_strict_array_var(array, line)?;
                let mut result = Vec::new();
                for idx_expr in indices {
                    let idx = self.eval_expr(idx_expr)?.to_int();
                    result.push(self.scope.get_array_element(array, idx));
                }
                Ok(PerlValue::array(result))
            }
            ExprKind::HashSlice { hash, keys } => {
                self.check_strict_hash_var(hash, line)?;
                self.touch_env_hash(hash);
                let mut result = Vec::new();
                for key_expr in keys {
                    let k = self.eval_expr(key_expr)?.to_string();
                    result.push(self.scope.get_hash_element(hash, &k));
                }
                Ok(PerlValue::array(result))
            }

            // References
            ExprKind::ScalarRef(inner) => {
                let val = self.eval_expr(inner)?;
                Ok(PerlValue::scalar_ref(Arc::new(RwLock::new(val))))
            }
            ExprKind::ArrayRef(elems) => {
                let mut arr = Vec::with_capacity(elems.len());
                for e in elems {
                    arr.push(self.eval_expr(e)?);
                }
                Ok(PerlValue::array_ref(Arc::new(RwLock::new(arr))))
            }
            ExprKind::HashRef(pairs) => {
                let mut map = IndexMap::new();
                for (k, v) in pairs {
                    let key = self.eval_expr(k)?.to_string();
                    let val = self.eval_expr(v)?;
                    map.insert(key, val);
                }
                Ok(PerlValue::hash_ref(Arc::new(RwLock::new(map))))
            }
            ExprKind::CodeRef { params, body } => {
                let captured = self.scope.capture();
                Ok(PerlValue::code_ref(Arc::new(PerlSub {
                    name: "__ANON__".to_string(),
                    params: params.clone(),
                    body: body.clone(),
                    closure_env: Some(captured),
                    prototype: None,
                    fib_like: None,
                })))
            }
            ExprKind::SubroutineRef(name) => self.call_named_sub(name, vec![], line, ctx),
            ExprKind::SubroutineCodeRef(name) => {
                let sub = self.resolve_sub_by_name(name).ok_or_else(|| {
                    PerlError::runtime(
                        format!("Undefined subroutine {}", self.qualify_sub_key(name)),
                        line,
                    )
                })?;
                Ok(PerlValue::code_ref(sub))
            }
            ExprKind::Deref { expr, kind } => {
                let val = self.eval_expr(expr)?;
                match kind {
                    Sigil::Scalar => {
                        if let Some(r) = val.as_scalar_ref() {
                            return Ok(r.read().clone());
                        }
                        if let Some(s) = val.as_str() {
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
                            return Ok(self.get_special_var(&s));
                        }
                        Err(
                            PerlError::runtime("Can't dereference non-reference as scalar", line)
                                .into(),
                        )
                    }
                    Sigil::Array => {
                        if let Some(r) = val.as_array_ref() {
                            return Ok(PerlValue::array(r.read().clone()));
                        }
                        if let Some(s) = val.as_str() {
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
                            return Ok(PerlValue::array(self.scope.get_array(&s)));
                        }
                        Err(
                            PerlError::runtime("Can't dereference non-reference as array", line)
                                .into(),
                        )
                    }
                    Sigil::Hash => {
                        if let Some(r) = val.as_hash_ref() {
                            return Ok(PerlValue::hash(r.read().clone()));
                        }
                        if let Some(s) = val.as_str() {
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
                            self.touch_env_hash(&s);
                            return Ok(PerlValue::hash(self.scope.get_hash(&s)));
                        }
                        Err(
                            PerlError::runtime("Can't dereference non-reference as hash", line)
                                .into(),
                        )
                    }
                    Sigil::Typeglob => {
                        if let Some(s) = val.as_str() {
                            return Ok(PerlValue::string(self.resolve_io_handle_name(&s)));
                        }
                        Err(
                            PerlError::runtime("Can't dereference non-reference as typeglob", line)
                                .into(),
                        )
                    }
                }
            }
            ExprKind::ArrowDeref { expr, index, kind } => {
                let val = self.eval_expr(expr)?;
                match kind {
                    DerefKind::Array => {
                        let idx = self.eval_expr(index)?.to_int();
                        if let Some(r) = val.as_array_ref() {
                            let arr = r.read();
                            let i = if idx < 0 {
                                (arr.len() as i64 + idx) as usize
                            } else {
                                idx as usize
                            };
                            return Ok(arr.get(i).cloned().unwrap_or(PerlValue::UNDEF));
                        }
                        Err(
                            PerlError::runtime("Can't use arrow deref on non-array-ref", line)
                                .into(),
                        )
                    }
                    DerefKind::Hash => {
                        let key = self.eval_expr(index)?.to_string();
                        if let Some(r) = val.as_hash_ref() {
                            let h = r.read();
                            return Ok(h.get(&key).cloned().unwrap_or(PerlValue::UNDEF));
                        }
                        if let Some(b) = val.as_blessed_ref() {
                            let data = b.data.read();
                            if let Some(v) = data.hash_get(&key) {
                                return Ok(v);
                            }
                            if let Some(r) = data.as_hash_ref() {
                                let h = r.read();
                                return Ok(h.get(&key).cloned().unwrap_or(PerlValue::UNDEF));
                            }
                            return Err(PerlError::runtime(
                                "Can't access hash field on non-hash blessed ref",
                                line,
                            )
                            .into());
                        }
                        Err(
                            PerlError::runtime("Can't use arrow deref on non-hash-ref", line)
                                .into(),
                        )
                    }
                    DerefKind::Call => {
                        // $coderef->(args)
                        if let ExprKind::List(ref arg_exprs) = index.kind {
                            let mut args = Vec::new();
                            for a in arg_exprs {
                                args.push(self.eval_expr(a)?);
                            }
                            if let Some(sub) = val.as_code_ref() {
                                return self.call_sub(&sub, args, ctx, line);
                            }
                            Err(PerlError::runtime("Not a code reference", line).into())
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
                        return Ok(PerlValue::integer(if m.is_true() { 0 } else { 1 }));
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
                        if !lv.is_undef() {
                            return Ok(lv);
                        }
                        return self.eval_expr(right);
                    }
                    _ => {}
                }
                let rv = self.eval_expr(right)?;
                if let Some(r) = self.try_overload_binop(*op, &lv, &rv, line) {
                    return r;
                }
                self.eval_binop(*op, &lv, &rv, line)
            }

            // Unary
            ExprKind::UnaryOp { op, expr } => match op {
                UnaryOp::PreIncrement => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
                        self.check_strict_scalar_var(name, line)?;
                        let n = self.english_scalar_name(name);
                        return Ok(self
                            .scope
                            .atomic_mutate(n, |v| PerlValue::integer(v.to_int() + 1)));
                    }
                    let val = self.eval_expr(expr)?;
                    let new_val = PerlValue::integer(val.to_int() + 1);
                    self.assign_value(expr, new_val.clone())?;
                    Ok(new_val)
                }
                UnaryOp::PreDecrement => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
                        self.check_strict_scalar_var(name, line)?;
                        let n = self.english_scalar_name(name);
                        return Ok(self
                            .scope
                            .atomic_mutate(n, |v| PerlValue::integer(v.to_int() - 1)));
                    }
                    let val = self.eval_expr(expr)?;
                    let new_val = PerlValue::integer(val.to_int() - 1);
                    self.assign_value(expr, new_val.clone())?;
                    Ok(new_val)
                }
                _ => {
                    let val = self.eval_expr(expr)?;
                    match op {
                        UnaryOp::Negate => {
                            if let Some(r) = self.try_overload_unary_dispatch("neg", &val, line) {
                                return r;
                            }
                            if let Some(n) = val.as_integer() {
                                Ok(PerlValue::integer(-n))
                            } else {
                                Ok(PerlValue::float(-val.to_number()))
                            }
                        }
                        UnaryOp::LogNot => {
                            if let Some(r) = self.try_overload_unary_dispatch("bool", &val, line) {
                                let pv = r?;
                                return Ok(PerlValue::integer(if pv.is_true() { 0 } else { 1 }));
                            }
                            Ok(PerlValue::integer(if val.is_true() { 0 } else { 1 }))
                        }
                        UnaryOp::BitNot => Ok(PerlValue::integer(!val.to_int())),
                        UnaryOp::LogNotWord => {
                            if let Some(r) = self.try_overload_unary_dispatch("bool", &val, line) {
                                let pv = r?;
                                return Ok(PerlValue::integer(if pv.is_true() { 0 } else { 1 }));
                            }
                            Ok(PerlValue::integer(if val.is_true() { 0 } else { 1 }))
                        }
                        UnaryOp::Ref => Ok(PerlValue::scalar_ref(Arc::new(RwLock::new(val)))),
                        _ => unreachable!(),
                    }
                }
            },

            ExprKind::PostfixOp { expr, op } => {
                // For scalar variables, use atomic_mutate_post to hold the lock
                // for the entire read-modify-write (critical for mysync).
                if let ExprKind::ScalarVar(name) = &expr.kind {
                    self.check_strict_scalar_var(name, line)?;
                    let n = self.english_scalar_name(name);
                    let f: fn(&PerlValue) -> PerlValue = match op {
                        PostfixOp::Increment => |v| PerlValue::integer(v.to_int() + 1),
                        PostfixOp::Decrement => |v| PerlValue::integer(v.to_int() - 1),
                    };
                    return Ok(self.scope.atomic_mutate_post(n, f));
                }
                let val = self.eval_expr(expr)?;
                let old = val.clone();
                let new_val = match op {
                    PostfixOp::Increment => PerlValue::integer(val.to_int() + 1),
                    PostfixOp::Decrement => PerlValue::integer(val.to_int() - 1),
                };
                self.assign_value(expr, new_val)?;
                Ok(old)
            }

            // Assignment
            ExprKind::Assign { target, value } => {
                if let ExprKind::Typeglob(lhs) = &target.kind {
                    if let ExprKind::Typeglob(rhs) = &value.kind {
                        self.copy_typeglob_slots(lhs, rhs, line)?;
                        return self.eval_expr(value);
                    }
                }
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
                    let n = self.english_scalar_name(name);
                    let op = *op;
                    return Ok(self.scalar_compound_assign_scalar_target(n, op, rhs));
                }
                // For hash element targets: $h{key} += 1
                if let ExprKind::HashElement { hash, key } = &target.kind {
                    self.check_strict_hash_var(hash, line)?;
                    let k = self.eval_expr(key)?.to_string();
                    let op = *op;
                    return Ok(self.scope.atomic_hash_mutate(hash, &k, |old| match op {
                        BinOp::Add => {
                            if let (Some(a), Some(b)) = (old.as_integer(), rhs.as_integer()) {
                                PerlValue::integer(a.wrapping_add(b))
                            } else {
                                PerlValue::float(old.to_number() + rhs.to_number())
                            }
                        }
                        BinOp::Sub => {
                            if let (Some(a), Some(b)) = (old.as_integer(), rhs.as_integer()) {
                                PerlValue::integer(a.wrapping_sub(b))
                            } else {
                                PerlValue::float(old.to_number() - rhs.to_number())
                            }
                        }
                        BinOp::Concat => {
                            let mut s = old.to_string();
                            rhs.append_to(&mut s);
                            PerlValue::string(s)
                        }
                        _ => PerlValue::float(old.to_number() + rhs.to_number()),
                    })?);
                }
                // For array element targets: $a[i] += 1
                if let ExprKind::ArrayElement { array, index } = &target.kind {
                    self.check_strict_array_var(array, line)?;
                    let idx = self.eval_expr(index)?.to_int();
                    let op = *op;
                    return Ok(self.scope.atomic_array_mutate(array, idx, |old| match op {
                        BinOp::Add => {
                            if let (Some(a), Some(b)) = (old.as_integer(), rhs.as_integer()) {
                                PerlValue::integer(a.wrapping_add(b))
                            } else {
                                PerlValue::float(old.to_number() + rhs.to_number())
                            }
                        }
                        _ => PerlValue::float(old.to_number() + rhs.to_number()),
                    })?);
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
                let list: Vec<PerlValue> = (f..=t).map(PerlValue::integer).collect();
                Ok(PerlValue::array(list))
            }

            // Repeat
            ExprKind::Repeat { expr, count } => {
                let val = self.eval_expr(expr)?;
                let n = self.eval_expr(count)?.to_int().max(0) as usize;
                if let Some(s) = val.as_str() {
                    Ok(PerlValue::string(s.repeat(n)))
                } else if let Some(a) = val.as_array_vec() {
                    let mut result = Vec::with_capacity(a.len() * n);
                    for _ in 0..n {
                        result.extend(a.iter().cloned());
                    }
                    Ok(PerlValue::array(result))
                } else {
                    Ok(PerlValue::string(val.to_string().repeat(n)))
                }
            }

            // Function calls
            ExprKind::FuncCall { name, args } => {
                let mut arg_vals = Vec::with_capacity(args.len());
                for a in args {
                    let v = self.eval_expr(a)?;
                    if let Some(items) = v.as_array_vec() {
                        arg_vals.extend(items);
                    } else {
                        arg_vals.push(v);
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
                super_call,
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
                let class = if let Some(b) = obj.as_blessed_ref() {
                    b.class.clone()
                } else if let Some(s) = obj.as_str() {
                    s // Class->method()
                } else {
                    return Err(PerlError::runtime("Can't call method on non-object", line).into());
                };
                let full_name = self
                    .resolve_method_full_name(&class, method, *super_call)
                    .ok_or_else(|| {
                        PerlError::runtime(
                            format!(
                                "Can't locate method \"{}\" for invocant \"{}\"",
                                method, class
                            ),
                            line,
                        )
                    })?;
                if let Some(sub) = self.subs.get(&full_name).cloned() {
                    self.call_sub(&sub, arg_vals, ctx, line)
                } else if method == "new" && !*super_call {
                    // Default constructor
                    self.builtin_new(&class, arg_vals, line)
                } else if let Some(r) =
                    self.try_autoload_call(&full_name, arg_vals, line, ctx, Some(&class))
                {
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
                    // Match Perl 5: `message at FILE line N.` (trailing period before newline).
                    msg.push_str(&format!(" at {} line {}.", self.file, line));
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
                    msg.push_str(&format!(" at {} line {}.", self.file, line));
                    msg.push('\n');
                }
                eprint!("{}", msg);
                Ok(PerlValue::integer(1))
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
                    if let Some(a) = val.as_array_vec() {
                        result.extend(a);
                    } else {
                        result.push(val);
                    }
                }
                Ok(PerlValue::array(result))
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
                Ok(PerlValue::array(result))
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
                Ok(PerlValue::array(items))
            }
            ExprKind::ReverseExpr(list) => {
                let val = self.eval_expr(list)?;
                if let Some(mut a) = val.as_array_vec() {
                    a.reverse();
                    Ok(PerlValue::array(a))
                } else if let Some(s) = val.as_str() {
                    Ok(PerlValue::string(s.chars().rev().collect()))
                } else {
                    let s: String = val.to_string().chars().rev().collect();
                    Ok(PerlValue::string(s))
                }
            }

            // ── Parallel operations (rayon-powered) ──
            ExprKind::ParLinesExpr {
                path,
                callback,
                progress,
            } => self.eval_par_lines_expr(
                path.as_ref(),
                callback.as_ref(),
                progress.as_deref(),
                line,
            ),
            ExprKind::ParWalkExpr {
                path,
                callback,
                progress,
            } => self.eval_par_walk_expr(
                path.as_ref(),
                callback.as_ref(),
                progress.as_deref(),
                line,
            ),
            ExprKind::PwatchExpr { path, callback } => {
                self.eval_pwatch_expr(path.as_ref(), callback.as_ref(), line)
            }
            ExprKind::PMapExpr {
                block,
                list,
                progress,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();
                let pmap_progress = PmapProgress::new(show_progress, items.len());

                let results: Vec<PerlValue> = items
                    .into_par_iter()
                    .map(|item| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        local_interp
                            .scope
                            .restore_atomics(&atomic_arrays, &atomic_hashes);
                        local_interp.enable_parallel_guard();
                        let _ = local_interp.scope.set_scalar("_", item);
                        let val = match local_interp.exec_block(&block) {
                            Ok(val) => val,
                            Err(_) => PerlValue::UNDEF,
                        };
                        pmap_progress.tick();
                        val
                    })
                    .collect();
                pmap_progress.finish();
                Ok(PerlValue::array(results))
            }
            ExprKind::PMapChunkedExpr {
                chunk_size,
                block,
                list,
                progress,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
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

                let n_chunks = indexed_chunks.len();
                let pmap_progress = PmapProgress::new(show_progress, n_chunks);

                let mut chunk_results: Vec<(usize, Vec<PerlValue>)> = indexed_chunks
                    .into_par_iter()
                    .map(|(chunk_idx, chunk)| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        local_interp
                            .scope
                            .restore_atomics(&atomic_arrays, &atomic_hashes);
                        local_interp.enable_parallel_guard();
                        let mut out = Vec::with_capacity(chunk.len());
                        for item in chunk {
                            let _ = local_interp.scope.set_scalar("_", item);
                            match local_interp.exec_block(&block) {
                                Ok(val) => out.push(val),
                                Err(_) => out.push(PerlValue::UNDEF),
                            }
                        }
                        pmap_progress.tick();
                        (chunk_idx, out)
                    })
                    .collect();

                pmap_progress.finish();
                chunk_results.sort_by_key(|(i, _)| *i);
                let results: Vec<PerlValue> =
                    chunk_results.into_iter().flat_map(|(_, v)| v).collect();
                Ok(PerlValue::array(results))
            }
            ExprKind::PGrepExpr {
                block,
                list,
                progress,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();
                let pmap_progress = PmapProgress::new(show_progress, items.len());

                let results: Vec<PerlValue> = items
                    .into_par_iter()
                    .filter_map(|item| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        local_interp
                            .scope
                            .restore_atomics(&atomic_arrays, &atomic_hashes);
                        local_interp.enable_parallel_guard();
                        let _ = local_interp.scope.set_scalar("_", item.clone());
                        let keep = match local_interp.exec_block(&block) {
                            Ok(val) => val.is_true(),
                            Err(_) => false,
                        };
                        pmap_progress.tick();
                        if keep {
                            Some(item)
                        } else {
                            None
                        }
                    })
                    .collect();
                pmap_progress.finish();
                Ok(PerlValue::array(results))
            }
            ExprKind::PForExpr {
                block,
                list,
                progress,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();

                let pmap_progress = PmapProgress::new(show_progress, items.len());
                let first_err: Arc<Mutex<Option<PerlError>>> = Arc::new(Mutex::new(None));
                items.into_par_iter().for_each(|item| {
                    if first_err.lock().is_some() {
                        return;
                    }
                    let mut local_interp = Interpreter::new();
                    local_interp.subs = subs.clone();
                    local_interp.scope.restore_capture(&scope_capture);
                    local_interp
                        .scope
                        .restore_atomics(&atomic_arrays, &atomic_hashes);
                    local_interp.enable_parallel_guard();
                    let _ = local_interp.scope.set_scalar("_", item);
                    match local_interp.exec_block(&block) {
                        Ok(_) => {}
                        Err(e) => {
                            let pe = match e {
                                FlowOrError::Error(pe) => pe,
                                FlowOrError::Flow(_) => PerlError::runtime(
                                    "return/last/next/redo not supported inside pfor block",
                                    line,
                                ),
                            };
                            let mut g = first_err.lock();
                            if g.is_none() {
                                *g = Some(pe);
                            }
                        }
                    }
                    pmap_progress.tick();
                });
                pmap_progress.finish();
                if let Some(e) = first_err.lock().take() {
                    return Err(FlowOrError::Error(e));
                }
                Ok(PerlValue::UNDEF)
            }
            ExprKind::FanExpr {
                count,
                block,
                progress,
                capture,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let n = match count {
                    Some(c) => self.eval_expr(c)?.to_int().max(0) as usize,
                    None => self.parallel_thread_count(),
                };
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();

                let fan_progress = FanProgress::new(show_progress, n);
                if *capture {
                    if n == 0 {
                        return Ok(PerlValue::array(Vec::new()));
                    }
                    let pairs: Vec<(usize, ExecResult)> = (0..n)
                        .into_par_iter()
                        .map(|i| {
                            fan_progress.start_worker(i);
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.suppress_stdout = show_progress;
                            local_interp.scope.restore_capture(&scope_capture);
                            local_interp
                                .scope
                                .restore_atomics(&atomic_arrays, &atomic_hashes);
                            local_interp.enable_parallel_guard();
                            let _ = local_interp
                                .scope
                                .set_scalar("_", PerlValue::integer(i as i64));
                            crate::parallel_trace::fan_worker_set_index(Some(i as i64));
                            let res = local_interp.exec_block(&block);
                            crate::parallel_trace::fan_worker_set_index(None);
                            fan_progress.finish_worker(i);
                            (i, res)
                        })
                        .collect();
                    fan_progress.finish();
                    let mut pairs = pairs;
                    pairs.sort_by_key(|(i, _)| *i);
                    let mut out = Vec::with_capacity(n);
                    for (_, r) in pairs {
                        match r {
                            Ok(v) => out.push(v),
                            Err(e) => return Err(e),
                        }
                    }
                    return Ok(PerlValue::array(out));
                }
                let first_err: Arc<Mutex<Option<PerlError>>> = Arc::new(Mutex::new(None));
                (0..n).into_par_iter().for_each(|i| {
                    if first_err.lock().is_some() {
                        return;
                    }
                    fan_progress.start_worker(i);
                    let mut local_interp = Interpreter::new();
                    local_interp.subs = subs.clone();
                    local_interp.suppress_stdout = show_progress;
                    local_interp.scope.restore_capture(&scope_capture);
                    local_interp
                        .scope
                        .restore_atomics(&atomic_arrays, &atomic_hashes);
                    local_interp.enable_parallel_guard();
                    let _ = local_interp
                        .scope
                        .set_scalar("_", PerlValue::integer(i as i64));
                    crate::parallel_trace::fan_worker_set_index(Some(i as i64));
                    match local_interp.exec_block(&block) {
                        Ok(_) => {}
                        Err(e) => {
                            let pe = match e {
                                FlowOrError::Error(pe) => pe,
                                FlowOrError::Flow(_) => PerlError::runtime(
                                    "return/last/next/redo not supported inside fan block",
                                    line,
                                ),
                            };
                            let mut g = first_err.lock();
                            if g.is_none() {
                                *g = Some(pe);
                            }
                        }
                    }
                    crate::parallel_trace::fan_worker_set_index(None);
                    fan_progress.finish_worker(i);
                });
                fan_progress.finish();
                if let Some(e) = first_err.lock().take() {
                    return Err(FlowOrError::Error(e));
                }
                Ok(PerlValue::UNDEF)
            }
            ExprKind::AlgebraicMatch { subject, arms } => {
                self.eval_algebraic_match(subject, arms, line)
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
                Ok(PerlValue::float(ms))
            }
            ExprKind::Bench { body, times } => {
                let n = self.eval_expr(times)?.to_int();
                if n < 0 {
                    return Err(PerlError::runtime(
                        "bench: iteration count must be non-negative",
                        line,
                    )
                    .into());
                }
                self.run_bench_block(body, n as usize, line)
            }
            ExprKind::Await(expr) => {
                let v = self.eval_expr(expr)?;
                if let Some(t) = v.as_async_task() {
                    t.await_result().map_err(FlowOrError::from)
                } else {
                    Ok(v)
                }
            }
            ExprKind::Slurp(e) => {
                let path = self.eval_expr(e)?.to_string();
                std::fs::read_to_string(&path)
                    .map(PerlValue::string)
                    .map_err(|e| {
                        FlowOrError::Error(PerlError::runtime(format!("slurp: {}", e), line))
                    })
            }
            ExprKind::Capture(e) => {
                let cmd = self.eval_expr(e)?.to_string();
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .output()
                    .map_err(|e| {
                        FlowOrError::Error(PerlError::runtime(format!("capture: {}", e), line))
                    })?;
                self.record_child_exit_status(output.status);
                let exitcode = output.status.code().unwrap_or(-1) as i64;
                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                Ok(PerlValue::capture(Arc::new(CaptureResult {
                    stdout,
                    stderr,
                    exitcode,
                })))
            }
            ExprKind::FetchUrl(e) => {
                let url = self.eval_expr(e)?.to_string();
                ureq::get(&url)
                    .call()
                    .map_err(|e| {
                        FlowOrError::Error(PerlError::runtime(format!("fetch_url: {}", e), line))
                    })
                    .and_then(|r| {
                        r.into_string().map(PerlValue::string).map_err(|e| {
                            FlowOrError::Error(PerlError::runtime(
                                format!("fetch_url: {}", e),
                                line,
                            ))
                        })
                    })
            }
            ExprKind::Pchannel { capacity } => {
                if let Some(c) = capacity {
                    let n = self.eval_expr(c)?.to_int().max(1) as usize;
                    Ok(crate::pchannel::create_bounded_pair(n))
                } else {
                    Ok(crate::pchannel::create_pair())
                }
            }
            ExprKind::PSortExpr {
                cmp,
                list,
                progress,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let list_val = self.eval_expr(list)?;
                let mut items = list_val.to_list();
                let pmap_progress = PmapProgress::new(show_progress, 2);
                pmap_progress.tick();
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
                pmap_progress.tick();
                pmap_progress.finish();
                Ok(PerlValue::array(items))
            }

            ExprKind::ReduceExpr { block, list } => {
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                if items.is_empty() {
                    return Ok(PerlValue::UNDEF);
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
                        Err(_) => PerlValue::UNDEF,
                    };
                }
                Ok(acc)
            }

            ExprKind::PReduceExpr {
                block,
                list,
                progress,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                if items.is_empty() {
                    return Ok(PerlValue::UNDEF);
                }
                if items.len() == 1 {
                    return Ok(items.into_iter().next().unwrap());
                }
                let block = block.clone();
                let subs = self.subs.clone();
                let scope_capture = self.scope.capture();
                let pmap_progress = PmapProgress::new(show_progress, items.len());

                let result = items
                    .into_par_iter()
                    .map(|x| {
                        pmap_progress.tick();
                        x
                    })
                    .reduce_with(|a, b| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        let _ = local_interp.scope.set_scalar("a", a);
                        let _ = local_interp.scope.set_scalar("b", b);
                        match local_interp.exec_block(&block) {
                            Ok(val) => val,
                            Err(_) => PerlValue::UNDEF,
                        }
                    });
                pmap_progress.finish();
                Ok(result.unwrap_or(PerlValue::UNDEF))
            }

            ExprKind::PReduceInitExpr {
                init,
                block,
                list,
                progress,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let init_val = self.eval_expr(init)?;
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                if items.is_empty() {
                    return Ok(init_val);
                }
                let block = block.clone();
                let subs = self.subs.clone();
                let scope_capture = self.scope.capture();
                let cap: &[(String, PerlValue)] = scope_capture.as_slice();
                if items.len() == 1 {
                    return Ok(fold_preduce_init_step(
                        &subs,
                        cap,
                        &block,
                        preduce_init_fold_identity(&init_val),
                        items.into_iter().next().unwrap(),
                    ));
                }
                let pmap_progress = PmapProgress::new(show_progress, items.len());
                let result = items
                    .into_par_iter()
                    .fold(
                        || preduce_init_fold_identity(&init_val),
                        |acc, item| {
                            pmap_progress.tick();
                            fold_preduce_init_step(&subs, cap, &block, acc, item)
                        },
                    )
                    .reduce(
                        || preduce_init_fold_identity(&init_val),
                        |a, b| merge_preduce_init_partials(a, b, &block, &subs, cap),
                    );
                pmap_progress.finish();
                Ok(result)
            }

            ExprKind::PMapReduceExpr {
                map_block,
                reduce_block,
                list,
                progress,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                if items.is_empty() {
                    return Ok(PerlValue::UNDEF);
                }
                let map_block = map_block.clone();
                let reduce_block = reduce_block.clone();
                let subs = self.subs.clone();
                let scope_capture = self.scope.capture();
                if items.len() == 1 {
                    let mut local_interp = Interpreter::new();
                    local_interp.subs = subs.clone();
                    local_interp.scope.restore_capture(&scope_capture);
                    let _ = local_interp.scope.set_scalar("_", items[0].clone());
                    return match local_interp.exec_block_no_scope(&map_block) {
                        Ok(v) => Ok(v),
                        Err(_) => Ok(PerlValue::UNDEF),
                    };
                }
                let pmap_progress = PmapProgress::new(show_progress, items.len());
                let result = items
                    .into_par_iter()
                    .map(|item| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        let _ = local_interp.scope.set_scalar("_", item);
                        let val = match local_interp.exec_block_no_scope(&map_block) {
                            Ok(val) => val,
                            Err(_) => PerlValue::UNDEF,
                        };
                        pmap_progress.tick();
                        val
                    })
                    .reduce_with(|a, b| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        let _ = local_interp.scope.set_scalar("a", a);
                        let _ = local_interp.scope.set_scalar("b", b);
                        match local_interp.exec_block_no_scope(&reduce_block) {
                            Ok(val) => val,
                            Err(_) => PerlValue::UNDEF,
                        }
                    });
                pmap_progress.finish();
                Ok(result.unwrap_or(PerlValue::UNDEF))
            }

            ExprKind::PcacheExpr {
                block,
                list,
                progress,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let list_val = self.eval_expr(list)?;
                let items = list_val.to_list();
                let block = block.clone();
                let subs = self.subs.clone();
                let scope_capture = self.scope.capture();
                let cache = &*crate::pcache::GLOBAL_PCACHE;
                let pmap_progress = PmapProgress::new(show_progress, items.len());
                let results: Vec<PerlValue> = items
                    .into_par_iter()
                    .map(|item| {
                        let k = crate::pcache::cache_key(&item);
                        if let Some(v) = cache.get(&k) {
                            pmap_progress.tick();
                            return v.clone();
                        }
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        let _ = local_interp.scope.set_scalar("_", item.clone());
                        let val = match local_interp.exec_block_no_scope(&block) {
                            Ok(v) => v,
                            Err(_) => PerlValue::UNDEF,
                        };
                        cache.insert(k, val.clone());
                        pmap_progress.tick();
                        val
                    })
                    .collect();
                pmap_progress.finish();
                Ok(PerlValue::array(results))
            }

            ExprKind::PselectExpr { receivers, timeout } => {
                let mut rx_vals = Vec::with_capacity(receivers.len());
                for r in receivers {
                    rx_vals.push(self.eval_expr(r)?);
                }
                let dur = if let Some(t) = timeout.as_ref() {
                    Some(std::time::Duration::from_secs_f64(
                        self.eval_expr(t)?.to_number().max(0.0),
                    ))
                } else {
                    None
                };
                Ok(crate::pchannel::pselect_recv_with_optional_timeout(
                    &rx_vals, dur, line,
                )?)
            }

            // Array ops
            ExprKind::Push { array, values } => {
                self.eval_push_expr(array.as_ref(), values.as_slice(), line)
            }
            ExprKind::Pop(array) => self.eval_pop_expr(array.as_ref(), line),
            ExprKind::Shift(array) => self.eval_shift_expr(array.as_ref(), line),
            ExprKind::Unshift { array, values } => {
                self.eval_unshift_expr(array.as_ref(), values.as_slice(), line)
            }
            ExprKind::Splice {
                array,
                offset,
                length,
                replacement,
            } => self.eval_splice_expr(
                array.as_ref(),
                offset.as_deref(),
                length.as_deref(),
                replacement.as_slice(),
                line,
            ),
            ExprKind::Delete(expr) => self.eval_delete_operand(expr.as_ref(), line),
            ExprKind::Exists(expr) => self.eval_exists_operand(expr.as_ref(), line),
            ExprKind::Keys(expr) => self.eval_keys_expr(expr.as_ref(), line),
            ExprKind::Values(expr) => self.eval_values_expr(expr.as_ref(), line),
            ExprKind::Each(_) => {
                // Simplified: returns empty list (full iterator state would need more work)
                Ok(PerlValue::array(vec![]))
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
                Ok(if let Some(a) = val.as_array_vec() {
                    PerlValue::integer(a.len() as i64)
                } else if let Some(h) = val.as_hash_map() {
                    PerlValue::integer(h.len() as i64)
                } else if let Some(b) = val.as_bytes_arc() {
                    PerlValue::integer(b.len() as i64)
                } else {
                    PerlValue::integer(val.to_string().len() as i64)
                })
            }
            ExprKind::Substr {
                string,
                offset,
                length,
                replacement,
            } => self.eval_substr_expr(
                string.as_ref(),
                offset.as_ref(),
                length.as_deref(),
                replacement.as_deref(),
                line,
            ),
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
                Ok(PerlValue::integer(result))
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
                Ok(PerlValue::integer(result))
            }
            ExprKind::Sprintf { format, args } => {
                let fmt = self.eval_expr(format)?.to_string();
                let mut arg_vals = Vec::new();
                for a in args {
                    arg_vals.push(self.eval_expr(a)?);
                }
                let s = self.perl_sprintf_stringify(&fmt, &arg_vals, line)?;
                Ok(PerlValue::string(s))
            }
            ExprKind::JoinExpr { separator, list } => {
                let sep = self.eval_expr(separator)?.to_string();
                // Like Perl 5, arguments after the separator are evaluated in list context so
                // `join(",", uniq @x)` passes list context into `uniq`.
                let items = if let ExprKind::List(exprs) = &list.kind {
                    let saved = self.wantarray_kind;
                    self.wantarray_kind = WantarrayCtx::List;
                    let mut vals = Vec::new();
                    for e in exprs {
                        let v = self.eval_expr(e)?;
                        if let Some(items) = v.as_array_vec() {
                            vals.extend(items);
                        } else {
                            vals.push(v);
                        }
                    }
                    self.wantarray_kind = saved;
                    vals
                } else {
                    self.eval_expr(list)?.to_list()
                };
                let joined = items
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(&sep);
                Ok(PerlValue::string(joined))
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
                    re.splitn_strings(&s, lim)
                        .into_iter()
                        .map(PerlValue::string)
                        .collect()
                } else {
                    re.split_strings(&s)
                        .into_iter()
                        .map(PerlValue::string)
                        .collect()
                };
                Ok(PerlValue::array(parts))
            }

            // Numeric
            ExprKind::Abs(expr) => {
                let val = self.eval_expr(expr)?;
                if let Some(r) = self.try_overload_unary_dispatch("abs", &val, line) {
                    return r;
                }
                Ok(PerlValue::float(val.to_number().abs()))
            }
            ExprKind::Int(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::integer(val.to_number() as i64))
            }
            ExprKind::Sqrt(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::float(val.to_number().sqrt()))
            }
            ExprKind::Sin(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::float(val.to_number().sin()))
            }
            ExprKind::Cos(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::float(val.to_number().cos()))
            }
            ExprKind::Atan2 { y, x } => {
                let yv = self.eval_expr(y)?.to_number();
                let xv = self.eval_expr(x)?.to_number();
                Ok(PerlValue::float(yv.atan2(xv)))
            }
            ExprKind::Exp(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::float(val.to_number().exp()))
            }
            ExprKind::Log(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::float(val.to_number().ln()))
            }
            ExprKind::Rand(upper) => {
                let u = match upper {
                    Some(e) => self.eval_expr(e)?.to_number(),
                    None => 1.0,
                };
                Ok(PerlValue::float(self.perl_rand(u)))
            }
            ExprKind::Srand(seed) => {
                let s = match seed {
                    Some(e) => Some(self.eval_expr(e)?.to_number()),
                    None => None,
                };
                Ok(PerlValue::integer(self.perl_srand(s)))
            }
            ExprKind::Hex(expr) => {
                let val = self.eval_expr(expr)?.to_string();
                let clean = val.trim().trim_start_matches("0x").trim_start_matches("0X");
                let n = i64::from_str_radix(clean, 16).unwrap_or(0);
                Ok(PerlValue::integer(n))
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
                Ok(PerlValue::integer(n))
            }

            // Case
            ExprKind::Lc(expr) => Ok(PerlValue::string(
                self.eval_expr(expr)?.to_string().to_lowercase(),
            )),
            ExprKind::Uc(expr) => Ok(PerlValue::string(
                self.eval_expr(expr)?.to_string().to_uppercase(),
            )),
            ExprKind::Lcfirst(expr) => {
                let s = self.eval_expr(expr)?.to_string();
                let mut chars = s.chars();
                let result = match chars.next() {
                    Some(c) => c.to_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                };
                Ok(PerlValue::string(result))
            }
            ExprKind::Ucfirst(expr) => {
                let s = self.eval_expr(expr)?.to_string();
                let mut chars = s.chars();
                let result = match chars.next() {
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                };
                Ok(PerlValue::string(result))
            }
            ExprKind::Fc(expr) => Ok(PerlValue::string(default_case_fold_str(
                &self.eval_expr(expr)?.to_string(),
            ))),
            ExprKind::Crypt { plaintext, salt } => {
                let p = self.eval_expr(plaintext)?.to_string();
                let sl = self.eval_expr(salt)?.to_string();
                Ok(PerlValue::string(perl_crypt(&p, &sl)))
            }
            ExprKind::Pos(e) => {
                let key = match e {
                    None => "_".to_string(),
                    Some(expr) => match &expr.kind {
                        ExprKind::ScalarVar(n) => n.clone(),
                        _ => self.eval_expr(expr)?.to_string(),
                    },
                };
                Ok(self
                    .regex_pos
                    .get(&key)
                    .copied()
                    .flatten()
                    .map(|p| PerlValue::integer(p as i64))
                    .unwrap_or(PerlValue::UNDEF))
            }
            ExprKind::Study(expr) => {
                let s = self.eval_expr(expr)?.to_string();
                Ok(PerlValue::integer(s.len() as i64))
            }

            // Type
            ExprKind::Defined(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::integer(if val.is_undef() { 0 } else { 1 }))
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
                Ok(PerlValue::string(
                    char::from_u32(n).map(|c| c.to_string()).unwrap_or_default(),
                ))
            }
            ExprKind::Ord(expr) => {
                let s = self.eval_expr(expr)?.to_string();
                Ok(PerlValue::integer(
                    s.chars().next().map(|c| c as i64).unwrap_or(0),
                ))
            }

            // I/O
            ExprKind::Open { handle, mode, file } => {
                let handle_s = self.eval_expr(handle)?.to_string();
                let handle_name = self.resolve_io_handle_name(&handle_s);
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
                let s = self.eval_expr(expr)?.to_string();
                let name = self.resolve_io_handle_name(&s);
                self.close_builtin_execute(name).map_err(Into::into)
            }
            ExprKind::ReadLine(handle) => self
                .readline_builtin_execute(handle.as_deref())
                .map_err(Into::into),
            ExprKind::Eof(expr) => {
                if let Some(e) = expr {
                    let name = self.eval_expr(e)?.to_string();
                    let at_eof = !self.has_input_handle(&name);
                    Ok(PerlValue::integer(if at_eof { 1 } else { 0 }))
                } else {
                    Ok(PerlValue::integer(0))
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
                Ok(PerlValue::integer(if result { 1 } else { 0 }))
            }

            // System
            ExprKind::System(args) => {
                let mut cmd_args = Vec::new();
                for a in args {
                    cmd_args.push(self.eval_expr(a)?.to_string());
                }
                if cmd_args.is_empty() {
                    return Ok(PerlValue::integer(-1));
                }
                let status = Command::new("sh")
                    .arg("-c")
                    .arg(cmd_args.join(" "))
                    .status();
                match status {
                    Ok(s) => {
                        self.record_child_exit_status(s);
                        Ok(PerlValue::integer(s.code().unwrap_or(-1) as i64))
                    }
                    Err(e) => {
                        self.apply_io_error_to_errno(&e);
                        Ok(PerlValue::integer(-1))
                    }
                }
            }
            ExprKind::Exec(args) => {
                let mut cmd_args = Vec::new();
                for a in args {
                    cmd_args.push(self.eval_expr(a)?.to_string());
                }
                if cmd_args.is_empty() {
                    return Ok(PerlValue::integer(-1));
                }
                let status = Command::new("sh")
                    .arg("-c")
                    .arg(cmd_args.join(" "))
                    .status();
                match status {
                    Ok(s) => std::process::exit(s.code().unwrap_or(-1)),
                    Err(e) => {
                        self.apply_io_error_to_errno(&e);
                        Ok(PerlValue::integer(-1))
                    }
                }
            }
            ExprKind::Eval(expr) => {
                self.eval_nesting += 1;
                let out = match &expr.kind {
                    ExprKind::CodeRef { body, .. } => match self.exec_block(body) {
                        Ok(v) => {
                            self.clear_eval_error();
                            Ok(v)
                        }
                        Err(FlowOrError::Error(e)) => {
                            self.set_eval_error(e.to_string());
                            Ok(PerlValue::UNDEF)
                        }
                        Err(FlowOrError::Flow(f)) => Err(FlowOrError::Flow(f)),
                    },
                    _ => {
                        let code = self.eval_expr(expr)?.to_string();
                        // Parse and execute the string as Perl code
                        match crate::parse_and_run_string(&code, self) {
                            Ok(v) => {
                                self.clear_eval_error();
                                Ok(v)
                            }
                            Err(e) => {
                                self.set_eval_error(e.to_string());
                                Ok(PerlValue::UNDEF)
                            }
                        }
                    }
                };
                self.eval_nesting -= 1;
                out
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
                                    self.set_eval_error(e.to_string());
                                    Ok(PerlValue::UNDEF)
                                }
                            },
                            Err(e) => {
                                self.apply_io_error_to_errno(&e);
                                Ok(PerlValue::UNDEF)
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
                    Ok(_) => Ok(PerlValue::integer(1)),
                    Err(e) => {
                        self.apply_io_error_to_errno(&e);
                        Ok(PerlValue::integer(0))
                    }
                }
            }
            ExprKind::Mkdir { path, mode: _ } => {
                let p = self.eval_expr(path)?.to_string();
                match std::fs::create_dir(&p) {
                    Ok(_) => Ok(PerlValue::integer(1)),
                    Err(e) => {
                        self.apply_io_error_to_errno(&e);
                        Ok(PerlValue::integer(0))
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
                Ok(PerlValue::integer(count))
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
                Ok(PerlValue::integer(crate::perl_fs::chmod_paths(
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
                Ok(PerlValue::integer(crate::perl_fs::chown_paths(
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
            ExprKind::GlobPar { args, progress } => {
                let mut pats = Vec::new();
                for a in args {
                    pats.push(self.eval_expr(a)?.to_string());
                }
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                if show_progress {
                    Ok(crate::perl_fs::glob_par_patterns_with_progress(&pats, true))
                } else {
                    Ok(crate::perl_fs::glob_par_patterns(&pats))
                }
            }
            ExprKind::ParSed { args, progress } => {
                let has_progress = progress.is_some();
                let mut vals: Vec<PerlValue> = Vec::new();
                for a in args {
                    vals.push(self.eval_expr(a)?);
                }
                if let Some(p) = progress {
                    vals.push(self.eval_expr(p.as_ref())?);
                }
                Ok(self.builtin_par_sed(&vals, line, has_progress)?)
            }
            ExprKind::Bless { ref_expr, class } => {
                let val = self.eval_expr(ref_expr)?;
                let class_name = if let Some(c) = class {
                    self.eval_expr(c)?.to_string()
                } else {
                    self.scope.get_scalar("__PACKAGE__").to_string()
                };
                Ok(PerlValue::blessed(Arc::new(crate::value::BlessedRef {
                    class: class_name,
                    data: RwLock::new(val),
                })))
            }
            ExprKind::Caller(_) => {
                // Simplified: return package, file, line
                Ok(PerlValue::array(vec![
                    PerlValue::string("main".into()),
                    PerlValue::string(self.file.clone()),
                    PerlValue::integer(line as i64),
                ]))
            }
            ExprKind::Wantarray => Ok(match self.wantarray_kind {
                WantarrayCtx::Void => PerlValue::UNDEF,
                WantarrayCtx::Scalar => PerlValue::integer(0),
                WantarrayCtx::List => PerlValue::integer(1),
            }),

            ExprKind::List(exprs) => {
                let mut vals = Vec::new();
                for e in exprs {
                    let v = self.eval_expr(e)?;
                    if let Some(items) = v.as_array_vec() {
                        vals.extend(items);
                    } else {
                        vals.push(v);
                    }
                }
                if vals.len() == 1 {
                    Ok(vals.pop().unwrap())
                } else {
                    Ok(PerlValue::array(vals))
                }
            }

            // Postfix modifiers
            ExprKind::PostfixIf { expr, condition } => {
                if self.eval_postfix_condition(condition)? {
                    self.eval_expr(expr)
                } else {
                    Ok(PerlValue::UNDEF)
                }
            }
            ExprKind::PostfixUnless { expr, condition } => {
                if !self.eval_postfix_condition(condition)? {
                    self.eval_expr(expr)
                } else {
                    Ok(PerlValue::UNDEF)
                }
            }
            ExprKind::PostfixWhile { expr, condition } => {
                // `do { ... } while (COND)` — body runs before the first condition check.
                // Parsed as PostfixWhile(Do(CodeRef), cond), not plain postfix-while.
                let is_do_block = matches!(
                    &expr.kind,
                    ExprKind::Do(inner) if matches!(inner.kind, ExprKind::CodeRef { .. })
                );
                let mut last = PerlValue::UNDEF;
                if is_do_block {
                    loop {
                        last = self.eval_expr(expr)?;
                        if !self.eval_postfix_condition(condition)? {
                            break;
                        }
                    }
                } else {
                    loop {
                        if !self.eval_postfix_condition(condition)? {
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
                let mut last = PerlValue::UNDEF;
                if is_do_block {
                    loop {
                        last = self.eval_expr(expr)?;
                        if self.eval_postfix_condition(condition)? {
                            break;
                        }
                    }
                } else {
                    loop {
                        if self.eval_postfix_condition(condition)? {
                            break;
                        }
                        last = self.eval_expr(expr)?;
                    }
                }
                Ok(last)
            }
            ExprKind::PostfixForeach { expr, list } => {
                let items = self.eval_expr(list)?.to_list();
                let mut last = PerlValue::UNDEF;
                for item in items {
                    let _ = self.scope.set_scalar("_", item);
                    last = self.eval_expr(expr)?;
                }
                Ok(last)
            }
        }
    }

    // ── Helpers ──

    fn overload_key_for_binop(op: BinOp) -> Option<&'static str> {
        match op {
            BinOp::Add => Some("+"),
            BinOp::Sub => Some("-"),
            BinOp::Mul => Some("*"),
            BinOp::Div => Some("/"),
            BinOp::Mod => Some("%"),
            BinOp::Pow => Some("**"),
            BinOp::Concat => Some("."),
            BinOp::StrEq => Some("eq"),
            BinOp::NumEq => Some("=="),
            BinOp::StrNe => Some("ne"),
            BinOp::NumNe => Some("!="),
            BinOp::StrLt => Some("lt"),
            BinOp::StrGt => Some("gt"),
            BinOp::StrLe => Some("le"),
            BinOp::StrGe => Some("ge"),
            BinOp::NumLt => Some("<"),
            BinOp::NumGt => Some(">"),
            BinOp::NumLe => Some("<="),
            BinOp::NumGe => Some(">="),
            BinOp::Spaceship => Some("<=>"),
            BinOp::StrCmp => Some("cmp"),
            _ => None,
        }
    }

    /// Perl `use overload '""' => ...` — key is `""` (empty) or `""` (two `"` chars from `'""'`).
    fn overload_stringify_method(map: &HashMap<String, String>) -> Option<&String> {
        map.get("").or_else(|| map.get("\"\""))
    }

    /// String context for blessed objects with `overload '""'`.
    pub(crate) fn stringify_value(
        &mut self,
        v: PerlValue,
        line: usize,
    ) -> Result<String, FlowOrError> {
        if let Some(r) = self.try_overload_stringify(&v, line) {
            let pv = r?;
            return Ok(pv.to_string());
        }
        Ok(v.to_string())
    }

    /// Like Perl `sprintf`, but `%s` uses [`stringify_value`] so `overload ""` applies.
    pub(crate) fn perl_sprintf_stringify(
        &mut self,
        fmt: &str,
        args: &[PerlValue],
        line: usize,
    ) -> Result<String, FlowOrError> {
        perl_sprintf_format_with(fmt, args, |v| self.stringify_value(v.clone(), line))
    }

    /// Expand a compiled [`crate::format::FormatTemplate`] using current expression evaluation.
    pub(crate) fn render_format_template(
        &mut self,
        tmpl: &crate::format::FormatTemplate,
        line: usize,
    ) -> Result<String, FlowOrError> {
        use crate::format::{FormatRecord, PictureSegment};
        let mut buf = String::new();
        for rec in &tmpl.records {
            match rec {
                FormatRecord::Literal(s) => {
                    buf.push_str(s);
                    buf.push('\n');
                }
                FormatRecord::Picture { segments, exprs } => {
                    let mut vals: Vec<String> = Vec::new();
                    for e in exprs {
                        let v = self.eval_expr(e)?;
                        vals.push(self.stringify_value(v, line)?);
                    }
                    let mut vi = 0usize;
                    let mut line_out = String::new();
                    for seg in segments {
                        match seg {
                            PictureSegment::Literal(t) => line_out.push_str(t),
                            PictureSegment::Field {
                                width,
                                align,
                                kind: _,
                            } => {
                                let s = vals.get(vi).map(|s| s.as_str()).unwrap_or("");
                                vi += 1;
                                line_out.push_str(&crate::format::pad_field(s, *width, *align));
                            }
                        }
                    }
                    buf.push_str(&line_out);
                    buf.push('\n');
                }
            }
        }
        Ok(buf)
    }

    /// `write` — output one record using `$~` format name in the current package (subset of Perl).
    pub(crate) fn write_format_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        if !args.is_empty() {
            return Err(PerlError::runtime(
                "write: filehandle argument not implemented (use selected STDOUT)",
                line,
            ));
        }
        let pkg = self.current_package();
        let mut fmt_name = self.scope.get_scalar("~").to_string();
        if fmt_name.is_empty() {
            fmt_name = "STDOUT".to_string();
        }
        let key = format!("{}::{}", pkg, fmt_name);
        let tmpl = self
            .format_templates
            .get(&key)
            .map(Arc::clone)
            .ok_or_else(|| {
                PerlError::runtime(
                    format!("Unknown format `{}` in package `{}`", fmt_name, pkg),
                    line,
                )
            })?;
        let out = self
            .render_format_template(&tmpl, line)
            .map_err(|e| match e {
                FlowOrError::Error(e) => e,
                FlowOrError::Flow(_) => PerlError::runtime("write: unexpected control flow", line),
            })?;
        print!("{}", out);
        if self.output_autoflush {
            let _ = IoWrite::flush(&mut io::stdout());
        }
        Ok(PerlValue::integer(1))
    }

    fn try_overload_stringify(&mut self, v: &PerlValue, line: usize) -> Option<ExecResult> {
        let br = v.as_blessed_ref()?;
        let class = br.class.clone();
        let map = self.overload_table.get(&class)?;
        let sub_short = Self::overload_stringify_method(map)?;
        let fq = format!("{}::{}", class, sub_short);
        let sub = self.subs.get(&fq)?.clone();
        Some(self.call_sub(&sub, vec![v.clone()], WantarrayCtx::Scalar, line))
    }

    fn try_overload_binop(
        &mut self,
        op: BinOp,
        lv: &PerlValue,
        rv: &PerlValue,
        line: usize,
    ) -> Option<ExecResult> {
        let key = Self::overload_key_for_binop(op)?;
        let (class, invocant, other) = if let Some(br) = lv.as_blessed_ref() {
            (br.class.clone(), lv.clone(), rv.clone())
        } else if let Some(br) = rv.as_blessed_ref() {
            (br.class.clone(), rv.clone(), lv.clone())
        } else {
            return None;
        };
        let map = self.overload_table.get(&class)?;
        let sub_short = if let Some(s) = map.get(key) {
            s.clone()
        } else if let Some(nm) = map.get("nomethod") {
            let fq = format!("{}::{}", class, nm);
            let sub = self.subs.get(&fq)?.clone();
            return Some(self.call_sub(
                &sub,
                vec![invocant, other, PerlValue::string(key.to_string())],
                WantarrayCtx::Scalar,
                line,
            ));
        } else {
            return None;
        };
        let fq = format!("{}::{}", class, sub_short);
        let sub = self.subs.get(&fq)?.clone();
        Some(self.call_sub(&sub, vec![invocant, other], WantarrayCtx::Scalar, line))
    }

    /// Unary overload: keys `neg`, `bool`, `abs`, `0+`, … — or `nomethod` with `(invocant, op_key)`.
    fn try_overload_unary_dispatch(
        &mut self,
        op_key: &str,
        val: &PerlValue,
        line: usize,
    ) -> Option<ExecResult> {
        let br = val.as_blessed_ref()?;
        let class = br.class.clone();
        let map = self.overload_table.get(&class)?;
        if let Some(s) = map.get(op_key) {
            let fq = format!("{}::{}", class, s);
            let sub = self.subs.get(&fq)?.clone();
            return Some(self.call_sub(&sub, vec![val.clone()], WantarrayCtx::Scalar, line));
        }
        if let Some(nm) = map.get("nomethod") {
            let fq = format!("{}::{}", class, nm);
            let sub = self.subs.get(&fq)?.clone();
            return Some(self.call_sub(
                &sub,
                vec![val.clone(), PerlValue::string(op_key.to_string())],
                WantarrayCtx::Scalar,
                line,
            ));
        }
        None
    }

    #[inline]
    fn eval_binop(
        &mut self,
        op: BinOp,
        lv: &PerlValue,
        rv: &PerlValue,
        _line: usize,
    ) -> ExecResult {
        Ok(match op {
            // ── Integer fast paths: avoid f64 conversion when both operands are i64 ──
            BinOp::Add => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(a.wrapping_add(b))
                } else {
                    PerlValue::float(lv.to_number() + rv.to_number())
                }
            }
            BinOp::Sub => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(a.wrapping_sub(b))
                } else {
                    PerlValue::float(lv.to_number() - rv.to_number())
                }
            }
            BinOp::Mul => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(a.wrapping_mul(b))
                } else {
                    PerlValue::float(lv.to_number() * rv.to_number())
                }
            }
            BinOp::Div => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    if b == 0 {
                        return Err(PerlError::runtime("Illegal division by zero", _line).into());
                    }
                    if a % b == 0 {
                        PerlValue::integer(a / b)
                    } else {
                        PerlValue::float(a as f64 / b as f64)
                    }
                } else {
                    let d = rv.to_number();
                    if d == 0.0 {
                        return Err(PerlError::runtime("Illegal division by zero", _line).into());
                    }
                    PerlValue::float(lv.to_number() / d)
                }
            }
            BinOp::Mod => {
                let d = rv.to_int();
                if d == 0 {
                    return Err(PerlError::runtime("Illegal modulus zero", _line).into());
                }
                PerlValue::integer(lv.to_int() % d)
            }
            BinOp::Pow => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    if (0..=63).contains(&b) {
                        PerlValue::integer(a.wrapping_pow(b as u32))
                    } else {
                        PerlValue::float(lv.to_number().powf(rv.to_number()))
                    }
                } else {
                    PerlValue::float(lv.to_number().powf(rv.to_number()))
                }
            }
            BinOp::Concat => {
                // Optimized: avoid allocating rv.to_string() by appending directly
                let mut s = lv.to_string();
                rv.append_to(&mut s);
                PerlValue::string(s)
            }
            BinOp::NumEq => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(if a == b { 1 } else { 0 })
                } else {
                    PerlValue::integer(if lv.to_number() == rv.to_number() {
                        1
                    } else {
                        0
                    })
                }
            }
            BinOp::NumNe => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(if a != b { 1 } else { 0 })
                } else {
                    PerlValue::integer(if lv.to_number() != rv.to_number() {
                        1
                    } else {
                        0
                    })
                }
            }
            BinOp::NumLt => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(if a < b { 1 } else { 0 })
                } else {
                    PerlValue::integer(if lv.to_number() < rv.to_number() {
                        1
                    } else {
                        0
                    })
                }
            }
            BinOp::NumGt => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(if a > b { 1 } else { 0 })
                } else {
                    PerlValue::integer(if lv.to_number() > rv.to_number() {
                        1
                    } else {
                        0
                    })
                }
            }
            BinOp::NumLe => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(if a <= b { 1 } else { 0 })
                } else {
                    PerlValue::integer(if lv.to_number() <= rv.to_number() {
                        1
                    } else {
                        0
                    })
                }
            }
            BinOp::NumGe => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(if a >= b { 1 } else { 0 })
                } else {
                    PerlValue::integer(if lv.to_number() >= rv.to_number() {
                        1
                    } else {
                        0
                    })
                }
            }
            BinOp::Spaceship => {
                if let (Some(a), Some(b)) = (lv.as_integer(), rv.as_integer()) {
                    PerlValue::integer(if a < b {
                        -1
                    } else if a > b {
                        1
                    } else {
                        0
                    })
                } else {
                    let a = lv.to_number();
                    let b = rv.to_number();
                    PerlValue::integer(if a < b {
                        -1
                    } else if a > b {
                        1
                    } else {
                        0
                    })
                }
            }
            BinOp::StrEq => PerlValue::integer(if lv.to_string() == rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrNe => PerlValue::integer(if lv.to_string() != rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrLt => PerlValue::integer(if lv.to_string() < rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrGt => PerlValue::integer(if lv.to_string() > rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrLe => PerlValue::integer(if lv.to_string() <= rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrGe => PerlValue::integer(if lv.to_string() >= rv.to_string() {
                1
            } else {
                0
            }),
            BinOp::StrCmp => {
                let cmp = lv.to_string().cmp(&rv.to_string());
                PerlValue::integer(match cmp {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Greater => 1,
                    std::cmp::Ordering::Equal => 0,
                })
            }
            BinOp::BitAnd => {
                if let Some(s) = crate::value::set_intersection(lv, rv) {
                    s
                } else {
                    PerlValue::integer(lv.to_int() & rv.to_int())
                }
            }
            BinOp::BitOr => {
                if let Some(s) = crate::value::set_union(lv, rv) {
                    s
                } else {
                    PerlValue::integer(lv.to_int() | rv.to_int())
                }
            }
            BinOp::BitXor => PerlValue::integer(lv.to_int() ^ rv.to_int()),
            BinOp::ShiftLeft => PerlValue::integer(lv.to_int() << rv.to_int()),
            BinOp::ShiftRight => PerlValue::integer(lv.to_int() >> rv.to_int()),
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
                    return Err(FlowOrError::Error(PerlError::runtime(
                        format!("Modification of a frozen value: ${}", name),
                        target.line,
                    )));
                }
                if let Some(obj) = self.tied_scalars.get(name).cloned() {
                    let class = obj
                        .as_blessed_ref()
                        .map(|b| b.class.clone())
                        .unwrap_or_default();
                    let full = format!("{}::STORE", class);
                    if let Some(sub) = self.subs.get(&full).cloned() {
                        let arg_vals = vec![obj, val];
                        return match self.call_sub(
                            &sub,
                            arg_vals,
                            WantarrayCtx::Scalar,
                            target.line,
                        ) {
                            Ok(_) => Ok(PerlValue::UNDEF),
                            Err(FlowOrError::Flow(_)) => Ok(PerlValue::UNDEF),
                            Err(FlowOrError::Error(e)) => Err(FlowOrError::Error(e)),
                        };
                    }
                }
                self.set_special_var(name, &val)
                    .map_err(|e| FlowOrError::Error(e.at_line(target.line)))?;
                Ok(PerlValue::UNDEF)
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
                self.scope.set_array(name, val.to_list())?;
                Ok(PerlValue::UNDEF)
            }
            ExprKind::HashVar(name) => {
                if self.strict_vars && !name.contains("::") && !self.scope.hash_binding_exists(name)
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
                self.scope.set_hash(name, map)?;
                Ok(PerlValue::UNDEF)
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
                let aname = self.stash_array_name_for_package(array);
                if let Some(obj) = self.tied_arrays.get(&aname).cloned() {
                    let class = obj
                        .as_blessed_ref()
                        .map(|b| b.class.clone())
                        .unwrap_or_default();
                    let full = format!("{}::STORE", class);
                    if let Some(sub) = self.subs.get(&full).cloned() {
                        let arg_vals = vec![obj, PerlValue::integer(idx), val];
                        return match self.call_sub(
                            &sub,
                            arg_vals,
                            WantarrayCtx::Scalar,
                            target.line,
                        ) {
                            Ok(_) => Ok(PerlValue::UNDEF),
                            Err(FlowOrError::Flow(_)) => Ok(PerlValue::UNDEF),
                            Err(FlowOrError::Error(e)) => Err(FlowOrError::Error(e)),
                        };
                    }
                }
                self.scope.set_array_element(&aname, idx, val)?;
                Ok(PerlValue::UNDEF)
            }
            ExprKind::HashElement { hash, key } => {
                if self.strict_vars && !hash.contains("::") && !self.scope.hash_binding_exists(hash)
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
                if let Some(obj) = self.tied_hashes.get(hash).cloned() {
                    let class = obj
                        .as_blessed_ref()
                        .map(|b| b.class.clone())
                        .unwrap_or_default();
                    let full = format!("{}::STORE", class);
                    if let Some(sub) = self.subs.get(&full).cloned() {
                        let arg_vals = vec![obj, PerlValue::string(k), val];
                        return match self.call_sub(
                            &sub,
                            arg_vals,
                            WantarrayCtx::Scalar,
                            target.line,
                        ) {
                            Ok(_) => Ok(PerlValue::UNDEF),
                            Err(FlowOrError::Flow(_)) => Ok(PerlValue::UNDEF),
                            Err(FlowOrError::Error(e)) => Err(FlowOrError::Error(e)),
                        };
                    }
                }
                self.scope.set_hash_element(hash, &k, val)?;
                Ok(PerlValue::UNDEF)
            }
            ExprKind::Typeglob(name) => {
                if let Some(sub) = val.as_code_ref() {
                    let lhs_sub = self.qualify_typeglob_sub_key(name);
                    self.subs.insert(lhs_sub, sub);
                    return Ok(PerlValue::UNDEF);
                }
                Err(PerlError::runtime(
                    "typeglob assignment requires a subroutine reference (e.g. *foo = \\&bar) or another typeglob (*foo = *bar)",
                    target.line,
                )
                .into())
            }
            ExprKind::ArrowDeref {
                expr,
                index,
                kind: DerefKind::Hash,
            } => {
                let key = self.eval_expr(index)?.to_string();
                let container = self.eval_expr(expr)?;
                if let Some(b) = container.as_blessed_ref() {
                    let mut data = b.data.write();
                    if let Some(r) = data.as_hash_ref() {
                        r.write().insert(key, val);
                        return Ok(PerlValue::UNDEF);
                    }
                    if let Some(mut map) = data.as_hash_map() {
                        map.insert(key, val);
                        *data = PerlValue::hash(map);
                        return Ok(PerlValue::UNDEF);
                    }
                    return Err(PerlError::runtime(
                        "Can't assign into non-hash blessed ref",
                        target.line,
                    )
                    .into());
                }
                if let Some(r) = container.as_hash_ref() {
                    r.write().insert(key, val);
                    return Ok(PerlValue::UNDEF);
                }
                Err(PerlError::runtime(
                    "Can't assign to arrow hash deref on non-hash(-ref)",
                    target.line,
                )
                .into())
            }
            _ => Ok(PerlValue::UNDEF),
        }
    }

    /// True when [`get_special_var`] must run instead of [`Scope::get_scalar`].
    pub(crate) fn is_special_scalar_name_for_get(name: &str) -> bool {
        name.starts_with('^')
            || matches!(
                name,
                "$$" | "0"
                    | "!"
                    | "@"
                    | "/"
                    | "\\"
                    | ","
                    | "."
                    | "]"
                    | ";"
                    | "ARGV"
                    | "^I"
                    | "^D"
                    | "^P"
                    | "^S"
                    | "^W"
                    | "^O"
                    | "^T"
                    | "^V"
                    | "^E"
                    | "^H"
                    | "^WARNING_BITS"
                    | "^GLOBAL_PHASE"
                    | "^MATCH"
                    | "^PREMATCH"
                    | "^POSTMATCH"
                    | "^LAST_SUBMATCH_RESULT"
                    | "<"
                    | ">"
                    | "("
                    | ")"
                    | "?"
                    | "|"
                    | "\""
                    | "+"
                    | "%"
                    | "="
                    | "-"
                    | ":"
                    | "*"
                    | "INC"
            )
    }

    /// Map English long names (`ARG` → [`crate::english::scalar_alias`]) when [`Self::english_enabled`],
    /// except for names registered in [`Self::english_lexical_scalars`] (lexical `my`/`our`/…).
    #[inline]
    pub(crate) fn english_scalar_name<'a>(&self, name: &'a str) -> &'a str {
        if !self.english_enabled {
            return name;
        }
        if self
            .english_lexical_scalars
            .iter()
            .any(|s| s.contains(name))
        {
            return name;
        }
        if let Some(short) = crate::english::scalar_alias(name) {
            return short;
        }
        name
    }

    /// True when [`set_special_var`] must run instead of [`Scope::set_scalar`].
    pub(crate) fn is_special_scalar_name_for_set(name: &str) -> bool {
        name.starts_with('^')
            || matches!(
                name,
                "0" | "/"
                    | "\\"
                    | ","
                    | ";"
                    | "\""
                    | "%"
                    | "="
                    | "-"
                    | ":"
                    | "*"
                    | "INC"
                    | "^I"
                    | "^D"
                    | "^P"
                    | "^W"
                    | "^H"
                    | "^WARNING_BITS"
                    | "$$"
                    | "]"
                    | "^S"
                    | "ARGV"
                    | "|"
                    | "+"
                    | "?"
                    | "!"
                    | "@"
            )
    }

    pub(crate) fn get_special_var(&self, name: &str) -> PerlValue {
        let name = self.english_scalar_name(name);
        match name {
            "$$" => PerlValue::integer(std::process::id() as i64),
            "_" => self.scope.get_scalar("_"),
            "^MATCH" => PerlValue::string(self.last_match.clone()),
            "^PREMATCH" => PerlValue::string(self.prematch.clone()),
            "^POSTMATCH" => PerlValue::string(self.postmatch.clone()),
            "^LAST_SUBMATCH_RESULT" => PerlValue::string(self.last_paren_match.clone()),
            "0" => PerlValue::string(self.program_name.clone()),
            "!" => PerlValue::errno_dual(self.errno_code, self.errno.clone()),
            "@" => PerlValue::errno_dual(self.eval_error_code, self.eval_error.clone()),
            "/" => PerlValue::string(self.irs.clone()),
            "\\" => PerlValue::string(self.ors.clone()),
            "," => PerlValue::string(self.ofs.clone()),
            "." => {
                if self.last_readline_handle.is_empty() {
                    PerlValue::integer(self.line_number)
                } else {
                    PerlValue::integer(
                        *self
                            .handle_line_numbers
                            .get(&self.last_readline_handle)
                            .unwrap_or(&0),
                    )
                }
            }
            "]" => PerlValue::float(perl_bracket_version()),
            ";" => PerlValue::string(self.subscript_sep.clone()),
            "ARGV" => PerlValue::string(self.argv_current_file.clone()),
            "^I" => PerlValue::string(self.inplace_edit.clone()),
            "^D" => PerlValue::integer(self.debug_flags),
            "^P" => PerlValue::integer(self.perl_debug_flags),
            "^S" => PerlValue::integer(if self.eval_nesting > 0 { 1 } else { 0 }),
            "^W" => PerlValue::integer(if self.warnings { 1 } else { 0 }),
            "^O" => PerlValue::string(perl_osname()),
            "^T" => PerlValue::integer(self.script_start_time),
            "^V" => PerlValue::string(perl_version_v_string()),
            "^E" => PerlValue::string(extended_os_error_string()),
            "^H" => PerlValue::integer(self.compile_hints),
            "^WARNING_BITS" => PerlValue::integer(self.warning_bits),
            "^GLOBAL_PHASE" => PerlValue::string(self.global_phase.clone()),
            "<" | ">" => PerlValue::integer(unix_id_for_special(name)),
            "(" | ")" => PerlValue::string(unix_group_list_for_special(name)),
            "?" => PerlValue::integer(self.child_exit_status),
            "|" => PerlValue::integer(if self.output_autoflush { 1 } else { 0 }),
            "\"" => PerlValue::string(self.list_separator.clone()),
            "+" => PerlValue::string(self.last_paren_match.clone()),
            "%" => PerlValue::integer(self.format_page_number),
            "=" => PerlValue::integer(self.format_lines_per_page),
            "-" => PerlValue::integer(self.format_lines_left),
            ":" => PerlValue::string(self.format_line_break_chars.clone()),
            "*" => PerlValue::integer(if self.multiline_match { 1 } else { 0 }),
            "^" => PerlValue::string(self.format_top_name.clone()),
            "INC" => PerlValue::integer(self.inc_hook_index),
            "^A" => PerlValue::string(self.accumulator_format.clone()),
            "^C" => PerlValue::integer(if self.sigint_pending_caret.replace(false) {
                1
            } else {
                0
            }),
            "^F" => PerlValue::integer(self.max_system_fd),
            "^L" => PerlValue::string(self.formfeed_string.clone()),
            "^M" => PerlValue::string(self.emergency_memory.clone()),
            "^N" => PerlValue::string(self.last_subpattern_name.clone()),
            "^X" => PerlValue::string(self.executable_path.clone()),
            // perlvar ${^…} — stubs with sane defaults where Perl exposes constants.
            "^TAINT" | "^TAINTED" => PerlValue::integer(0),
            "^UNICODE" => PerlValue::integer(if self.utf8_pragma { 1 } else { 0 }),
            "^UTF8LOCALE" => PerlValue::integer(0),
            "^UTF8CACHE" => PerlValue::integer(-1),
            _ if name.starts_with('^') && name.len() > 1 => self
                .special_caret_scalars
                .get(name)
                .cloned()
                .unwrap_or(PerlValue::UNDEF),
            _ => self.scope.get_scalar(name),
        }
    }

    pub(crate) fn set_special_var(&mut self, name: &str, val: &PerlValue) -> Result<(), PerlError> {
        let name = self.english_scalar_name(name);
        match name {
            "!" => {
                let code = val.to_int() as i32;
                self.errno_code = code;
                self.errno = if code == 0 {
                    String::new()
                } else {
                    std::io::Error::from_raw_os_error(code).to_string()
                };
            }
            "@" => {
                if let Some((code, msg)) = val.errno_dual_parts() {
                    self.eval_error_code = code;
                    self.eval_error = msg;
                } else {
                    self.eval_error = val.to_string();
                    let mut code = val.to_int() as i32;
                    if code == 0 && !self.eval_error.is_empty() {
                        code = 1;
                    }
                    self.eval_error_code = code;
                }
            }
            "0" => self.program_name = val.to_string(),
            "/" => self.irs = val.to_string(),
            "\\" => self.ors = val.to_string(),
            "," => self.ofs = val.to_string(),
            ";" => self.subscript_sep = val.to_string(),
            "\"" => self.list_separator = val.to_string(),
            "%" => self.format_page_number = val.to_int(),
            "=" => self.format_lines_per_page = val.to_int(),
            "-" => self.format_lines_left = val.to_int(),
            ":" => self.format_line_break_chars = val.to_string(),
            "*" => self.multiline_match = val.to_int() != 0,
            "^" => self.format_top_name = val.to_string(),
            "INC" => self.inc_hook_index = val.to_int(),
            "^A" => self.accumulator_format = val.to_string(),
            "^F" => self.max_system_fd = val.to_int(),
            "^L" => self.formfeed_string = val.to_string(),
            "^M" => self.emergency_memory = val.to_string(),
            "^I" => self.inplace_edit = val.to_string(),
            "^D" => self.debug_flags = val.to_int(),
            "^P" => self.perl_debug_flags = val.to_int(),
            "^W" => self.warnings = val.to_int() != 0,
            "^H" => self.compile_hints = val.to_int(),
            "^WARNING_BITS" => self.warning_bits = val.to_int(),
            "|" => {
                self.output_autoflush = val.to_int() != 0;
                if self.output_autoflush {
                    let _ = io::stdout().flush();
                }
            }
            // Read-only or pid-backed
            "$$"
            | "]"
            | "^S"
            | "ARGV"
            | "?"
            | "^O"
            | "^T"
            | "^V"
            | "^E"
            | "^GLOBAL_PHASE"
            | "^MATCH"
            | "^PREMATCH"
            | "^POSTMATCH"
            | "^LAST_SUBMATCH_RESULT"
            | "^C"
            | "^N"
            | "^X"
            | "^TAINT"
            | "^TAINTED"
            | "^UNICODE"
            | "^UTF8LOCALE"
            | "^UTF8CACHE"
            | "+"
            | "<"
            | ">"
            | "("
            | ")" => {}
            _ if name.starts_with('^') && name.len() > 1 => {
                self.special_caret_scalars
                    .insert(name.to_string(), val.clone());
            }
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

    /// Walk C3 MRO from `start_package` and return the first `Package::AUTOLOAD` (`AUTOLOAD` in `main`).
    pub(crate) fn resolve_autoload_sub(&self, start_package: &str) -> Option<Arc<PerlSub>> {
        let root = if start_package.is_empty() {
            "main"
        } else {
            start_package
        };
        for pkg in self.mro_linearize(root) {
            let key = if pkg == "main" {
                "AUTOLOAD".to_string()
            } else {
                format!("{}::AUTOLOAD", pkg)
            };
            if let Some(s) = self.subs.get(&key) {
                return Some(s.clone());
            }
        }
        None
    }

    /// If an `AUTOLOAD` exists in the invocant's inheritance chain, set `$AUTOLOAD` to the fully
    /// qualified missing sub or method name and invoke the handler (same argument list as the
    /// missing call). For plain subs, `method_invocant_class` is `None` and the search starts from
    /// the package prefix of the missing name (or current package).
    pub(crate) fn try_autoload_call(
        &mut self,
        missing_name: &str,
        args: Vec<PerlValue>,
        line: usize,
        want: WantarrayCtx,
        method_invocant_class: Option<&str>,
    ) -> Option<ExecResult> {
        let pkg = self.current_package();
        let full = if missing_name.contains("::") {
            missing_name.to_string()
        } else {
            format!("{}::{}", pkg, missing_name)
        };
        let start_pkg = method_invocant_class.unwrap_or_else(|| {
            full.rsplit_once("::")
                .map(|(p, _)| p)
                .filter(|p| !p.is_empty())
                .unwrap_or("main")
        });
        let sub = self.resolve_autoload_sub(start_pkg)?;
        if let Err(e) = self
            .scope
            .set_scalar("AUTOLOAD", PerlValue::string(full.clone()))
        {
            return Some(Err(e.into()));
        }
        Some(self.call_sub(&sub, args, want, line))
    }

    pub(crate) fn with_topic_default_args(&self, args: Vec<PerlValue>) -> Vec<PerlValue> {
        if args.is_empty() {
            vec![self.scope.get_scalar("_").clone()]
        } else {
            args
        }
    }

    fn call_named_sub(
        &mut self,
        name: &str,
        args: Vec<PerlValue>,
        line: usize,
        want: WantarrayCtx,
    ) -> ExecResult {
        if let Some(sub) = self.resolve_sub_by_name(name) {
            let args = self.with_topic_default_args(args);
            return self.call_sub(&sub, args, want, line);
        }
        match name {
            "deque" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("deque() takes no arguments", line).into());
                }
                Ok(PerlValue::deque(Arc::new(Mutex::new(VecDeque::new()))))
            }
            "heap" => {
                if args.len() != 1 {
                    return Err(
                        PerlError::runtime("heap() expects one comparator sub", line).into(),
                    );
                }
                if let Some(sub) = args[0].as_code_ref() {
                    Ok(PerlValue::heap(Arc::new(Mutex::new(PerlHeap {
                        items: Vec::new(),
                        cmp: Arc::clone(&sub),
                    }))))
                } else {
                    Err(PerlError::runtime("heap() requires a code reference", line).into())
                }
            }
            "pipeline" => {
                let mut items = Vec::new();
                for v in args {
                    if let Some(a) = v.as_array_vec() {
                        items.extend(a);
                    } else {
                        items.push(v);
                    }
                }
                Ok(PerlValue::pipeline(Arc::new(Mutex::new(PipelineInner {
                    source: items,
                    ops: Vec::new(),
                    has_scalar_terminal: false,
                    par_stream: false,
                    streaming: false,
                    streaming_workers: 0,
                    streaming_buffer: 256,
                }))))
            }
            "par_pipeline" => {
                if crate::par_pipeline::is_named_par_pipeline_args(&args) {
                    return crate::par_pipeline::run_par_pipeline(self, &args, line)
                        .map_err(Into::into);
                }
                Ok(self.builtin_par_pipeline_stream(&args, line)?)
            }
            "par_pipeline_stream" => {
                if crate::par_pipeline::is_named_par_pipeline_args(&args) {
                    return crate::par_pipeline::run_par_pipeline_streaming(self, &args, line)
                        .map_err(Into::into);
                }
                Ok(self.builtin_par_pipeline_stream_new(&args, line)?)
            }
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
                Ok(PerlValue::barrier(PerlBarrier(Arc::new(Barrier::new(n)))))
            }
            _ => {
                let args = self.with_topic_default_args(args);
                if let Some(r) = self.try_autoload_call(name, args, line, want, None) {
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

    /// True if `name` is a registered or standard process-global handle.
    pub(crate) fn is_bound_handle(&self, name: &str) -> bool {
        matches!(name, "STDIN" | "STDOUT" | "STDERR")
            || self.input_handles.contains_key(name)
            || self.output_handles.contains_key(name)
            || self.io_file_slots.contains_key(name)
            || self.pipe_children.contains_key(name)
    }

    /// IO::File-style methods on handle values (`$fh->print`, `STDOUT->say`, …).
    pub(crate) fn io_handle_method(
        &mut self,
        name: &str,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match method {
            "print" => self.io_handle_print(name, args, false, line),
            "say" => self.io_handle_print(name, args, true, line),
            "printf" => self.io_handle_printf(name, args, line),
            "getline" | "readline" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime(
                        format!("{}: too many arguments", method),
                        line,
                    ));
                }
                self.readline_builtin_execute(Some(name))
            }
            "close" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("close: too many arguments", line));
                }
                self.close_builtin_execute(name.to_string())
            }
            "eof" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("eof: too many arguments", line));
                }
                let at_eof = !self.has_input_handle(name);
                Ok(PerlValue::integer(if at_eof { 1 } else { 0 }))
            }
            "getc" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("getc: too many arguments", line));
                }
                match crate::builtins::try_builtin(
                    self,
                    "getc",
                    &[PerlValue::string(name.to_string())],
                    line,
                ) {
                    Some(r) => r,
                    None => Err(PerlError::runtime("getc: not available", line)),
                }
            }
            "binmode" => match crate::builtins::try_builtin(
                self,
                "binmode",
                &[PerlValue::string(name.to_string())],
                line,
            ) {
                Some(r) => r,
                None => Err(PerlError::runtime("binmode: not available", line)),
            },
            "fileno" => match crate::builtins::try_builtin(
                self,
                "fileno",
                &[PerlValue::string(name.to_string())],
                line,
            ) {
                Some(r) => r,
                None => Err(PerlError::runtime("fileno: not available", line)),
            },
            "flush" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("flush: too many arguments", line));
                }
                self.io_handle_flush(name, line)
            }
            _ => Err(PerlError::runtime(
                format!("Unknown method for filehandle: {}", method),
                line,
            )),
        }
    }

    fn io_handle_flush(&mut self, handle_name: &str, line: usize) -> PerlResult<PerlValue> {
        match handle_name {
            "STDOUT" => {
                let _ = IoWrite::flush(&mut io::stdout());
            }
            "STDERR" => {
                let _ = IoWrite::flush(&mut io::stderr());
            }
            name => {
                if let Some(writer) = self.output_handles.get_mut(name) {
                    let _ = IoWrite::flush(&mut *writer);
                } else {
                    return Err(PerlError::runtime(
                        format!("flush on unopened filehandle {}", name),
                        line,
                    ));
                }
            }
        }
        Ok(PerlValue::integer(1))
    }

    fn io_handle_print(
        &mut self,
        handle_name: &str,
        args: &[PerlValue],
        newline: bool,
        line: usize,
    ) -> PerlResult<PerlValue> {
        if newline && (self.feature_bits & FEAT_SAY) == 0 {
            return Err(PerlError::runtime(
                "say() is disabled (enable with use feature 'say' or use feature ':5.10')",
                line,
            ));
        }
        let mut output = String::new();
        if args.is_empty() {
            // Match Perl: print with no LIST prints $_ (same overload rules as other args here: `to_string`).
            output.push_str(&self.scope.get_scalar("_").to_string());
        } else {
            for (i, val) in args.iter().enumerate() {
                if i > 0 && !self.ofs.is_empty() {
                    output.push_str(&self.ofs);
                }
                output.push_str(&val.to_string());
            }
        }
        if newline {
            output.push('\n');
        }
        output.push_str(&self.ors);

        match handle_name {
            "STDOUT" => {
                if !self.suppress_stdout {
                    print!("{}", output);
                    if self.output_autoflush {
                        let _ = io::stdout().flush();
                    }
                }
            }
            "STDERR" => {
                eprint!("{}", output);
                let _ = io::stderr().flush();
            }
            name => {
                if let Some(writer) = self.output_handles.get_mut(name) {
                    let _ = writer.write_all(output.as_bytes());
                    if self.output_autoflush {
                        let _ = writer.flush();
                    }
                } else {
                    return Err(PerlError::runtime(
                        format!("print on unopened filehandle {}", name),
                        line,
                    ));
                }
            }
        }
        Ok(PerlValue::integer(1))
    }

    fn io_handle_printf(
        &mut self,
        handle_name: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        let (fmt, rest): (String, &[PerlValue]) = if args.is_empty() {
            let s = match self.stringify_value(self.scope.get_scalar("_").clone(), line) {
                Ok(s) => s,
                Err(FlowOrError::Error(e)) => return Err(e),
                Err(FlowOrError::Flow(_)) => {
                    return Err(PerlError::runtime(
                        "printf: unexpected control flow in sprintf",
                        line,
                    ));
                }
            };
            (s, &[])
        } else {
            (args[0].to_string(), &args[1..])
        };
        let output = match self.perl_sprintf_stringify(&fmt, rest, line) {
            Ok(s) => s,
            Err(FlowOrError::Error(e)) => return Err(e),
            Err(FlowOrError::Flow(_)) => {
                return Err(PerlError::runtime(
                    "printf: unexpected control flow in sprintf",
                    line,
                ));
            }
        };
        match handle_name {
            "STDOUT" => {
                if !self.suppress_stdout {
                    print!("{}", output);
                    if self.output_autoflush {
                        let _ = IoWrite::flush(&mut io::stdout());
                    }
                }
            }
            "STDERR" => {
                eprint!("{}", output);
                let _ = IoWrite::flush(&mut io::stderr());
            }
            name => {
                if let Some(writer) = self.output_handles.get_mut(name) {
                    let _ = writer.write_all(output.as_bytes());
                    if self.output_autoflush {
                        let _ = writer.flush();
                    }
                } else {
                    return Err(PerlError::runtime(
                        format!("printf on unopened filehandle {}", name),
                        line,
                    ));
                }
            }
        }
        Ok(PerlValue::integer(1))
    }

    /// `deque` / `heap` method dispatch (`$q->push_back`, `$pq->pop`, …).
    pub(crate) fn try_native_method(
        &mut self,
        receiver: &PerlValue,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> Option<PerlResult<PerlValue>> {
        if let Some(name) = receiver.as_io_handle_name() {
            return Some(self.io_handle_method(&name, method, args, line));
        }
        if let Some(ref s) = receiver.as_str() {
            if self.is_bound_handle(s) {
                return Some(self.io_handle_method(s, method, args, line));
            }
        }
        if let Some(c) = receiver.as_sqlite_conn() {
            return Some(crate::native_data::sqlite_dispatch(&c, method, args, line));
        }
        if let Some(s) = receiver.as_struct_inst() {
            if let Some(idx) = s.def.field_index(method) {
                if !args.is_empty() {
                    return Some(Err(PerlError::runtime(
                        format!("struct field `{}` takes no arguments", method),
                        line,
                    )));
                }
                return Some(Ok(s.values[idx].clone()));
            }
            return None;
        }
        if let Some(d) = receiver.as_dataframe() {
            return Some(self.dataframe_method(d, method, args, line));
        }
        if let Some(d) = receiver.as_deque() {
            return Some(self.deque_method(d, method, args, line));
        }
        if let Some(h) = receiver.as_heap_pq() {
            return Some(self.heap_method(h, method, args, line));
        }
        if let Some(p) = receiver.as_pipeline() {
            return Some(self.pipeline_method(p, method, args, line));
        }
        if let Some(c) = receiver.as_capture() {
            return Some(self.capture_method(c, method, args, line));
        }
        if let Some(p) = receiver.as_ppool() {
            return Some(self.ppool_method(p, method, args, line));
        }
        if let Some(b) = receiver.as_barrier() {
            return Some(self.barrier_method(b, method, args, line));
        }
        if let Some(arc) = receiver.as_atomic_arc() {
            let inner = arc.lock().clone();
            if let Some(d) = inner.as_deque() {
                return Some(self.deque_method(d, method, args, line));
            }
            if let Some(h) = inner.as_heap_pq() {
                return Some(self.heap_method(h, method, args, line));
            }
        }
        None
    }

    /// `dataframe(path)` — `filter`, `group_by`, `sum`, `nrow`, `ncol`.
    fn dataframe_method(
        &mut self,
        d: Arc<Mutex<PerlDataFrame>>,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match method {
            "nrow" | "nrows" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime(
                        format!("dataframe {} takes no arguments", method),
                        line,
                    ));
                }
                Ok(PerlValue::integer(d.lock().nrows() as i64))
            }
            "ncol" | "ncols" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime(
                        format!("dataframe {} takes no arguments", method),
                        line,
                    ));
                }
                Ok(PerlValue::integer(d.lock().ncols() as i64))
            }
            "filter" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "dataframe filter expects 1 argument (sub)",
                        line,
                    ));
                }
                let Some(sub) = args[0].as_code_ref() else {
                    return Err(PerlError::runtime(
                        "dataframe filter expects a code reference",
                        line,
                    ));
                };
                let df_guard = d.lock();
                let n = df_guard.nrows();
                let mut keep = vec![false; n];
                for (r, row_keep) in keep.iter_mut().enumerate().take(n) {
                    let row = df_guard.row_hashref(r);
                    self.scope_push_hook();
                    let _ = self.scope.set_scalar("_", row);
                    if let Some(ref env) = sub.closure_env {
                        self.scope.restore_capture(env);
                    }
                    let pass = match self.exec_block_no_scope(&sub.body) {
                        Ok(v) => v.is_true(),
                        Err(_) => false,
                    };
                    self.scope_pop_hook();
                    *row_keep = pass;
                }
                let columns = df_guard.columns.clone();
                let cols: Vec<Vec<PerlValue>> = (0..df_guard.ncols())
                    .map(|i| {
                        let mut out = Vec::new();
                        for (r, pass_row) in keep.iter().enumerate().take(n) {
                            if *pass_row {
                                out.push(df_guard.cols[i][r].clone());
                            }
                        }
                        out
                    })
                    .collect();
                let group_by = df_guard.group_by.clone();
                drop(df_guard);
                let new_df = PerlDataFrame {
                    columns,
                    cols,
                    group_by,
                };
                Ok(PerlValue::dataframe(Arc::new(Mutex::new(new_df))))
            }
            "group_by" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "dataframe group_by expects 1 column name",
                        line,
                    ));
                }
                let key = args[0].to_string();
                let inner = d.lock();
                if inner.col_index(&key).is_none() {
                    return Err(PerlError::runtime(
                        format!("dataframe group_by: unknown column \"{}\"", key),
                        line,
                    ));
                }
                let new_df = PerlDataFrame {
                    columns: inner.columns.clone(),
                    cols: inner.cols.clone(),
                    group_by: Some(key),
                };
                Ok(PerlValue::dataframe(Arc::new(Mutex::new(new_df))))
            }
            "sum" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "dataframe sum expects 1 column name",
                        line,
                    ));
                }
                let col_name = args[0].to_string();
                let inner = d.lock();
                let val_idx = inner.col_index(&col_name).ok_or_else(|| {
                    PerlError::runtime(
                        format!("dataframe sum: unknown column \"{}\"", col_name),
                        line,
                    )
                })?;
                match &inner.group_by {
                    Some(gcol) => {
                        let gi = inner.col_index(gcol).ok_or_else(|| {
                            PerlError::runtime(
                                format!("dataframe sum: unknown group column \"{}\"", gcol),
                                line,
                            )
                        })?;
                        let mut acc: IndexMap<String, f64> = IndexMap::new();
                        for r in 0..inner.nrows() {
                            let k = inner.cols[gi][r].to_string();
                            let v = inner.cols[val_idx][r].to_number();
                            *acc.entry(k).or_insert(0.0) += v;
                        }
                        let keys: Vec<String> = acc.keys().cloned().collect();
                        let sums: Vec<f64> = acc.values().copied().collect();
                        let cols = vec![
                            keys.into_iter().map(PerlValue::string).collect(),
                            sums.into_iter().map(PerlValue::float).collect(),
                        ];
                        let columns = vec![gcol.clone(), format!("sum_{}", col_name)];
                        let out = PerlDataFrame {
                            columns,
                            cols,
                            group_by: None,
                        };
                        Ok(PerlValue::dataframe(Arc::new(Mutex::new(out))))
                    }
                    None => {
                        let total: f64 = (0..inner.nrows())
                            .map(|r| inner.cols[val_idx][r].to_number())
                            .sum();
                        Ok(PerlValue::float(total))
                    }
                }
            }
            _ => Err(PerlError::runtime(
                format!("Unknown method for dataframe: {}", method),
                line,
            )),
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
                Ok(PerlValue::integer(d.lock().len() as i64))
            }
            "push_front" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime("push_front expects 1 argument", line));
                }
                d.lock().push_front(args[0].clone());
                Ok(PerlValue::integer(d.lock().len() as i64))
            }
            "pop_back" => Ok(d.lock().pop_back().unwrap_or(PerlValue::UNDEF)),
            "pop_front" => Ok(d.lock().pop_front().unwrap_or(PerlValue::UNDEF)),
            "size" | "len" => Ok(PerlValue::integer(d.lock().len() as i64)),
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
                Ok(PerlValue::integer(g.items.len() as i64))
            }
            "pop" => {
                let mut g = h.lock();
                if g.items.is_empty() {
                    return Ok(PerlValue::UNDEF);
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
            "peek" => Ok(h.lock().items.first().cloned().unwrap_or(PerlValue::UNDEF)),
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
                Ok(PerlValue::integer(1))
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
            "stdout" => Ok(PerlValue::string(c.stdout.clone())),
            "stderr" => Ok(PerlValue::string(c.stderr.clone())),
            "exitcode" => Ok(PerlValue::integer(c.exitcode)),
            "failed" => Ok(PerlValue::integer(if c.exitcode != 0 { 1 } else { 0 })),
            _ => Err(PerlError::runtime(
                format!("Unknown method for capture: {}", method),
                line,
            )),
        }
    }

    pub(crate) fn builtin_par_pipeline_stream(
        &mut self,
        args: &[PerlValue],
        _line: usize,
    ) -> PerlResult<PerlValue> {
        let mut items = Vec::new();
        for v in args {
            if let Some(a) = v.as_array_vec() {
                items.extend(a);
            } else {
                items.push(v.clone());
            }
        }
        Ok(PerlValue::pipeline(Arc::new(Mutex::new(PipelineInner {
            source: items,
            ops: Vec::new(),
            has_scalar_terminal: false,
            par_stream: true,
            streaming: false,
            streaming_workers: 0,
            streaming_buffer: 256,
        }))))
    }

    /// `par_pipeline_stream(@list, workers => N, buffer => N)` — create a streaming pipeline
    /// that wires ops through bounded channels on `collect()`.
    pub(crate) fn builtin_par_pipeline_stream_new(
        &mut self,
        args: &[PerlValue],
        _line: usize,
    ) -> PerlResult<PerlValue> {
        let mut items = Vec::new();
        let mut workers: usize = 0;
        let mut buffer: usize = 256;
        // Separate list items from keyword args (workers => N, buffer => N).
        let mut i = 0;
        while i < args.len() {
            let s = args[i].to_string();
            if (s == "workers" || s == "buffer") && i + 1 < args.len() {
                let val = args[i + 1].to_int().max(1) as usize;
                if s == "workers" {
                    workers = val;
                } else {
                    buffer = val;
                }
                i += 2;
            } else if let Some(a) = args[i].as_array_vec() {
                items.extend(a);
                i += 1;
            } else {
                items.push(args[i].clone());
                i += 1;
            }
        }
        Ok(PerlValue::pipeline(Arc::new(Mutex::new(PipelineInner {
            source: items,
            ops: Vec::new(),
            has_scalar_terminal: false,
            par_stream: false,
            streaming: true,
            streaming_workers: workers,
            streaming_buffer: buffer,
        }))))
    }

    fn pipeline_push(
        &self,
        p: &Arc<Mutex<PipelineInner>>,
        op: PipelineOp,
        line: usize,
    ) -> PerlResult<()> {
        let mut g = p.lock();
        if g.has_scalar_terminal {
            return Err(PerlError::runtime(
                "pipeline: cannot chain after preduce / preduce_init / pmap_reduce (must be last before collect)",
                line,
            ));
        }
        if matches!(
            &op,
            PipelineOp::PReduce { .. }
                | PipelineOp::PReduceInit { .. }
                | PipelineOp::PMapReduce { .. }
        ) {
            g.has_scalar_terminal = true;
        }
        g.ops.push(op);
        Ok(())
    }

    fn pipeline_parse_sub_progress(
        args: &[PerlValue],
        line: usize,
        name: &str,
    ) -> PerlResult<(Arc<PerlSub>, bool)> {
        if args.is_empty() {
            return Err(PerlError::runtime(
                format!("pipeline {}: expects at least 1 argument (code ref)", name),
                line,
            ));
        }
        let Some(sub) = args[0].as_code_ref() else {
            return Err(PerlError::runtime(
                format!("pipeline {}: first argument must be a code reference", name),
                line,
            ));
        };
        let progress = args.get(1).map(|x| x.is_true()).unwrap_or(false);
        if args.len() > 2 {
            return Err(PerlError::runtime(
                format!(
                    "pipeline {}: at most 2 arguments (sub, optional progress flag)",
                    name
                ),
                line,
            ));
        }
        Ok((sub, progress))
    }

    fn pipeline_method(
        &mut self,
        p: Arc<Mutex<PipelineInner>>,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match method {
            "filter" | "grep" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "pipeline filter/grep expects 1 argument (sub)",
                        line,
                    ));
                }
                let Some(sub) = args[0].as_code_ref() else {
                    return Err(PerlError::runtime(
                        "pipeline filter/grep expects a code reference",
                        line,
                    ));
                };
                self.pipeline_push(&p, PipelineOp::Filter(sub), line)?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "map" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "pipeline map expects 1 argument (sub)",
                        line,
                    ));
                }
                let Some(sub) = args[0].as_code_ref() else {
                    return Err(PerlError::runtime(
                        "pipeline map expects a code reference",
                        line,
                    ));
                };
                self.pipeline_push(&p, PipelineOp::Map(sub), line)?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "take" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime("pipeline take expects 1 argument", line));
                }
                let n = args[0].to_int();
                self.pipeline_push(&p, PipelineOp::Take(n), line)?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "pmap" => {
                let (sub, progress) = Self::pipeline_parse_sub_progress(args, line, "pmap")?;
                self.pipeline_push(&p, PipelineOp::PMap { sub, progress }, line)?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "pgrep" => {
                let (sub, progress) = Self::pipeline_parse_sub_progress(args, line, "pgrep")?;
                self.pipeline_push(&p, PipelineOp::PGrep { sub, progress }, line)?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "pfor" => {
                let (sub, progress) = Self::pipeline_parse_sub_progress(args, line, "pfor")?;
                self.pipeline_push(&p, PipelineOp::PFor { sub, progress }, line)?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "pmap_chunked" => {
                if args.len() < 2 {
                    return Err(PerlError::runtime(
                        "pipeline pmap_chunked expects chunk size and a code reference",
                        line,
                    ));
                }
                let chunk = args[0].to_int().max(1);
                let Some(sub) = args[1].as_code_ref() else {
                    return Err(PerlError::runtime(
                        "pipeline pmap_chunked: second argument must be a code reference",
                        line,
                    ));
                };
                let progress = args.get(2).map(|x| x.is_true()).unwrap_or(false);
                if args.len() > 3 {
                    return Err(PerlError::runtime(
                        "pipeline pmap_chunked: chunk, sub, optional progress (at most 3 args)",
                        line,
                    ));
                }
                self.pipeline_push(
                    &p,
                    PipelineOp::PMapChunked {
                        chunk,
                        sub,
                        progress,
                    },
                    line,
                )?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "psort" => {
                let (cmp, progress) = match args.len() {
                    0 => (None, false),
                    1 => {
                        if let Some(s) = args[0].as_code_ref() {
                            (Some(s), false)
                        } else {
                            (None, args[0].is_true())
                        }
                    }
                    2 => {
                        let Some(s) = args[0].as_code_ref() else {
                            return Err(PerlError::runtime(
                                "pipeline psort: with two arguments, the first must be a comparator sub",
                                line,
                            ));
                        };
                        (Some(s), args[1].is_true())
                    }
                    _ => {
                        return Err(PerlError::runtime(
                            "pipeline psort: 0 args, 1 (sub or progress), or 2 (sub, progress)",
                            line,
                        ));
                    }
                };
                self.pipeline_push(&p, PipelineOp::PSort { cmp, progress }, line)?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "pcache" => {
                let (sub, progress) = Self::pipeline_parse_sub_progress(args, line, "pcache")?;
                self.pipeline_push(&p, PipelineOp::PCache { sub, progress }, line)?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "preduce" => {
                let (sub, progress) = Self::pipeline_parse_sub_progress(args, line, "preduce")?;
                self.pipeline_push(&p, PipelineOp::PReduce { sub, progress }, line)?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "preduce_init" => {
                if args.len() < 2 {
                    return Err(PerlError::runtime(
                        "pipeline preduce_init expects init value and a code reference",
                        line,
                    ));
                }
                let init = args[0].clone();
                let Some(sub) = args[1].as_code_ref() else {
                    return Err(PerlError::runtime(
                        "pipeline preduce_init: second argument must be a code reference",
                        line,
                    ));
                };
                let progress = args.get(2).map(|x| x.is_true()).unwrap_or(false);
                if args.len() > 3 {
                    return Err(PerlError::runtime(
                        "pipeline preduce_init: init, sub, optional progress (at most 3 args)",
                        line,
                    ));
                }
                self.pipeline_push(
                    &p,
                    PipelineOp::PReduceInit {
                        init,
                        sub,
                        progress,
                    },
                    line,
                )?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "pmap_reduce" => {
                if args.len() < 2 {
                    return Err(PerlError::runtime(
                        "pipeline pmap_reduce expects map sub and reduce sub",
                        line,
                    ));
                }
                let Some(map) = args[0].as_code_ref() else {
                    return Err(PerlError::runtime(
                        "pipeline pmap_reduce: first argument must be a code reference (map)",
                        line,
                    ));
                };
                let Some(reduce) = args[1].as_code_ref() else {
                    return Err(PerlError::runtime(
                        "pipeline pmap_reduce: second argument must be a code reference (reduce)",
                        line,
                    ));
                };
                let progress = args.get(2).map(|x| x.is_true()).unwrap_or(false);
                if args.len() > 3 {
                    return Err(PerlError::runtime(
                        "pipeline pmap_reduce: map, reduce, optional progress (at most 3 args)",
                        line,
                    ));
                }
                self.pipeline_push(
                    &p,
                    PipelineOp::PMapReduce {
                        map,
                        reduce,
                        progress,
                    },
                    line,
                )?;
                Ok(PerlValue::pipeline(Arc::clone(&p)))
            }
            "collect" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime(
                        "pipeline collect takes no arguments",
                        line,
                    ));
                }
                self.pipeline_collect(&p, line)
            }
            _ => {
                // Any other name: resolve as a subroutine (`sub name { ... }` in scope) and treat
                // like `->map` — `$_` is each element (same as `map { } @_` over the stream).
                if let Some(sub) = self.resolve_sub_by_name(method) {
                    if !args.is_empty() {
                        return Err(PerlError::runtime(
                            format!(
                                "pipeline ->{}: resolved subroutine takes no arguments; use a no-arg call or built-in ->map(sub {{ ... }}) / ->filter(sub {{ ... }})",
                                method
                            ),
                            line,
                        ));
                    }
                    self.pipeline_push(&p, PipelineOp::Map(sub), line)?;
                    Ok(PerlValue::pipeline(Arc::clone(&p)))
                } else {
                    Err(PerlError::runtime(
                        format!("Unknown method for pipeline: {}", method),
                        line,
                    ))
                }
            }
        }
    }

    fn pipeline_parallel_map(
        &mut self,
        items: Vec<PerlValue>,
        sub: &Arc<PerlSub>,
        progress: bool,
    ) -> Vec<PerlValue> {
        let subs = self.subs.clone();
        let (scope_capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
        let pmap_progress = PmapProgress::new(progress, items.len());
        let results: Vec<PerlValue> = items
            .into_par_iter()
            .map(|item| {
                let mut local_interp = Interpreter::new();
                local_interp.subs = subs.clone();
                local_interp.scope.restore_capture(&scope_capture);
                local_interp
                    .scope
                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                local_interp.enable_parallel_guard();
                let _ = local_interp.scope.set_scalar("_", item);
                local_interp.scope_push_hook();
                let val = match local_interp.exec_block_no_scope(&sub.body) {
                    Ok(val) => val,
                    Err(_) => PerlValue::UNDEF,
                };
                local_interp.scope_pop_hook();
                pmap_progress.tick();
                val
            })
            .collect();
        pmap_progress.finish();
        results
    }

    /// Order-preserving parallel filter for `par_pipeline(LIST)` (same capture rules as `pgrep`).
    fn pipeline_par_stream_filter(
        &mut self,
        items: Vec<PerlValue>,
        sub: &Arc<PerlSub>,
    ) -> Vec<PerlValue> {
        if items.is_empty() {
            return items;
        }
        let subs = self.subs.clone();
        let (scope_capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
        let indexed: Vec<(usize, PerlValue)> = items.into_iter().enumerate().collect();
        let mut kept: Vec<(usize, PerlValue)> = indexed
            .into_par_iter()
            .filter_map(|(i, item)| {
                let mut local_interp = Interpreter::new();
                local_interp.subs = subs.clone();
                local_interp.scope.restore_capture(&scope_capture);
                local_interp
                    .scope
                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                local_interp.enable_parallel_guard();
                let _ = local_interp.scope.set_scalar("_", item.clone());
                local_interp.scope_push_hook();
                let keep = match local_interp.exec_block_no_scope(&sub.body) {
                    Ok(val) => val.is_true(),
                    Err(_) => false,
                };
                local_interp.scope_pop_hook();
                if keep {
                    Some((i, item))
                } else {
                    None
                }
            })
            .collect();
        kept.sort_by_key(|(i, _)| *i);
        kept.into_iter().map(|(_, x)| x).collect()
    }

    /// Order-preserving parallel map for `par_pipeline(LIST)` (same capture rules as `pmap`).
    fn pipeline_par_stream_map(
        &mut self,
        items: Vec<PerlValue>,
        sub: &Arc<PerlSub>,
    ) -> Vec<PerlValue> {
        if items.is_empty() {
            return items;
        }
        let subs = self.subs.clone();
        let (scope_capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
        let indexed: Vec<(usize, PerlValue)> = items.into_iter().enumerate().collect();
        let mut mapped: Vec<(usize, PerlValue)> = indexed
            .into_par_iter()
            .map(|(i, item)| {
                let mut local_interp = Interpreter::new();
                local_interp.subs = subs.clone();
                local_interp.scope.restore_capture(&scope_capture);
                local_interp
                    .scope
                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                local_interp.enable_parallel_guard();
                let _ = local_interp.scope.set_scalar("_", item);
                local_interp.scope_push_hook();
                let val = match local_interp.exec_block_no_scope(&sub.body) {
                    Ok(val) => val,
                    Err(_) => PerlValue::UNDEF,
                };
                local_interp.scope_pop_hook();
                (i, val)
            })
            .collect();
        mapped.sort_by_key(|(i, _)| *i);
        mapped.into_iter().map(|(_, x)| x).collect()
    }

    fn pipeline_collect(
        &mut self,
        p: &Arc<Mutex<PipelineInner>>,
        line: usize,
    ) -> PerlResult<PerlValue> {
        let (mut v, ops, par_stream, streaming, streaming_workers, streaming_buffer) = {
            let g = p.lock();
            (
                g.source.clone(),
                g.ops.clone(),
                g.par_stream,
                g.streaming,
                g.streaming_workers,
                g.streaming_buffer,
            )
        };
        if streaming {
            return self.pipeline_collect_streaming(
                v,
                &ops,
                streaming_workers,
                streaming_buffer,
                line,
            );
        }
        for op in ops {
            match op {
                PipelineOp::Filter(sub) => {
                    if par_stream {
                        v = self.pipeline_par_stream_filter(v, &sub);
                    } else {
                        let mut out = Vec::new();
                        for item in v {
                            self.scope_push_hook();
                            let _ = self.scope.set_scalar("_", item.clone());
                            if let Some(ref env) = sub.closure_env {
                                self.scope.restore_capture(env);
                            }
                            let keep = match self.exec_block_no_scope(&sub.body) {
                                Ok(val) => val.is_true(),
                                Err(_) => false,
                            };
                            self.scope_pop_hook();
                            if keep {
                                out.push(item);
                            }
                        }
                        v = out;
                    }
                }
                PipelineOp::Map(sub) => {
                    if par_stream {
                        v = self.pipeline_par_stream_map(v, &sub);
                    } else {
                        let mut out = Vec::new();
                        for item in v {
                            self.scope_push_hook();
                            let _ = self.scope.set_scalar("_", item);
                            if let Some(ref env) = sub.closure_env {
                                self.scope.restore_capture(env);
                            }
                            let mapped = match self.exec_block_no_scope(&sub.body) {
                                Ok(val) => val,
                                Err(_) => PerlValue::UNDEF,
                            };
                            self.scope_pop_hook();
                            out.push(mapped);
                        }
                        v = out;
                    }
                }
                PipelineOp::Take(n) => {
                    let n = n.max(0) as usize;
                    if v.len() > n {
                        v.truncate(n);
                    }
                }
                PipelineOp::PMap { sub, progress } => {
                    v = self.pipeline_parallel_map(v, &sub, progress);
                }
                PipelineOp::PGrep { sub, progress } => {
                    let subs = self.subs.clone();
                    let (scope_capture, atomic_arrays, atomic_hashes) =
                        self.scope.capture_with_atomics();
                    let pmap_progress = PmapProgress::new(progress, v.len());
                    v = v
                        .into_par_iter()
                        .filter_map(|item| {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            local_interp
                                .scope
                                .restore_atomics(&atomic_arrays, &atomic_hashes);
                            local_interp.enable_parallel_guard();
                            let _ = local_interp.scope.set_scalar("_", item.clone());
                            local_interp.scope_push_hook();
                            let keep = match local_interp.exec_block_no_scope(&sub.body) {
                                Ok(val) => val.is_true(),
                                Err(_) => false,
                            };
                            local_interp.scope_pop_hook();
                            pmap_progress.tick();
                            if keep {
                                Some(item)
                            } else {
                                None
                            }
                        })
                        .collect();
                    pmap_progress.finish();
                }
                PipelineOp::PFor { sub, progress } => {
                    let subs = self.subs.clone();
                    let (scope_capture, atomic_arrays, atomic_hashes) =
                        self.scope.capture_with_atomics();
                    let pmap_progress = PmapProgress::new(progress, v.len());
                    let first_err: Arc<Mutex<Option<PerlError>>> = Arc::new(Mutex::new(None));
                    v.clone().into_par_iter().for_each(|item| {
                        if first_err.lock().is_some() {
                            return;
                        }
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        local_interp
                            .scope
                            .restore_atomics(&atomic_arrays, &atomic_hashes);
                        local_interp.enable_parallel_guard();
                        let _ = local_interp.scope.set_scalar("_", item);
                        local_interp.scope_push_hook();
                        match local_interp.exec_block_no_scope(&sub.body) {
                            Ok(_) => {}
                            Err(e) => {
                                let pe = match e {
                                    FlowOrError::Error(pe) => pe,
                                    FlowOrError::Flow(_) => PerlError::runtime(
                                        "return/last/next/redo not supported inside pipeline pfor block",
                                        line,
                                    ),
                                };
                                let mut g = first_err.lock();
                                if g.is_none() {
                                    *g = Some(pe);
                                }
                            }
                        }
                        local_interp.scope_pop_hook();
                        pmap_progress.tick();
                    });
                    pmap_progress.finish();
                    let pfor_err = first_err.lock().take();
                    if let Some(e) = pfor_err {
                        return Err(e);
                    }
                }
                PipelineOp::PMapChunked {
                    chunk,
                    sub,
                    progress,
                } => {
                    let chunk_n = chunk.max(1) as usize;
                    let subs = self.subs.clone();
                    let (scope_capture, atomic_arrays, atomic_hashes) =
                        self.scope.capture_with_atomics();
                    let indexed_chunks: Vec<(usize, Vec<PerlValue>)> = v
                        .chunks(chunk_n)
                        .enumerate()
                        .map(|(i, c)| (i, c.to_vec()))
                        .collect();
                    let n_chunks = indexed_chunks.len();
                    let pmap_progress = PmapProgress::new(progress, n_chunks);
                    let mut chunk_results: Vec<(usize, Vec<PerlValue>)> = indexed_chunks
                        .into_par_iter()
                        .map(|(chunk_idx, chunk)| {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            local_interp
                                .scope
                                .restore_atomics(&atomic_arrays, &atomic_hashes);
                            local_interp.enable_parallel_guard();
                            let mut out = Vec::with_capacity(chunk.len());
                            for item in chunk {
                                let _ = local_interp.scope.set_scalar("_", item);
                                local_interp.scope_push_hook();
                                match local_interp.exec_block_no_scope(&sub.body) {
                                    Ok(val) => {
                                        local_interp.scope_pop_hook();
                                        out.push(val);
                                    }
                                    Err(_) => {
                                        local_interp.scope_pop_hook();
                                        out.push(PerlValue::UNDEF);
                                    }
                                }
                            }
                            pmap_progress.tick();
                            (chunk_idx, out)
                        })
                        .collect();
                    pmap_progress.finish();
                    chunk_results.sort_by_key(|(i, _)| *i);
                    v = chunk_results.into_iter().flat_map(|(_, x)| x).collect();
                }
                PipelineOp::PSort { cmp, progress } => {
                    let pmap_progress = PmapProgress::new(progress, 2);
                    pmap_progress.tick();
                    match cmp {
                        Some(cmp_block) => {
                            if let Some(mode) = detect_sort_block_fast(&cmp_block.body) {
                                v.par_sort_by(|a, b| sort_magic_cmp(a, b, mode));
                            } else {
                                let subs = self.subs.clone();
                                let scope_capture = self.scope.capture();
                                v.par_sort_by(|a, b| {
                                    let mut local_interp = Interpreter::new();
                                    local_interp.subs = subs.clone();
                                    local_interp.scope.restore_capture(&scope_capture);
                                    local_interp.enable_parallel_guard();
                                    let _ = local_interp.scope.set_scalar("a", a.clone());
                                    let _ = local_interp.scope.set_scalar("b", b.clone());
                                    local_interp.scope_push_hook();
                                    let ord =
                                        match local_interp.exec_block_no_scope(&cmp_block.body) {
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
                                        };
                                    local_interp.scope_pop_hook();
                                    ord
                                });
                            }
                        }
                        None => {
                            v.par_sort_by(|a, b| a.to_string().cmp(&b.to_string()));
                        }
                    }
                    pmap_progress.tick();
                    pmap_progress.finish();
                }
                PipelineOp::PCache { sub, progress } => {
                    let subs = self.subs.clone();
                    let scope_capture = self.scope.capture();
                    let cache = &*crate::pcache::GLOBAL_PCACHE;
                    let pmap_progress = PmapProgress::new(progress, v.len());
                    v = v
                        .into_par_iter()
                        .map(|item| {
                            let k = crate::pcache::cache_key(&item);
                            if let Some(cached) = cache.get(&k) {
                                pmap_progress.tick();
                                return cached.clone();
                            }
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            local_interp.enable_parallel_guard();
                            let _ = local_interp.scope.set_scalar("_", item.clone());
                            local_interp.scope_push_hook();
                            let val = match local_interp.exec_block_no_scope(&sub.body) {
                                Ok(v) => v,
                                Err(_) => PerlValue::UNDEF,
                            };
                            local_interp.scope_pop_hook();
                            cache.insert(k, val.clone());
                            pmap_progress.tick();
                            val
                        })
                        .collect();
                    pmap_progress.finish();
                }
                PipelineOp::PReduce { sub, progress } => {
                    if v.is_empty() {
                        return Ok(PerlValue::UNDEF);
                    }
                    if v.len() == 1 {
                        return Ok(v.into_iter().next().unwrap());
                    }
                    let block = sub.body.clone();
                    let subs = self.subs.clone();
                    let scope_capture = self.scope.capture();
                    let pmap_progress = PmapProgress::new(progress, v.len());
                    let result = v
                        .into_par_iter()
                        .map(|x| {
                            pmap_progress.tick();
                            x
                        })
                        .reduce_with(|a, b| {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            local_interp.enable_parallel_guard();
                            let _ = local_interp.scope.set_scalar("a", a);
                            let _ = local_interp.scope.set_scalar("b", b);
                            match local_interp.exec_block(&block) {
                                Ok(val) => val,
                                Err(_) => PerlValue::UNDEF,
                            }
                        });
                    pmap_progress.finish();
                    return Ok(result.unwrap_or(PerlValue::UNDEF));
                }
                PipelineOp::PReduceInit {
                    init,
                    sub,
                    progress,
                } => {
                    if v.is_empty() {
                        return Ok(init);
                    }
                    let block = sub.body.clone();
                    let subs = self.subs.clone();
                    let scope_capture = self.scope.capture();
                    let cap: &[(String, PerlValue)] = scope_capture.as_slice();
                    if v.len() == 1 {
                        return Ok(fold_preduce_init_step(
                            &subs,
                            cap,
                            &block,
                            preduce_init_fold_identity(&init),
                            v.into_iter().next().unwrap(),
                        ));
                    }
                    let pmap_progress = PmapProgress::new(progress, v.len());
                    let result = v
                        .into_par_iter()
                        .fold(
                            || preduce_init_fold_identity(&init),
                            |acc, item| {
                                pmap_progress.tick();
                                fold_preduce_init_step(&subs, cap, &block, acc, item)
                            },
                        )
                        .reduce(
                            || preduce_init_fold_identity(&init),
                            |a, b| merge_preduce_init_partials(a, b, &block, &subs, cap),
                        );
                    pmap_progress.finish();
                    return Ok(result);
                }
                PipelineOp::PMapReduce {
                    map,
                    reduce,
                    progress,
                } => {
                    if v.is_empty() {
                        return Ok(PerlValue::UNDEF);
                    }
                    let map_block = map.body.clone();
                    let reduce_block = reduce.body.clone();
                    let subs = self.subs.clone();
                    let scope_capture = self.scope.capture();
                    if v.len() == 1 {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        let _ = local_interp.scope.set_scalar("_", v[0].clone());
                        return match local_interp.exec_block_no_scope(&map_block) {
                            Ok(val) => Ok(val),
                            Err(_) => Ok(PerlValue::UNDEF),
                        };
                    }
                    let pmap_progress = PmapProgress::new(progress, v.len());
                    let result = v
                        .into_par_iter()
                        .map(|item| {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            let _ = local_interp.scope.set_scalar("_", item);
                            let val = match local_interp.exec_block_no_scope(&map_block) {
                                Ok(val) => val,
                                Err(_) => PerlValue::UNDEF,
                            };
                            pmap_progress.tick();
                            val
                        })
                        .reduce_with(|a, b| {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            let _ = local_interp.scope.set_scalar("a", a);
                            let _ = local_interp.scope.set_scalar("b", b);
                            match local_interp.exec_block_no_scope(&reduce_block) {
                                Ok(val) => val,
                                Err(_) => PerlValue::UNDEF,
                            }
                        });
                    pmap_progress.finish();
                    return Ok(result.unwrap_or(PerlValue::UNDEF));
                }
            }
        }
        Ok(PerlValue::array(v))
    }

    /// Streaming collect: wire pipeline ops through bounded channels so items flow
    /// between stages concurrently.  Order is **not** preserved.
    fn pipeline_collect_streaming(
        &mut self,
        source: Vec<PerlValue>,
        ops: &[PipelineOp],
        workers_per_stage: usize,
        buffer: usize,
        line: usize,
    ) -> PerlResult<PerlValue> {
        use crossbeam::channel::{bounded, Receiver, Sender};

        // Validate: reject ops that require all items (can't stream).
        for op in ops {
            match op {
                PipelineOp::PSort { .. }
                | PipelineOp::PReduce { .. }
                | PipelineOp::PReduceInit { .. }
                | PipelineOp::PMapReduce { .. }
                | PipelineOp::PMapChunked { .. } => {
                    return Err(PerlError::runtime(
                        format!(
                            "par_pipeline_stream: {:?} requires all items and cannot stream; use par_pipeline instead",
                            std::mem::discriminant(op)
                        ),
                        line,
                    ));
                }
                _ => {}
            }
        }

        // Filter out non-streamable ops and collect streamable ones.
        // Supported: Filter, Map, Take, PMap, PGrep, PFor, PCache.
        let streamable_ops: Vec<&PipelineOp> = ops.iter().collect();
        if streamable_ops.is_empty() {
            return Ok(PerlValue::array(source));
        }

        let n_stages = streamable_ops.len();
        let wn = if workers_per_stage > 0 {
            workers_per_stage
        } else {
            self.parallel_thread_count()
        };
        let subs = self.subs.clone();
        let (capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();

        // Build channels: one between each pair of stages, plus one for output.
        // channel[0]: source → stage 0
        // channel[i]: stage i-1 → stage i
        // channel[n_stages]: stage n_stages-1 → collector
        let mut channels: Vec<(Sender<PerlValue>, Receiver<PerlValue>)> =
            (0..=n_stages).map(|_| bounded(buffer)).collect();

        let err: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let take_done: Arc<std::sync::atomic::AtomicBool> =
            Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Collect senders/receivers for each stage.
        // Stage i reads from channels[i].1 and writes to channels[i+1].0.
        let source_tx = channels[0].0.clone();
        let result_rx = channels[n_stages].1.clone();
        let results: Arc<Mutex<Vec<PerlValue>>> = Arc::new(Mutex::new(Vec::new()));

        std::thread::scope(|scope| {
            // Collector thread: drain results concurrently to avoid deadlock
            // when bounded channels fill up.
            let result_rx_c = result_rx.clone();
            let results_c = Arc::clone(&results);
            scope.spawn(move || {
                while let Ok(item) = result_rx_c.recv() {
                    results_c.lock().push(item);
                }
            });

            // Source feeder thread.
            let err_s = Arc::clone(&err);
            let take_done_s = Arc::clone(&take_done);
            scope.spawn(move || {
                for item in source {
                    if err_s.lock().is_some()
                        || take_done_s.load(std::sync::atomic::Ordering::Relaxed)
                    {
                        break;
                    }
                    if source_tx.send(item).is_err() {
                        break;
                    }
                }
            });

            // Spawn workers for each stage.
            for (stage_idx, op) in streamable_ops.iter().enumerate() {
                let rx = channels[stage_idx].1.clone();
                let tx = channels[stage_idx + 1].0.clone();

                for _ in 0..wn {
                    let rx = rx.clone();
                    let tx = tx.clone();
                    let subs = subs.clone();
                    let capture = capture.clone();
                    let atomic_arrays = atomic_arrays.clone();
                    let atomic_hashes = atomic_hashes.clone();
                    let err_w = Arc::clone(&err);
                    let take_done_w = Arc::clone(&take_done);

                    match *op {
                        PipelineOp::Filter(ref sub) | PipelineOp::PGrep { ref sub, .. } => {
                            let sub = Arc::clone(sub);
                            scope.spawn(move || {
                                while let Ok(item) = rx.recv() {
                                    if err_w.lock().is_some() {
                                        break;
                                    }
                                    let mut interp = Interpreter::new();
                                    interp.subs = subs.clone();
                                    interp.scope.restore_capture(&capture);
                                    interp.scope.restore_atomics(&atomic_arrays, &atomic_hashes);
                                    interp.enable_parallel_guard();
                                    let _ = interp.scope.set_scalar("_", item.clone());
                                    interp.scope_push_hook();
                                    let keep = match interp.exec_block_no_scope(&sub.body) {
                                        Ok(val) => val.is_true(),
                                        Err(_) => false,
                                    };
                                    interp.scope_pop_hook();
                                    if keep && tx.send(item).is_err() {
                                        break;
                                    }
                                }
                            });
                        }
                        PipelineOp::Map(ref sub) | PipelineOp::PMap { ref sub, .. } => {
                            let sub = Arc::clone(sub);
                            scope.spawn(move || {
                                while let Ok(item) = rx.recv() {
                                    if err_w.lock().is_some() {
                                        break;
                                    }
                                    let mut interp = Interpreter::new();
                                    interp.subs = subs.clone();
                                    interp.scope.restore_capture(&capture);
                                    interp.scope.restore_atomics(&atomic_arrays, &atomic_hashes);
                                    interp.enable_parallel_guard();
                                    let _ = interp.scope.set_scalar("_", item);
                                    interp.scope_push_hook();
                                    let mapped = match interp.exec_block_no_scope(&sub.body) {
                                        Ok(val) => val,
                                        Err(_) => PerlValue::UNDEF,
                                    };
                                    interp.scope_pop_hook();
                                    if tx.send(mapped).is_err() {
                                        break;
                                    }
                                }
                            });
                        }
                        PipelineOp::Take(n) => {
                            let limit = (*n).max(0) as usize;
                            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                            let count_w = Arc::clone(&count);
                            scope.spawn(move || {
                                while let Ok(item) = rx.recv() {
                                    let prev =
                                        count_w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                    if prev >= limit {
                                        take_done_w
                                            .store(true, std::sync::atomic::Ordering::Relaxed);
                                        break;
                                    }
                                    if tx.send(item).is_err() {
                                        break;
                                    }
                                }
                            });
                            // Take only needs 1 worker; skip remaining worker spawns.
                            break;
                        }
                        PipelineOp::PFor { ref sub, .. } => {
                            let sub = Arc::clone(sub);
                            scope.spawn(move || {
                                while let Ok(item) = rx.recv() {
                                    if err_w.lock().is_some() {
                                        break;
                                    }
                                    let mut interp = Interpreter::new();
                                    interp.subs = subs.clone();
                                    interp.scope.restore_capture(&capture);
                                    interp
                                        .scope
                                        .restore_atomics(&atomic_arrays, &atomic_hashes);
                                    interp.enable_parallel_guard();
                                    let _ = interp.scope.set_scalar("_", item.clone());
                                    interp.scope_push_hook();
                                    match interp.exec_block_no_scope(&sub.body) {
                                        Ok(_) => {}
                                        Err(e) => {
                                            let msg = match e {
                                                FlowOrError::Error(pe) => pe.to_string(),
                                                FlowOrError::Flow(_) => {
                                                    "unexpected control flow in par_pipeline_stream pfor".into()
                                                }
                                            };
                                            let mut g = err_w.lock();
                                            if g.is_none() {
                                                *g = Some(msg);
                                            }
                                            interp.scope_pop_hook();
                                            break;
                                        }
                                    }
                                    interp.scope_pop_hook();
                                    if tx.send(item).is_err() {
                                        break;
                                    }
                                }
                            });
                        }
                        PipelineOp::PCache { ref sub, .. } => {
                            let sub = Arc::clone(sub);
                            scope.spawn(move || {
                                while let Ok(item) = rx.recv() {
                                    if err_w.lock().is_some() {
                                        break;
                                    }
                                    let k = crate::pcache::cache_key(&item);
                                    let val = if let Some(cached) =
                                        crate::pcache::GLOBAL_PCACHE.get(&k)
                                    {
                                        cached.clone()
                                    } else {
                                        let mut interp = Interpreter::new();
                                        interp.subs = subs.clone();
                                        interp.scope.restore_capture(&capture);
                                        interp
                                            .scope
                                            .restore_atomics(&atomic_arrays, &atomic_hashes);
                                        interp.enable_parallel_guard();
                                        let _ = interp.scope.set_scalar("_", item);
                                        interp.scope_push_hook();
                                        let v = match interp.exec_block_no_scope(&sub.body) {
                                            Ok(v) => v,
                                            Err(_) => PerlValue::UNDEF,
                                        };
                                        interp.scope_pop_hook();
                                        crate::pcache::GLOBAL_PCACHE.insert(k, v.clone());
                                        v
                                    };
                                    if tx.send(val).is_err() {
                                        break;
                                    }
                                }
                            });
                        }
                        // Non-streaming ops already rejected above.
                        _ => unreachable!(),
                    }
                }
            }

            // Drop our copies of intermediate senders/receivers so channels disconnect
            // when workers finish.  Also drop result_rx so the collector thread exits
            // once all stage workers are done.
            channels.clear();
            drop(result_rx);
        });

        if let Some(msg) = err.lock().take() {
            return Err(PerlError::runtime(msg, line));
        }

        let results = std::mem::take(&mut *results.lock());
        Ok(PerlValue::array(results))
    }

    fn heap_compare(&mut self, cmp: &Arc<PerlSub>, a: &PerlValue, b: &PerlValue) -> Ordering {
        self.scope_push_hook();
        if let Some(ref env) = cmp.closure_env {
            self.scope.restore_capture(env);
        }
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
        self.scope_pop_hook();
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
        self.scope_push_hook();
        self.scope.declare_array("_", args);
        let argv = self.scope.get_array("_");
        if let Some(ref env) = sub.closure_env {
            self.scope.restore_capture(env);
        }
        let saved = self.wantarray_kind;
        self.wantarray_kind = want;
        if let Some(r) = crate::list_util::native_dispatch(self, sub, &argv, want) {
            self.wantarray_kind = saved;
            self.scope_pop_hook();
            return match r {
                Ok(v) => Ok(v),
                Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
                Err(e) => Err(e),
            };
        }
        if let Some(pat) = sub.fib_like.as_ref() {
            if argv.len() == 1 {
                if let Some(n0) = argv[0].as_integer() {
                    let t0 = self.profiler.is_some().then(std::time::Instant::now);
                    if let Some(p) = &mut self.profiler {
                        p.enter_sub(&sub.name);
                    }
                    let n = crate::fib_like_tail::eval_fib_like_recursive_add(n0, pat);
                    if let (Some(p), Some(t0)) = (&mut self.profiler, t0) {
                        p.exit_sub(t0.elapsed());
                    }
                    self.wantarray_kind = saved;
                    self.scope_pop_hook();
                    return Ok(PerlValue::integer(n));
                }
            }
        }
        let t0 = self.profiler.is_some().then(std::time::Instant::now);
        if let Some(p) = &mut self.profiler {
            p.enter_sub(&sub.name);
        }
        let result = self.exec_block_no_scope(&sub.body);
        if let (Some(p), Some(t0)) = (&mut self.profiler, t0) {
            p.exit_sub(t0.elapsed());
        }
        self.wantarray_kind = saved;
        self.scope_pop_hook();
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
        Ok(PerlValue::blessed(Arc::new(crate::value::BlessedRef {
            class: class.to_string(),
            data: RwLock::new(PerlValue::hash(map)),
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
        if args.is_empty() {
            // Perl: print with no LIST prints $_ (same for say).
            let topic = self.scope.get_scalar("_").clone();
            let s = self.stringify_value(topic, line)?;
            output.push_str(&s);
        } else {
            for (i, a) in args.iter().enumerate() {
                if i > 0 && !self.ofs.is_empty() {
                    output.push_str(&self.ofs);
                }
                let val = self.eval_expr(a)?;
                let s = self.stringify_value(val, line)?;
                output.push_str(&s);
            }
        }
        if newline {
            output.push('\n');
        }
        output.push_str(&self.ors);

        let handle_name = self.resolve_io_handle_name(handle.unwrap_or("STDOUT"));
        match handle_name.as_str() {
            "STDOUT" => {
                if !self.suppress_stdout {
                    print!("{}", output);
                    if self.output_autoflush {
                        let _ = io::stdout().flush();
                    }
                }
            }
            "STDERR" => {
                eprint!("{}", output);
                let _ = io::stderr().flush();
            }
            name => {
                if let Some(writer) = self.output_handles.get_mut(name) {
                    let _ = writer.write_all(output.as_bytes());
                    if self.output_autoflush {
                        let _ = writer.flush();
                    }
                } else {
                    return Err(PerlError::runtime(
                        format!("print on unopened filehandle {}", name),
                        line,
                    )
                    .into());
                }
            }
        }
        Ok(PerlValue::integer(1))
    }

    fn exec_printf(&mut self, handle: Option<&str>, args: &[Expr], line: usize) -> ExecResult {
        let (fmt, rest): (String, &[Expr]) = if args.is_empty() {
            // Perl: printf with no args uses $_ as the format string.
            let s = self.stringify_value(self.scope.get_scalar("_").clone(), line)?;
            (s, &[])
        } else {
            (self.eval_expr(&args[0])?.to_string(), &args[1..])
        };
        let mut arg_vals = Vec::new();
        for a in rest {
            arg_vals.push(self.eval_expr(a)?);
        }
        let output = self.perl_sprintf_stringify(&fmt, &arg_vals, line)?;
        let handle_name = self.resolve_io_handle_name(handle.unwrap_or("STDOUT"));
        match handle_name.as_str() {
            "STDOUT" => {
                if !self.suppress_stdout {
                    print!("{}", output);
                    if self.output_autoflush {
                        let _ = io::stdout().flush();
                    }
                }
            }
            "STDERR" => {
                eprint!("{}", output);
                let _ = io::stderr().flush();
            }
            name => {
                if let Some(writer) = self.output_handles.get_mut(name) {
                    let _ = writer.write_all(output.as_bytes());
                    if self.output_autoflush {
                        let _ = writer.flush();
                    }
                }
            }
        }
        Ok(PerlValue::integer(1))
    }

    /// `substr` with optional replacement — mutates `string` when `replacement` is `Some` (also used by VM).
    pub(crate) fn eval_substr_expr(
        &mut self,
        string: &Expr,
        offset: &Expr,
        length: Option<&Expr>,
        replacement: Option<&Expr>,
        _line: usize,
    ) -> Result<PerlValue, FlowOrError> {
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
            s.len().saturating_sub(start)
        };
        let end = (start + len).min(s.len());
        let result = s.get(start..end).unwrap_or("").to_string();
        if let Some(rep) = replacement {
            let rep_s = self.eval_expr(rep)?.to_string();
            let mut new_s = String::new();
            new_s.push_str(&s[..start]);
            new_s.push_str(&rep_s);
            new_s.push_str(&s[end..]);
            self.assign_value(string, PerlValue::string(new_s))?;
        }
        Ok(PerlValue::string(result))
    }

    pub(crate) fn eval_push_expr(
        &mut self,
        array: &Expr,
        values: &[Expr],
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
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
            if let Some(items) = val.as_array_vec() {
                for item in items {
                    self.scope
                        .push_to_array(&arr_name, item)
                        .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
                }
            } else {
                self.scope
                    .push_to_array(&arr_name, val)
                    .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            }
        }
        let len = self.scope.array_len(&arr_name);
        Ok(PerlValue::integer(len as i64))
    }

    pub(crate) fn eval_pop_expr(
        &mut self,
        array: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let arr_name = self.extract_array_name(array)?;
        self.scope
            .pop_from_array(&arr_name)
            .map_err(|e| FlowOrError::Error(e.at_line(line)))
    }

    pub(crate) fn eval_shift_expr(
        &mut self,
        array: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let arr_name = self.extract_array_name(array)?;
        self.scope
            .shift_from_array(&arr_name)
            .map_err(|e| FlowOrError::Error(e.at_line(line)))
    }

    pub(crate) fn eval_unshift_expr(
        &mut self,
        array: &Expr,
        values: &[Expr],
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let arr_name = self.extract_array_name(array)?;
        let mut vals = Vec::new();
        for v in values {
            let val = self.eval_expr(v)?;
            vals.push(val);
        }
        let arr = self
            .scope
            .get_array_mut(&arr_name)
            .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
        for (i, v) in vals.into_iter().enumerate() {
            arr.insert(i, v);
        }
        let len = arr.len();
        Ok(PerlValue::integer(len as i64))
    }

    pub(crate) fn eval_splice_expr(
        &mut self,
        array: &Expr,
        offset: Option<&Expr>,
        length: Option<&Expr>,
        replacement: &[Expr],
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let arr_name = self.extract_array_name(array)?;
        let off = if let Some(o) = offset {
            self.eval_expr(o)?.to_int() as usize
        } else {
            0
        };
        let len = if let Some(l) = length {
            self.eval_expr(l)?.to_int() as usize
        } else {
            let arr = self
                .scope
                .get_array_mut(&arr_name)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            arr.len() - off
        };
        let mut rep_vals = Vec::new();
        for r in replacement {
            rep_vals.push(self.eval_expr(r)?);
        }
        let arr = self
            .scope
            .get_array_mut(&arr_name)
            .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
        let end = (off + len).min(arr.len());
        let removed: Vec<PerlValue> = arr.drain(off..end).collect();
        for (i, v) in rep_vals.into_iter().enumerate() {
            arr.insert(off + i, v);
        }
        Ok(PerlValue::array(removed))
    }

    pub(crate) fn eval_keys_expr(
        &mut self,
        expr: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let val = self.eval_expr(expr)?;
        if let Some(h) = val.as_hash_map() {
            Ok(PerlValue::array(
                h.keys().map(|k| PerlValue::string(k.clone())).collect(),
            ))
        } else if let Some(r) = val.as_hash_ref() {
            Ok(PerlValue::array(
                r.read()
                    .keys()
                    .map(|k| PerlValue::string(k.clone()))
                    .collect(),
            ))
        } else {
            Err(PerlError::runtime("keys requires hash", line).into())
        }
    }

    pub(crate) fn eval_values_expr(
        &mut self,
        expr: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let val = self.eval_expr(expr)?;
        if let Some(h) = val.as_hash_map() {
            Ok(PerlValue::array(h.values().cloned().collect()))
        } else if let Some(r) = val.as_hash_ref() {
            Ok(PerlValue::array(r.read().values().cloned().collect()))
        } else {
            Err(PerlError::runtime("values requires hash", line).into())
        }
    }

    pub(crate) fn eval_delete_operand(
        &mut self,
        expr: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        match &expr.kind {
            ExprKind::HashElement { hash, key } => {
                let k = self.eval_expr(key)?.to_string();
                self.touch_env_hash(hash);
                if let Some(obj) = self.tied_hashes.get(hash).cloned() {
                    let class = obj
                        .as_blessed_ref()
                        .map(|b| b.class.clone())
                        .unwrap_or_default();
                    let full = format!("{}::DELETE", class);
                    if let Some(sub) = self.subs.get(&full).cloned() {
                        return self.call_sub(
                            &sub,
                            vec![obj, PerlValue::string(k)],
                            WantarrayCtx::Scalar,
                            line,
                        );
                    }
                }
                self.scope
                    .delete_hash_element(hash, &k)
                    .map_err(|e| FlowOrError::Error(e.at_line(line)))
            }
            ExprKind::ArrowDeref {
                expr: inner,
                index,
                kind: DerefKind::Hash,
            } => {
                let k = self.eval_expr(index)?.to_string();
                let container = self.eval_expr(inner)?;
                self.delete_arrow_hash_element(container, &k, line)
                    .map_err(Into::into)
            }
            _ => Err(PerlError::runtime("delete requires hash element", line).into()),
        }
    }

    pub(crate) fn eval_exists_operand(
        &mut self,
        expr: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        match &expr.kind {
            ExprKind::HashElement { hash, key } => {
                let k = self.eval_expr(key)?.to_string();
                self.touch_env_hash(hash);
                if let Some(obj) = self.tied_hashes.get(hash).cloned() {
                    let class = obj
                        .as_blessed_ref()
                        .map(|b| b.class.clone())
                        .unwrap_or_default();
                    let full = format!("{}::EXISTS", class);
                    if let Some(sub) = self.subs.get(&full).cloned() {
                        return self.call_sub(
                            &sub,
                            vec![obj, PerlValue::string(k)],
                            WantarrayCtx::Scalar,
                            line,
                        );
                    }
                }
                Ok(PerlValue::integer(
                    if self.scope.exists_hash_element(hash, &k) {
                        1
                    } else {
                        0
                    },
                ))
            }
            ExprKind::ArrowDeref {
                expr: inner,
                index,
                kind: DerefKind::Hash,
            } => {
                let k = self.eval_expr(index)?.to_string();
                let container = self.eval_expr(inner)?;
                let yes = self.exists_arrow_hash_element(container, &k, line)?;
                Ok(PerlValue::integer(if yes { 1 } else { 0 }))
            }
            _ => Err(PerlError::runtime("exists requires hash element", line).into()),
        }
    }

    /// `par_lines PATH, sub { } [, progress => EXPR]` — mmap + parallel line iteration (also used by VM).
    pub(crate) fn eval_par_lines_expr(
        &mut self,
        path: &Expr,
        callback: &Expr,
        progress: Option<&Expr>,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let show_progress = progress
            .map(|p| self.eval_expr(p))
            .transpose()?
            .map(|v| v.is_true())
            .unwrap_or(false);
        let path_s = self.eval_expr(path)?.to_string();
        let cb_val = self.eval_expr(callback)?;
        let sub = if let Some(s) = cb_val.as_code_ref() {
            s
        } else {
            return Err(PerlError::runtime(
                "par_lines: second argument must be a code reference",
                line,
            )
            .into());
        };
        let subs = self.subs.clone();
        let (scope_capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
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
            return Ok(PerlValue::UNDEF);
        }
        let line_total = crate::par_lines::line_count_bytes(data);
        let pmap_progress = PmapProgress::new(show_progress, line_total);
        if self.num_threads == 0 {
            self.num_threads = rayon::current_num_threads();
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
                local_interp.enable_parallel_guard();
                let _ = local_interp
                    .scope
                    .set_scalar("_", PerlValue::string(line_str));
                match local_interp.call_sub(&sub, vec![], WantarrayCtx::Void, line) {
                    Ok(_) => {}
                    Err(e) => return Err(e),
                }
                pmap_progress.tick();
                if e >= slice.len() {
                    break;
                }
                s = e + 1;
            }
            Ok(())
        })?;
        pmap_progress.finish();
        Ok(PerlValue::UNDEF)
    }

    /// `par_walk PATH, sub { } [, progress => EXPR]` — parallel recursive directory walk (also used by VM).
    pub(crate) fn eval_par_walk_expr(
        &mut self,
        path: &Expr,
        callback: &Expr,
        progress: Option<&Expr>,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let show_progress = progress
            .map(|p| self.eval_expr(p))
            .transpose()?
            .map(|v| v.is_true())
            .unwrap_or(false);
        let path_val = self.eval_expr(path)?;
        let roots: Vec<PathBuf> = if let Some(arr) = path_val.as_array_vec() {
            arr.into_iter()
                .map(|v| PathBuf::from(v.to_string()))
                .collect()
        } else {
            vec![PathBuf::from(path_val.to_string())]
        };
        let cb_val = self.eval_expr(callback)?;
        let sub = if let Some(s) = cb_val.as_code_ref() {
            s
        } else {
            return Err(PerlError::runtime(
                "par_walk: second argument must be a code reference",
                line,
            )
            .into());
        };
        let subs = self.subs.clone();
        let (scope_capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();

        if show_progress {
            let paths = crate::par_walk::collect_paths(&roots);
            let pmap_progress = PmapProgress::new(true, paths.len());
            paths.into_par_iter().try_for_each(|p| {
                let s = p.to_string_lossy().into_owned();
                let mut local_interp = Interpreter::new();
                local_interp.subs = subs.clone();
                local_interp.scope.restore_capture(&scope_capture);
                local_interp
                    .scope
                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                local_interp.enable_parallel_guard();
                let _ = local_interp.scope.set_scalar("_", PerlValue::string(s));
                match local_interp.call_sub(sub.as_ref(), vec![], WantarrayCtx::Void, line) {
                    Ok(_) => {}
                    Err(e) => return Err(e),
                }
                pmap_progress.tick();
                Ok(())
            })?;
            pmap_progress.finish();
        } else {
            for r in &roots {
                par_walk_recursive(
                    r.as_path(),
                    &sub,
                    &subs,
                    &scope_capture,
                    &atomic_arrays,
                    &atomic_hashes,
                    line,
                )?;
            }
        }
        Ok(PerlValue::UNDEF)
    }

    /// `par_sed(PATTERN, REPLACEMENT, FILES...)` — parallel in-place regex substitution per file (`g` semantics).
    pub(crate) fn builtin_par_sed(
        &mut self,
        args: &[PerlValue],
        line: usize,
        has_progress: bool,
    ) -> PerlResult<PerlValue> {
        let show_progress = if has_progress {
            args.last().map(|v| v.is_true()).unwrap_or(false)
        } else {
            false
        };
        let slice = if has_progress {
            &args[..args.len().saturating_sub(1)]
        } else {
            args
        };
        if slice.len() < 3 {
            return Err(PerlError::runtime(
                "par_sed: need pattern, replacement, and at least one file path",
                line,
            ));
        }
        let pat_val = &slice[0];
        let repl = slice[1].to_string();
        let files: Vec<String> = slice[2..].iter().map(|v| v.to_string()).collect();

        let re = if let Some(rx) = pat_val.as_regex() {
            rx
        } else {
            let pattern = pat_val.to_string();
            match self.compile_regex(&pattern, "g", line) {
                Ok(r) => r,
                Err(FlowOrError::Error(e)) => return Err(e),
                Err(FlowOrError::Flow(f)) => {
                    return Err(PerlError::runtime(format!("par_sed: {:?}", f), line))
                }
            }
        };

        let pmap = PmapProgress::new(show_progress, files.len());
        let touched = AtomicUsize::new(0);
        files.par_iter().try_for_each(|path| {
            let content = std::fs::read_to_string(path).map_err(|e| {
                PerlError::runtime(format!("par_sed {}: {}", path, e), line)
            })?;
            let new_s = re.replace_all(&content, &repl);
            if new_s != content {
                std::fs::write(path, new_s.as_bytes()).map_err(|e| {
                    PerlError::runtime(format!("par_sed {}: {}", path, e), line)
                })?;
                touched.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            pmap.tick();
            Ok(())
        })?;
        pmap.finish();
        Ok(PerlValue::integer(
            touched.load(std::sync::atomic::Ordering::Relaxed) as i64,
        ))
    }

    /// `pwatch GLOB, sub { }` — filesystem notify loop (also used by VM).
    pub(crate) fn eval_pwatch_expr(
        &mut self,
        path: &Expr,
        callback: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let pattern_s = self.eval_expr(path)?.to_string();
        let cb_val = self.eval_expr(callback)?;
        let sub = if let Some(s) = cb_val.as_code_ref() {
            s
        } else {
            return Err(PerlError::runtime(
                "pwatch: second argument must be a code reference",
                line,
            )
            .into());
        };
        let subs = self.subs.clone();
        let (scope_capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
        crate::pwatch::run_pwatch(
            &pattern_s,
            sub,
            subs,
            scope_capture,
            atomic_arrays,
            atomic_hashes,
            line,
        )
        .map_err(FlowOrError::Error)
    }

    pub(crate) fn compile_regex(
        &mut self,
        pattern: &str,
        flags: &str,
        line: usize,
    ) -> Result<Arc<PerlCompiledRegex>, FlowOrError> {
        // Fast path: same regex as last call (common in loops).
        // Arc clone is cheap (ref-count increment) AND preserves the lazy DFA cache.
        let multiline = self.multiline_match;
        if let Some((ref lp, ref lf, ref lm, ref lr)) = self.regex_last {
            if lp == pattern && lf == flags && *lm == multiline {
                return Ok(lr.clone());
            }
        }
        // Slow path: HashMap lookup
        let key = format!("{}\x00{}\x00{}", multiline as u8, flags, pattern);
        if let Some(cached) = self.regex_cache.get(&key) {
            self.regex_last = Some((
                pattern.to_string(),
                flags.to_string(),
                multiline,
                cached.clone(),
            ));
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
        // Deprecated `$*` multiline: dot matches newline (same intent as `(?s)`).
        if multiline {
            re_str.push_str("(?s)");
        }
        re_str.push_str(&expanded);
        let re = PerlCompiledRegex::compile(&re_str).map_err(|e| {
            FlowOrError::Error(PerlError::runtime(
                format!("Invalid regex /{}/: {}", pattern, e),
                line,
            ))
        })?;
        let arc = re;
        self.regex_last = Some((
            pattern.to_string(),
            flags.to_string(),
            multiline,
            arc.clone(),
        ));
        self.regex_cache.insert(key, arc.clone());
        Ok(arc)
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
            .set_scalar("_", PerlValue::string(line_str.to_string()));

        if self.auto_split {
            let sep = self.field_separator.as_deref().unwrap_or(" ");
            let re = regex::Regex::new(sep).unwrap_or_else(|_| regex::Regex::new(" ").unwrap());
            let fields: Vec<PerlValue> = re
                .split(line_str.trim_end_matches('\n'))
                .map(|s| PerlValue::string(s.to_string()))
                .collect();
            self.scope.set_array("F", fields)?;
        }

        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::SubDecl { .. }
                | StmtKind::Begin(_)
                | StmtKind::UnitCheck(_)
                | StmtKind::Check(_)
                | StmtKind::Init(_)
                | StmtKind::End(_) => continue,
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

fn par_walk_invoke_entry(
    path: &Path,
    sub: &Arc<PerlSub>,
    subs: &HashMap<String, Arc<PerlSub>>,
    scope_capture: &[(String, PerlValue)],
    atomic_arrays: &[(String, crate::scope::AtomicArray)],
    atomic_hashes: &[(String, crate::scope::AtomicHash)],
    line: usize,
) -> Result<(), FlowOrError> {
    let s = path.to_string_lossy().into_owned();
    let mut local_interp = Interpreter::new();
    local_interp.subs = subs.clone();
    local_interp.scope.restore_capture(scope_capture);
    local_interp
        .scope
        .restore_atomics(atomic_arrays, atomic_hashes);
    local_interp.enable_parallel_guard();
    let _ = local_interp.scope.set_scalar("_", PerlValue::string(s));
    local_interp.call_sub(sub.as_ref(), vec![], WantarrayCtx::Void, line)?;
    Ok(())
}

fn par_walk_recursive(
    path: &Path,
    sub: &Arc<PerlSub>,
    subs: &HashMap<String, Arc<PerlSub>>,
    scope_capture: &[(String, PerlValue)],
    atomic_arrays: &[(String, crate::scope::AtomicArray)],
    atomic_hashes: &[(String, crate::scope::AtomicHash)],
    line: usize,
) -> Result<(), FlowOrError> {
    if path.is_file() || (path.is_symlink() && !path.is_dir()) {
        return par_walk_invoke_entry(
            path,
            sub,
            subs,
            scope_capture,
            atomic_arrays,
            atomic_hashes,
            line,
        );
    }
    if !path.is_dir() {
        return Ok(());
    }
    par_walk_invoke_entry(
        path,
        sub,
        subs,
        scope_capture,
        atomic_arrays,
        atomic_hashes,
        line,
    )?;
    let read = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let entries: Vec<_> = read.filter_map(|e| e.ok()).collect();
    entries.par_iter().try_for_each(|e| {
        par_walk_recursive(
            &e.path(),
            sub,
            subs,
            scope_capture,
            atomic_arrays,
            atomic_hashes,
            line,
        )
    })?;
    Ok(())
}

/// `sprintf` with pluggable `%s` formatting (stringify for overload-aware `Interpreter`).
pub(crate) fn perl_sprintf_format_with<F>(
    fmt: &str,
    args: &[PerlValue],
    mut string_for_s: F,
) -> Result<String, FlowOrError>
where
    F: FnMut(&PerlValue) -> Result<String, FlowOrError>,
{
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

            let arg = args.get(arg_idx).cloned().unwrap_or(PerlValue::UNDEF);
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
                    let s = string_for_s(&arg)?;
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
    Ok(result)
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

#[cfg(test)]
mod special_scalar_name_tests {
    use super::Interpreter;

    #[test]
    fn special_scalar_name_for_get_matches_magic_globals() {
        assert!(Interpreter::is_special_scalar_name_for_get("0"));
        assert!(Interpreter::is_special_scalar_name_for_get("!"));
        assert!(Interpreter::is_special_scalar_name_for_get("^W"));
        assert!(Interpreter::is_special_scalar_name_for_get("^O"));
        assert!(Interpreter::is_special_scalar_name_for_get("^MATCH"));
        assert!(Interpreter::is_special_scalar_name_for_get("<"));
        assert!(Interpreter::is_special_scalar_name_for_get("?"));
        assert!(Interpreter::is_special_scalar_name_for_get("|"));
        assert!(Interpreter::is_special_scalar_name_for_get("^UNICODE"));
        assert!(Interpreter::is_special_scalar_name_for_get("\""));
        assert!(!Interpreter::is_special_scalar_name_for_get("foo"));
        assert!(!Interpreter::is_special_scalar_name_for_get("plainvar"));
    }

    #[test]
    fn special_scalar_name_for_set_matches_set_special_var_arms() {
        assert!(Interpreter::is_special_scalar_name_for_set("0"));
        assert!(Interpreter::is_special_scalar_name_for_set("^D"));
        assert!(Interpreter::is_special_scalar_name_for_set("^H"));
        assert!(Interpreter::is_special_scalar_name_for_set("^WARNING_BITS"));
        assert!(Interpreter::is_special_scalar_name_for_set("ARGV"));
        assert!(Interpreter::is_special_scalar_name_for_set("|"));
        assert!(Interpreter::is_special_scalar_name_for_set("?"));
        assert!(Interpreter::is_special_scalar_name_for_set("^UNICODE"));
        assert!(!Interpreter::is_special_scalar_name_for_set("foo"));
        assert!(!Interpreter::is_special_scalar_name_for_set("__PACKAGE__"));
    }

    #[test]
    fn caret_and_id_specials_roundtrip_get() {
        let i = Interpreter::new();
        assert_eq!(i.get_special_var("^O").to_string(), super::perl_osname());
        assert_eq!(
            i.get_special_var("^V").to_string(),
            format!("v{}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(i.get_special_var("^GLOBAL_PHASE").to_string(), "RUN");
        assert!(i.get_special_var("^T").to_int() >= 0);
        #[cfg(unix)]
        {
            assert!(i.get_special_var("<").to_int() >= 0);
        }
    }
}
