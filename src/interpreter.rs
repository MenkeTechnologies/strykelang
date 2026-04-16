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
use std::sync::Arc;
use std::sync::{Barrier, OnceLock};
use std::time::{Duration, Instant};

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
use crate::perl_decode::decode_utf8_or_latin1;
use crate::perl_fs::read_file_text_perl_compat;
use crate::perl_regex::{perl_quotemeta, PerlCaptures, PerlCompiledRegex};
use crate::pmap_progress::{FanProgress, PmapProgress};
use crate::profiler::Profiler;
use crate::scope::Scope;
use crate::sort_fast::{detect_sort_block_fast, sort_magic_cmp};
use crate::value::{
    perl_list_range_expand, CaptureResult, PerlAsyncTask, PerlBarrier, PerlDataFrame,
    PerlGenerator, PerlHeap, PerlPpool, PerlSub, PerlValue, PipelineInner, PipelineOp,
    RemoteCluster,
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

/// `(off, end)` for `splice` / `arr.drain(off..end)` — Perl negative OFFSET/LENGTH; clamps offset to array length.
#[inline]
fn splice_compute_range(
    arr_len: usize,
    offset_val: &PerlValue,
    length_val: &PerlValue,
) -> (usize, usize) {
    let off_i = offset_val.to_int();
    let off = if off_i < 0 {
        arr_len.saturating_sub((-off_i) as usize)
    } else {
        (off_i as usize).min(arr_len)
    };
    let rest = arr_len.saturating_sub(off);
    let take = if length_val.is_undef() {
        rest
    } else {
        let l = length_val.to_int();
        if l < 0 {
            rest.saturating_sub((-l) as usize)
        } else {
            (l as usize).min(rest)
        }
    };
    let end = (off + take).min(arr_len);
    (off, end)
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
    let _ = local_interp.scope.set_scalar("a", a.clone());
    let _ = local_interp.scope.set_scalar("b", b.clone());
    let _ = local_interp.scope.set_scalar("_0", a);
    let _ = local_interp.scope.set_scalar("_1", b);
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
    let _ = local_interp.scope.set_scalar("a", acc.clone());
    let _ = local_interp.scope.set_scalar("b", item.clone());
    let _ = local_interp.scope.set_scalar("_0", acc);
    let _ = local_interp.scope.set_scalar("_1", item);
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
    Yield(PerlValue),
    /// `goto &sub` — tail-call: replace current sub with the named one, keeping @_.
    GotoSub(String),
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

/// Bindings introduced by a successful algebraic [`MatchPattern`] (scalar vs array).
enum PatternBinding {
    Scalar(String, PerlValue),
    Array(String, Vec<PerlValue>),
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

/// Minimum log level filter for `log_*` / `log_json` (trace = most verbose).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LogLevelFilter {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevelFilter {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// True when `@$aref->[IX]` / `IX` needs **list** context on the RHS of `=` (multi-slot slice).
fn arrow_deref_array_assign_rhs_list_ctx(index: &Expr) -> bool {
    match &index.kind {
        ExprKind::Range { .. } => true,
        ExprKind::QW(ws) => ws.len() > 1,
        ExprKind::List(el) => {
            if el.len() > 1 {
                true
            } else if el.len() == 1 {
                arrow_deref_array_assign_rhs_list_ctx(&el[0])
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Wantarray for the RHS of a plain `=` assignment — must match [`crate::compiler::Compiler`] lowering
/// so `<>` / `readline` list-slurp matches Perl for `@a = <>` (not only `my`/`our`/`local` initializers).
pub(crate) fn assign_rhs_wantarray(target: &Expr) -> WantarrayCtx {
    match &target.kind {
        ExprKind::ArrayVar(_) | ExprKind::HashVar(_) => WantarrayCtx::List,
        ExprKind::ScalarVar(_) | ExprKind::ArrayElement { .. } | ExprKind::HashElement { .. } => {
            WantarrayCtx::Scalar
        }
        ExprKind::Deref { kind, .. } => match kind {
            Sigil::Scalar | Sigil::Typeglob => WantarrayCtx::Scalar,
            Sigil::Array | Sigil::Hash => WantarrayCtx::List,
        },
        ExprKind::ArrowDeref {
            index,
            kind: DerefKind::Array,
            ..
        } => {
            if arrow_deref_array_assign_rhs_list_ctx(index) {
                WantarrayCtx::List
            } else {
                WantarrayCtx::Scalar
            }
        }
        ExprKind::ArrowDeref {
            kind: DerefKind::Hash,
            ..
        }
        | ExprKind::ArrowDeref {
            kind: DerefKind::Call,
            ..
        } => WantarrayCtx::Scalar,
        ExprKind::HashSliceDeref { .. } | ExprKind::HashSlice { .. } => WantarrayCtx::List,
        ExprKind::ArraySlice { indices, .. } => {
            if indices.len() > 1 {
                WantarrayCtx::List
            } else if indices.len() == 1 {
                if arrow_deref_array_assign_rhs_list_ctx(&indices[0]) {
                    WantarrayCtx::List
                } else {
                    WantarrayCtx::Scalar
                }
            } else {
                WantarrayCtx::Scalar
            }
        }
        ExprKind::AnonymousListSlice { indices, .. } => {
            if indices.len() > 1 {
                WantarrayCtx::List
            } else if indices.len() == 1 {
                if arrow_deref_array_assign_rhs_list_ctx(&indices[0]) {
                    WantarrayCtx::List
                } else {
                    WantarrayCtx::Scalar
                }
            } else {
                WantarrayCtx::Scalar
            }
        }
        ExprKind::Typeglob(_) | ExprKind::TypeglobExpr(_) => WantarrayCtx::Scalar,
        _ => WantarrayCtx::Scalar,
    }
}

/// Memoized inputs + result for a non-`g` `regex_match_execute` call. Populated on every
/// successful match and consulted at the top of the next call; on exact-match (same pattern,
/// flags, multiline, and haystack content) we skip regex execution + capture-var scope population
/// entirely, replaying the stored `PerlValue` result. See [`Interpreter::regex_match_memo`].
#[derive(Clone)]
pub(crate) struct RegexMatchMemo {
    pub pattern: String,
    pub flags: String,
    pub multiline: bool,
    pub haystack: String,
    pub result: PerlValue,
}

/// Tree-walker state for scalar `..` / `...` (key: `Expr` address).
#[derive(Clone, Copy, Default)]
struct FlipFlopTreeState {
    active: bool,
    /// Exclusive `...`: `$.` line where the left bound matched — right is only tested when `$.` is
    /// strictly greater (Perl: do not test the right operand until the next evaluation; for numeric
    /// `$.` that defers past the left-match line, including multiple evals on that line).
    exclusive_left_line: Option<i64>,
}

/// `BufReader` / `print` / `sysread` / `tell` on the same handle share this [`File`] cursor.
#[derive(Clone)]
pub(crate) struct IoSharedFile(pub Arc<Mutex<File>>);

impl Read for IoSharedFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.lock().read(buf)
    }
}

pub(crate) struct IoSharedFileWrite(pub Arc<Mutex<File>>);

impl IoWrite for IoSharedFileWrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().flush()
    }
}

pub struct Interpreter {
    pub scope: Scope,
    pub(crate) subs: HashMap<String, Arc<PerlSub>>,
    pub(crate) file: String,
    /// File handles: name → writer
    pub(crate) output_handles: HashMap<String, Box<dyn IoWrite + Send>>,
    pub(crate) input_handles: HashMap<String, BufReader<Box<dyn Read + Send>>>,
    /// Output separator ($,)
    pub ofs: String,
    /// Output record separator ($\)
    pub ors: String,
    /// Input record separator (`$/`). `None` represents undef (slurp mode in `<>`).
    /// Default at startup: `Some("\n")`. `local $/` (no init) sets `None`.
    pub irs: Option<String>,
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
    /// Bracket text for `die` / `warn` after a stdin read: `"<>"` (diamond / `-n` queue) vs `"<STDIN>"`.
    pub(crate) last_stdin_die_bracket: String,
    /// Line count per handle for `$.` when keyed (Perl-style last-read handle).
    pub handle_line_numbers: HashMap<String, i64>,
    /// Scalar and regex `..` / `...` flip-flop state for bytecode ([`crate::bytecode::Op::ScalarFlipFlop`],
    /// [`crate::bytecode::Op::RegexFlipFlop`], [`crate::bytecode::Op::RegexEofFlipFlop`],
    /// [`crate::bytecode::Op::RegexFlipFlopExprRhs`]).
    pub(crate) flip_flop_active: Vec<bool>,
    /// Exclusive `...`: parallel to [`Self::flip_flop_active`] — `Some($. )` where the left bound
    /// matched; right is only compared when `$.` is strictly greater (see [`FlipFlopTreeState`]).
    pub(crate) flip_flop_exclusive_left_line: Vec<Option<i64>>,
    /// Running match counter for each scalar flip-flop slot — emitted as the *value* of a
    /// scalar `..`/`...` range (`"1"`, `"2"`, …, trailing `"E0"` on the exclusive close line)
    /// so `my $x = 1..5` matches Perl's stringification rather than returning a plain integer.
    pub(crate) flip_flop_sequence: Vec<i64>,
    /// Last `$.` seen for each slot so scalar flip-flop `seq` increments once per line, not
    /// per re-evaluation on the same `$.` (matches Perl `pp_flop`: two evaluations of the same
    /// range on one line return the same sequence number).
    pub(crate) flip_flop_last_dot: Vec<Option<i64>>,
    /// Scalar `..` / `...` flip-flop for tree-walker (key: `Expr` address).
    flip_flop_tree: HashMap<usize, FlipFlopTreeState>,
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
    /// Default handle for `print` / `say` / `printf` with no explicit handle (`select FH` sets this).
    pub default_print_handle: String,
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
    /// `use open ':encoding(UTF-8)'` / `qw(:std :encoding(UTF-8))` / `:utf8` — readline uses UTF-8 lossy decode.
    pub open_pragma_utf8: bool,
    /// `use feature` — bit flags (`FEAT_*`).
    pub feature_bits: u64,
    /// Number of parallel threads
    pub num_threads: usize,
    /// Compiled regex cache: "flags///pattern" → [`PerlCompiledRegex`] (Rust `regex` or `fancy-regex`).
    regex_cache: HashMap<String, Arc<PerlCompiledRegex>>,
    /// Last compiled regex — fast-path to avoid format! + HashMap lookup in tight loops.
    /// Third flag: `$*` multiline (prepends `(?s)` when true).
    regex_last: Option<(String, String, bool, Arc<PerlCompiledRegex>)>,
    /// Memo of the most-recent match's inputs and result for `regex_match_execute` (non-`g`,
    /// non-`scalar_g` path). Hot loops that re-match the same text against the same pattern
    /// (e.g. `while (...) { $text =~ /p/ }`) skip the regex execution AND the capture-variable
    /// scope population entirely on cache hit.
    ///
    /// Invalidation: any VM write to a capture variable (`$&`, `` $` ``, `$'`, `$+`, `$1`..`$9`,
    /// `@-`, `@+`, `%+`) clears the "scope still in sync" flag. The memo survives; only the
    /// capture-var side-effect replay is forced on the next hit.
    regex_match_memo: Option<RegexMatchMemo>,
    /// False when the user (or some non-regex code path) has written to one of the capture
    /// variables since the last `apply_regex_captures` call. The memoized match result is still
    /// valid, but the scope side effects need to be reapplied on the next hit.
    regex_capture_scope_fresh: bool,
    /// Offsets for Perl `m//g` in scalar context (`pos`), keyed by scalar name (`"_"` for `$_`).
    pub(crate) regex_pos: HashMap<String, Option<usize>>,
    /// Persistent storage for `state` variables, keyed by "line:name".
    pub(crate) state_vars: HashMap<String, PerlValue>,
    /// Per-frame tracking of state variable bindings: (var_name, state_key).
    state_bindings_stack: Vec<Vec<(String, String)>>,
    /// PRNG for `rand` / `srand` (matches Perl-style seeding, not crypto).
    pub(crate) rand_rng: StdRng,
    /// Directory handles from `opendir`: name → snapshot + read cursor (`readdir` / `rewinddir` / …).
    pub(crate) dir_handles: HashMap<String, DirHandleState>,
    /// Raw `File` per handle (shared with buffered input / `print` / `sys*`) so `tell` matches writes.
    pub(crate) io_file_slots: HashMap<String, Arc<Mutex<File>>>,
    /// Child processes for `open(H, "-|", cmd)` / `open(H, "|-", cmd)`; waited on `close`.
    pub(crate) pipe_children: HashMap<String, Child>,
    /// Sockets from `socket` / `accept` / `connect`.
    pub(crate) socket_handles: HashMap<String, PerlSocket>,
    /// `wantarray()` inside the current subroutine (`WantarrayCtx`; VM threads it on `Call`/`MethodCall`/`ArrowCall`).
    pub(crate) wantarray_kind: WantarrayCtx,
    /// `struct Name { ... }` definitions (merged from VM chunks and tree-walker).
    pub struct_defs: HashMap<String, Arc<StructDef>>,
    /// When set, `pe --profile` records timings: VM path uses per-opcode line samples and sub
    /// call/return (JIT disabled); tree-walker fallback uses per-statement lines and subs.
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
    /// `local` saves of special-variable backing fields (`$/`, `$\`, `$,`, `$"`, …).
    /// Mirrors `glob_restore_frames`: one Vec per scope frame; on `scope_pop_hook` each
    /// `(name, old_value)` is replayed via `set_special_var` so the underlying interpreter
    /// state (`self.irs` / `self.ofs` / etc.) restores when a `{ local $X = … }` block exits.
    pub(crate) special_var_restore_frames: Vec<Vec<(String, PerlValue)>>,
    /// `use English` — long names ([`crate::english::scalar_alias`]) map to short special scalars.
    pub(crate) english_enabled: bool,
    /// `use English qw(-no_match_vars)` — suppress `$MATCH`/`$PREMATCH`/`$POSTMATCH` aliases.
    pub(crate) english_no_match_vars: bool,
    /// Once `use English` (without `-no_match_vars`) has activated match vars, they stay
    /// available for the rest of the program — Perl exports them into the caller's namespace
    /// and later `no English` / `use English qw(-no_match_vars)` cannot un-export them.
    pub(crate) english_match_vars_ever_enabled: bool,
    /// Lexical scalar names (`my`/`our`/`foreach`/`given`/`match`/`try` catch) per scope frame (parallel to [`Scope`] depth).
    english_lexical_scalars: Vec<HashSet<String>>,
    /// Bare names from `our $x` per frame — same length as [`Self::english_lexical_scalars`].
    our_lexical_scalars: Vec<HashSet<String>>,
    /// When false, the bytecode VM runs without Cranelift (see [`crate::try_vm_execute`]). Disabled by
    /// `PERLRS_NO_JIT=1` / `true` / `yes`, or `pe --no-jit` after [`Self::new`].
    pub vm_jit_enabled: bool,
    /// When true, [`crate::try_vm_execute`] prints bytecode disassembly to stderr before running the VM.
    pub disasm_bytecode: bool,
    /// Sideband: precompiled [`crate::bytecode::Chunk`] loaded from a `.pec` cache hit. When
    /// `Some`, [`crate::try_vm_execute`] uses it directly and skips `compile_program`. Consumed
    /// (`.take()`) on first read so re-entry compiles normally.
    pub pec_precompiled_chunk: Option<crate::bytecode::Chunk>,
    /// Sideband: fingerprint to save the compiled chunk under after a cache miss (pairs with
    /// [`crate::pec::try_save`]). `None` when the cache is disabled or the caller does not want
    /// the compiled chunk persisted.
    pub pec_cache_fingerprint: Option<[u8; 32]>,
    /// Set while stepping a `gen { }` body (`yield`).
    pub(crate) in_generator: bool,
    /// `-n`/`-p` driver: prelude only in [`Self::execute_tree`]; body runs in [`Self::process_line`].
    pub line_mode_skip_main: bool,
    /// Set for the duration of each [`Self::process_line`] call when the current line is the last
    /// from the active input source (stdin or current `@ARGV` file), so `eof` with no arguments
    /// matches Perl (true on the last line of that source).
    pub(crate) line_mode_eof_pending: bool,
    /// `-n`/`-p` stdin driver: lines **peek-read** to compute `eof` / `is_last` are pushed here so
    /// `<>` / `readline` in the body reads them before the real stdin stream (Perl shares one fd).
    pub line_mode_stdin_pending: VecDeque<String>,
    /// Sliding-window timestamps for `rate_limit(...)` (indexed by parse-time slot).
    pub(crate) rate_limit_slots: Vec<VecDeque<Instant>>,
    /// `log_level('…')` override; when `None`, use `%ENV{LOG_LEVEL}` (default `info`).
    pub(crate) log_level_override: Option<LogLevelFilter>,
    /// Stack of currently-executing subroutines for `__SUB__` (anonymous recursion).
    /// Pushed on `call_sub` entry, popped on exit.
    pub(crate) current_sub_stack: Vec<Arc<PerlSub>>,
}

/// Snapshot of stash + `@ISA` for REPL `$obj->method` tab-completion (no `Interpreter` handle needed).
#[derive(Debug, Clone, Default)]
pub struct ReplCompletionSnapshot {
    pub subs: Vec<String>,
    pub blessed_scalars: HashMap<String, String>,
    pub isa_for_class: HashMap<String, Vec<String>>,
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
            out.push_str(&perl_quotemeta(&c.to_string()));
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

/// Copy a Perl character class `[` … `]` from `chars[i]` (must be `'['`) into `out`; return index
/// past the closing `]`.
fn copy_regex_char_class(chars: &[char], mut i: usize, out: &mut String) -> usize {
    debug_assert_eq!(chars.get(i), Some(&'['));
    out.push('[');
    i += 1;
    if i < chars.len() && chars[i] == '^' {
        out.push('^');
        i += 1;
    }
    if i >= chars.len() {
        return i;
    }
    // `]` as the first class character is literal iff another unescaped `]` closes the class
    // (e.g. `[]]` / `[^]]`, or `[]\[^$.*/]`). Otherwise `[]` / `[^]` is an empty class closed by
    // this `]`.
    if chars[i] == ']' {
        if i + 1 < chars.len() && chars[i + 1] == ']' {
            // `[]]` / `[^]]`: literal `]` then the closing `]`.
            out.push(']');
            i += 1;
        } else {
            let mut scan = i + 1;
            let mut found_closing = false;
            while scan < chars.len() {
                if chars[scan] == '\\' && scan + 1 < chars.len() {
                    scan += 2;
                    continue;
                }
                if chars[scan] == ']' {
                    found_closing = true;
                    break;
                }
                scan += 1;
            }
            if found_closing {
                out.push(']');
                i += 1;
            } else {
                out.push(']');
                return i + 1;
            }
        }
    }
    while i < chars.len() && chars[i] != ']' {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(chars[i]);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    if i < chars.len() {
        out.push(']');
        i += 1;
    }
    i
}

/// Perl `$` (without `/m`) matches end-of-string **or** before a single trailing `\n`. Rust's `$`
/// matches only the haystack end, so rewrite bare `$` anchors to `(?:\n?\z)` (after `\Q...\E` and
/// outside character classes). Skips `\$`, `$1`…, `${…}`, and `$name` forms that are not end
/// anchors. When the `/m` flag is present, Rust `(?m)$` already matches line ends like Perl.
fn rewrite_perl_regex_dollar_end_anchor(pat: &str, multiline_flag: bool) -> String {
    if multiline_flag {
        return pat.to_string();
    }
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::with_capacity(pat.len().saturating_add(16));
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push(c);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '[' {
            i = copy_regex_char_class(&chars, i, &mut out);
            continue;
        }
        if c == '$' {
            if let Some(&next) = chars.get(i + 1) {
                if next.is_ascii_digit() {
                    out.push(c);
                    i += 1;
                    continue;
                }
                if next == '{' {
                    out.push(c);
                    i += 1;
                    continue;
                }
                if next.is_ascii_alphanumeric() || next == '_' {
                    out.push(c);
                    i += 1;
                    continue;
                }
            }
            out.push_str("(?:\\n?\\z)");
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
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

/// Home directory for [`getuid`](libc::getuid) when **`HOME`** is missing (OpenSSH uses it for
/// `~/.ssh/config` and keys).
#[cfg(unix)]
fn pw_home_dir_for_current_uid() -> Option<std::ffi::OsString> {
    use libc::{getpwuid_r, getuid};
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStringExt;
    let uid = unsafe { getuid() };
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf = vec![0u8; 16_384];
    let rc = unsafe {
        getpwuid_r(
            uid,
            &mut pw,
            buf.as_mut_ptr().cast::<libc::c_char>(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() || pw.pw_dir.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(pw.pw_dir).to_bytes() };
    if bytes.is_empty() {
        return None;
    }
    Some(std::ffi::OsString::from_vec(bytes.to_vec()))
}

/// Passwd home for a login name (e.g. **`SUDO_USER`** when `pe` runs under `sudo`).
#[cfg(unix)]
fn pw_home_dir_for_login_name(login: &std::ffi::OsStr) -> Option<std::ffi::OsString> {
    use libc::getpwnam_r;
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let bytes = login.as_bytes();
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    let cname = CString::new(bytes).ok()?;
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf = vec![0u8; 16_384];
    let rc = unsafe {
        getpwnam_r(
            cname.as_ptr(),
            &mut pw,
            buf.as_mut_ptr().cast::<libc::c_char>(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() || pw.pw_dir.is_null() {
        return None;
    }
    let dir_bytes = unsafe { CStr::from_ptr(pw.pw_dir).to_bytes() };
    if dir_bytes.is_empty() {
        return None;
    }
    Some(std::ffi::OsString::from_vec(dir_bytes.to_vec()))
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
        // Reflection hashes — populated from `build.rs`-generated tables so
        // they track the real parser/dispatcher/LSP without hand-maintenance.
        // Three hashes instead of the earlier five: values now carry real
        // signal (category strings / aliases / descriptions) so the old
        // set-shaped `%perl_compats`/`%extensions`/`%callable` dropped out
        // — derive those with `grep` + `exists` on `%builtins` / `%aliases`
        // if needed.
        //
        // Registered under `perlrs::*` (no collision with user scripts) plus
        // short single-letter aliases (`%b`, `%a`, `%d`) — safe because the
        // hash sigil namespace is distinct from scalars (`$a`/`$b` sort
        // specials) and subs. Aliases duplicate the map; reflection data is
        // read-only in practice, so divergence isn't a concern.
        let builtins_map = crate::builtins::builtins_hash_map();
        let aliases_map = crate::builtins::aliases_hash_map();
        let descriptions_map = crate::builtins::descriptions_hash_map();
        scope.declare_hash("perlrs::builtins", builtins_map.clone());
        scope.declare_hash("perlrs::aliases", aliases_map.clone());
        scope.declare_hash("perlrs::descriptions", descriptions_map.clone());
        scope.declare_hash("b", builtins_map);
        scope.declare_hash("a", aliases_map);
        scope.declare_hash("d", descriptions_map);
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

        let mut s = Self {
            scope,
            subs: HashMap::new(),
            struct_defs: HashMap::new(),
            file: "-e".to_string(),
            output_handles: HashMap::new(),
            input_handles: HashMap::new(),
            ofs: String::new(),
            ors: String::new(),
            irs: Some("\n".to_string()),
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
            last_stdin_die_bracket: "<STDIN>".to_string(),
            handle_line_numbers: HashMap::new(),
            flip_flop_active: Vec::new(),
            flip_flop_exclusive_left_line: Vec::new(),
            flip_flop_sequence: Vec::new(),
            flip_flop_last_dot: Vec::new(),
            flip_flop_tree: HashMap::new(),
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
            default_print_handle: "STDOUT".to_string(),
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
            open_pragma_utf8: false,
            // Like Perl 5.10+, `say` is enabled by default; `no feature 'say'` disables it.
            feature_bits: FEAT_SAY,
            num_threads: 0, // lazily read from rayon on first parallel op
            regex_cache: HashMap::new(),
            regex_last: None,
            regex_match_memo: None,
            regex_capture_scope_fresh: false,
            regex_pos: HashMap::new(),
            state_vars: HashMap::new(),
            state_bindings_stack: Vec::new(),
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
            special_var_restore_frames: vec![Vec::new()],
            english_enabled: false,
            english_no_match_vars: false,
            english_match_vars_ever_enabled: false,
            english_lexical_scalars: vec![HashSet::new()],
            our_lexical_scalars: vec![HashSet::new()],
            vm_jit_enabled: !matches!(
                std::env::var("PERLRS_NO_JIT"),
                Ok(v)
                    if v == "1"
                        || v.eq_ignore_ascii_case("true")
                        || v.eq_ignore_ascii_case("yes")
            ),
            disasm_bytecode: false,
            pec_precompiled_chunk: None,
            pec_cache_fingerprint: None,
            in_generator: false,
            line_mode_skip_main: false,
            line_mode_eof_pending: false,
            line_mode_stdin_pending: VecDeque::new(),
            rate_limit_slots: Vec::new(),
            log_level_override: None,
            current_sub_stack: Vec::new(),
        };
        s.install_overload_pragma_stubs();
        crate::list_util::install_scalar_util(&mut s);
        crate::list_util::install_sub_util(&mut s);
        s.install_utf8_unicode_to_native_stub();
        s
    }

    /// `utf8::unicode_to_native` — core XS in perl; JSON::PP calls it from BEGIN before utf8_heavy.
    fn install_utf8_unicode_to_native_stub(&mut self) {
        let empty: Block = vec![];
        let key = "utf8::unicode_to_native".to_string();
        self.subs.insert(
            key.clone(),
            Arc::new(PerlSub {
                name: key,
                params: vec![],
                body: empty,
                prototype: None,
                closure_env: None,
                fib_like: None,
            }),
        );
    }

    /// `overload::import` / `overload::unimport` — core stubs used by CPAN modules (e.g.
    /// `JSON::PP::Boolean`) before real `overload.pm` is modeled. Empty bodies are enough for
    /// strict subs and to satisfy `use overload ();` call sites.
    fn install_overload_pragma_stubs(&mut self) {
        let empty: Block = vec![];
        for key in ["overload::import", "overload::unimport"] {
            let name = key.to_string();
            self.subs.insert(
                name.clone(),
                Arc::new(PerlSub {
                    name,
                    params: vec![],
                    body: empty.clone(),
                    prototype: None,
                    closure_env: None,
                    fib_like: None,
                }),
            );
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
            last_stdin_die_bracket: "<STDIN>".to_string(),
            handle_line_numbers: HashMap::new(),
            flip_flop_active: Vec::new(),
            flip_flop_exclusive_left_line: Vec::new(),
            flip_flop_sequence: Vec::new(),
            flip_flop_last_dot: Vec::new(),
            flip_flop_tree: HashMap::new(),
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
            default_print_handle: self.default_print_handle.clone(),
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
            open_pragma_utf8: self.open_pragma_utf8,
            feature_bits: self.feature_bits,
            num_threads: 0,
            regex_cache: self.regex_cache.clone(),
            regex_last: self.regex_last.clone(),
            regex_match_memo: self.regex_match_memo.clone(),
            regex_capture_scope_fresh: false,
            regex_pos: self.regex_pos.clone(),
            state_vars: self.state_vars.clone(),
            state_bindings_stack: Vec::new(),
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
            special_var_restore_frames: self.special_var_restore_frames.clone(),
            english_enabled: self.english_enabled,
            english_no_match_vars: self.english_no_match_vars,
            english_match_vars_ever_enabled: self.english_match_vars_ever_enabled,
            english_lexical_scalars: self.english_lexical_scalars.clone(),
            our_lexical_scalars: self.our_lexical_scalars.clone(),
            vm_jit_enabled: self.vm_jit_enabled,
            disasm_bytecode: self.disasm_bytecode,
            // Sideband cache fields belong to the top-level driver, not line-mode workers.
            pec_precompiled_chunk: None,
            pec_cache_fingerprint: None,
            in_generator: false,
            line_mode_skip_main: false,
            line_mode_eof_pending: false,
            line_mode_stdin_pending: VecDeque::new(),
            rate_limit_slots: Vec::new(),
            log_level_override: self.log_level_override,
            current_sub_stack: Vec::new(),
        }
    }

    /// Rayon pool size (`pe -j`); lazily initialized from `rayon::current_num_threads()`.
    pub(crate) fn parallel_thread_count(&mut self) -> usize {
        if self.num_threads == 0 {
            self.num_threads = rayon::current_num_threads();
        }
        self.num_threads
    }

    /// `puniq` / `pfirst` / `pany` — parallel list builtins ([`crate::par_list`]).
    pub(crate) fn eval_par_list_call(
        &mut self,
        name: &str,
        args: &[PerlValue],
        ctx: WantarrayCtx,
        line: usize,
    ) -> PerlResult<PerlValue> {
        match name {
            "puniq" => {
                let (list_src, show_prog) = match args.len() {
                    0 => return Err(PerlError::runtime("puniq: expected LIST", line)),
                    1 => (&args[0], false),
                    2 => (&args[0], args[1].is_true()),
                    _ => {
                        return Err(PerlError::runtime(
                            "puniq: expected LIST [, progress => EXPR]",
                            line,
                        ));
                    }
                };
                let list = list_src.to_list();
                let n_threads = self.parallel_thread_count();
                let pmap_progress = PmapProgress::new(show_prog, list.len());
                let out = crate::par_list::puniq_run(list, n_threads, &pmap_progress);
                pmap_progress.finish();
                if ctx == WantarrayCtx::List {
                    Ok(PerlValue::array(out))
                } else {
                    Ok(PerlValue::integer(out.len() as i64))
                }
            }
            "pfirst" => {
                let (code_val, list_src, show_prog) = match args.len() {
                    2 => (&args[0], &args[1], false),
                    3 => (&args[0], &args[1], args[2].is_true()),
                    _ => {
                        return Err(PerlError::runtime(
                            "pfirst: expected BLOCK, LIST [, progress => EXPR]",
                            line,
                        ));
                    }
                };
                let Some(sub) = code_val.as_code_ref() else {
                    return Err(PerlError::runtime(
                        "pfirst: first argument must be a code reference",
                        line,
                    ));
                };
                let sub = sub.clone();
                let list = list_src.to_list();
                if list.is_empty() {
                    return Ok(PerlValue::UNDEF);
                }
                let pmap_progress = PmapProgress::new(show_prog, list.len());
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();
                let out = crate::par_list::pfirst_run(list, &pmap_progress, |item| {
                    let mut local_interp = Interpreter::new();
                    local_interp.subs = subs.clone();
                    local_interp.scope.restore_capture(&scope_capture);
                    local_interp
                        .scope
                        .restore_atomics(&atomic_arrays, &atomic_hashes);
                    local_interp.enable_parallel_guard();
                    local_interp.scope.set_topic(item);
                    match local_interp.call_sub(sub.as_ref(), vec![], WantarrayCtx::Scalar, line) {
                        Ok(v) => v.is_true(),
                        Err(_) => false,
                    }
                });
                pmap_progress.finish();
                Ok(out.unwrap_or(PerlValue::UNDEF))
            }
            "pany" => {
                let (code_val, list_src, show_prog) = match args.len() {
                    2 => (&args[0], &args[1], false),
                    3 => (&args[0], &args[1], args[2].is_true()),
                    _ => {
                        return Err(PerlError::runtime(
                            "pany: expected BLOCK, LIST [, progress => EXPR]",
                            line,
                        ));
                    }
                };
                let Some(sub) = code_val.as_code_ref() else {
                    return Err(PerlError::runtime(
                        "pany: first argument must be a code reference",
                        line,
                    ));
                };
                let sub = sub.clone();
                let list = list_src.to_list();
                let pmap_progress = PmapProgress::new(show_prog, list.len());
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();
                let b = crate::par_list::pany_run(list, &pmap_progress, |item| {
                    let mut local_interp = Interpreter::new();
                    local_interp.subs = subs.clone();
                    local_interp.scope.restore_capture(&scope_capture);
                    local_interp
                        .scope
                        .restore_atomics(&atomic_arrays, &atomic_hashes);
                    local_interp.enable_parallel_guard();
                    local_interp.scope.set_topic(item);
                    match local_interp.call_sub(sub.as_ref(), vec![], WantarrayCtx::Scalar, line) {
                        Ok(v) => v.is_true(),
                        Err(_) => false,
                    }
                });
                pmap_progress.finish();
                Ok(PerlValue::integer(if b { 1 } else { 0 }))
            }
            _ => Err(PerlError::runtime(
                format!("internal: unknown par_list builtin {name}"),
                line,
            )),
        }
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

    /// `ssh LIST` — run the real `ssh` binary with `LIST` as argv (no `sh -c`).
    ///
    /// **`Host` aliases in `~/.ssh/config`** are honored by OpenSSH like in a normal shell (same
    /// binary, inherited env). **Shell** `alias` / functions are not applied (no `sh -c`). If
    /// **`HOME`** is unset, on Unix we set it from the passwd DB so config and keys resolve.
    ///
    /// **`sudo`:** the child `ssh` normally sees **`HOME=/root`**, so it reads **`/root/.ssh/config`**
    /// and host aliases in *your* config are missing. When **`SUDO_USER`** is set and the effective
    /// uid is **0**, we set **`HOME`** for this subprocess to **`SUDO_USER`'s** passwd home so your
    /// `~/.ssh/config` and keys apply.
    pub(crate) fn ssh_builtin_execute(&mut self, args: &[PerlValue]) -> PerlResult<PerlValue> {
        use std::process::Command;
        let mut cmd = Command::new("ssh");
        #[cfg(unix)]
        {
            use libc::geteuid;
            let home_for_ssh = if unsafe { geteuid() } == 0 {
                std::env::var_os("SUDO_USER").and_then(|u| pw_home_dir_for_login_name(&u))
            } else {
                None
            };
            if let Some(h) = home_for_ssh {
                cmd.env("HOME", h);
            } else if std::env::var_os("HOME").is_none() {
                if let Some(h) = pw_home_dir_for_current_uid() {
                    cmd.env("HOME", h);
                }
            }
        }
        for a in args {
            cmd.arg(a.to_string());
        }
        match cmd.status() {
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

    /// Package stash key for `our $name` (same rule as [`Compiler::qualify_stash_scalar_name`]).
    pub(crate) fn stash_scalar_name_for_package(&self, name: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        let pkg = self.current_package();
        if pkg.is_empty() || pkg == "main" {
            format!("main::{}", name)
        } else {
            format!("{}::{}", pkg, name)
        }
    }

    /// Tree-walker: bare `$x` after `our $x` reads the package stash scalar (`main::x` / `Pkg::x`).
    pub(crate) fn tree_scalar_storage_name(&self, name: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        for (lex, our) in self
            .english_lexical_scalars
            .iter()
            .zip(self.our_lexical_scalars.iter())
            .rev()
        {
            if lex.contains(name) {
                if our.contains(name) {
                    return self.stash_scalar_name_for_package(name);
                }
                return name.to_string();
            }
        }
        name.to_string()
    }

    /// Shared by tree `StmtKind::Tie` and bytecode [`crate::bytecode::Op::Tie`].
    pub(crate) fn tie_execute(
        &mut self,
        target_kind: u8,
        target_name: &str,
        class_and_args: Vec<PerlValue>,
        line: usize,
    ) -> PerlResult<PerlValue> {
        let mut it = class_and_args.into_iter();
        let class = it.next().unwrap_or(PerlValue::UNDEF);
        let pkg = class.to_string();
        let pkg = pkg.trim_matches(|c| c == '\'' || c == '"').to_string();
        let tie_ctor = match target_kind {
            0 => "TIESCALAR",
            1 => "TIEARRAY",
            2 => "TIEHASH",
            _ => return Err(PerlError::runtime("tie: invalid target kind", line)),
        };
        let tie_fn = format!("{}::{}", pkg, tie_ctor);
        let sub = self
            .subs
            .get(&tie_fn)
            .cloned()
            .ok_or_else(|| PerlError::runtime(format!("tie: cannot find &{}", tie_fn), line))?;
        let mut call_args = vec![PerlValue::string(pkg.clone())];
        call_args.extend(it);
        let obj = match self.call_sub(&sub, call_args, WantarrayCtx::Scalar, line) {
            Ok(v) => v,
            Err(FlowOrError::Flow(_)) => PerlValue::UNDEF,
            Err(FlowOrError::Error(e)) => return Err(e),
        };
        match target_kind {
            0 => {
                self.tied_scalars.insert(target_name.to_string(), obj);
            }
            1 => {
                let key = self.stash_array_name_for_package(target_name);
                self.tied_arrays.insert(key, obj);
            }
            2 => {
                self.tied_hashes.insert(target_name.to_string(), obj);
            }
            _ => return Err(PerlError::runtime("tie: invalid target kind", line)),
        }
        Ok(PerlValue::UNDEF)
    }

    /// Immediate parents from live `@Class::ISA` (no cached MRO — changes take effect on next method lookup).
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
        if let Some(alias) = self.glob_handle_alias.get(name) {
            return alias.clone();
        }
        // `print $fh …` stores the handle as "$varname"; resolve it by
        // reading the scalar variable which holds the IO handle name.
        if let Some(var_name) = name.strip_prefix('$') {
            let val = self.scope.get_scalar(var_name);
            let s = val.to_string();
            if !s.is_empty() {
                return self.resolve_io_handle_name(&s);
            }
        }
        name.to_string()
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
    pub(crate) fn copy_typeglob_slots(
        &mut self,
        lhs: &str,
        rhs: &str,
        line: usize,
    ) -> PerlResult<()> {
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

    /// `format NAME =` … — register under `current_package::NAME` (VM [`crate::bytecode::Op::FormatDecl`] and tree).
    pub(crate) fn install_format_decl(
        &mut self,
        basename: &str,
        lines: &[String],
        line: usize,
    ) -> PerlResult<()> {
        let pkg = self.current_package();
        let key = format!("{}::{}", pkg, basename);
        let tmpl = crate::format::parse_format_template(lines).map_err(|e| e.at_line(line))?;
        self.format_templates.insert(key, Arc::new(tmpl));
        Ok(())
    }

    /// `use overload` — merge pairs into [`Self::overload_table`] for [`Self::current_package`].
    pub(crate) fn install_use_overload_pairs(&mut self, pairs: &[(String, String)]) {
        let pkg = self.current_package();
        let ent = self.overload_table.entry(pkg).or_default();
        for (k, v) in pairs {
            ent.insert(k.clone(), v.clone());
        }
    }

    /// `local *LHS` / `local *LHS = *RHS` — save/restore [`Self::glob_handle_alias`] like the tree
    /// [`StmtKind::Local`] / [`StmtKind::LocalExpr`] paths.
    pub(crate) fn local_declare_typeglob(
        &mut self,
        lhs: &str,
        rhs: Option<&str>,
        line: usize,
    ) -> PerlResult<()> {
        let old = self.glob_handle_alias.remove(lhs);
        let Some(frame) = self.glob_restore_frames.last_mut() else {
            return Err(PerlError::runtime(
                "internal: no glob restore frame for local *GLOB",
                line,
            ));
        };
        frame.push((lhs.to_string(), old));
        if let Some(r) = rhs {
            self.glob_handle_alias
                .insert(lhs.to_string(), r.to_string());
        }
        Ok(())
    }

    pub(crate) fn scope_push_hook(&mut self) {
        self.scope.push_frame();
        self.glob_restore_frames.push(Vec::new());
        self.special_var_restore_frames.push(Vec::new());
        self.english_lexical_scalars.push(HashSet::new());
        self.our_lexical_scalars.push(HashSet::new());
        self.state_bindings_stack.push(Vec::new());
    }

    #[inline]
    pub(crate) fn english_note_lexical_scalar(&mut self, name: &str) {
        if let Some(s) = self.english_lexical_scalars.last_mut() {
            s.insert(name.to_string());
        }
    }

    #[inline]
    fn note_our_scalar(&mut self, bare_name: &str) {
        if let Some(s) = self.our_lexical_scalars.last_mut() {
            s.insert(bare_name.to_string());
        }
    }

    pub(crate) fn scope_pop_hook(&mut self) {
        if !self.scope.can_pop_frame() {
            return;
        }
        // Execute deferred blocks in LIFO order before popping the frame
        let defers = self.scope.take_defers();
        for coderef in defers {
            if let Some(sub) = coderef.as_code_ref() {
                // Defers run in void context, errors are silently ignored
                let _ = self.call_sub(&sub, vec![], WantarrayCtx::Void, 0);
            }
        }
        // Save state variable values back before popping the frame
        if let Some(bindings) = self.state_bindings_stack.pop() {
            for (var_name, state_key) in &bindings {
                let val = self.scope.get_scalar(var_name).clone();
                self.state_vars.insert(state_key.clone(), val);
            }
        }
        // `local $/` / `$\` / `$,` / `$"` etc. — restore each special-var backing field
        // BEFORE the scope frame is popped, since `set_special_var` may consult `self.scope`.
        if let Some(entries) = self.special_var_restore_frames.pop() {
            for (name, old) in entries.into_iter().rev() {
                let _ = self.set_special_var(&name, &old);
            }
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
        let _ = self.our_lexical_scalars.pop();
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
    ///
    /// Unset `%SIG` entries and the string **`DEFAULT`** mean **POSIX default** for that signal (not
    /// IGNORE). That matters for `SIGINT` / `SIGTERM` / `SIGALRM`, where default is terminate — so
    /// Ctrl+C is not “trapped” when no handler is installed (including parallel `pmap` / `progress`
    /// workers that call `perl_signal::poll`).
    pub(crate) fn invoke_sig_handler(&mut self, sig: &str) -> PerlResult<()> {
        self.touch_env_hash("SIG");
        let v = self.scope.get_hash_element("SIG", sig);
        if v.is_undef() {
            return Self::default_sig_action(sig);
        }
        if let Some(s) = v.as_str() {
            if s == "IGNORE" {
                return Ok(());
            }
            if s == "DEFAULT" {
                return Self::default_sig_action(sig);
            }
        }
        if let Some(sub) = v.as_code_ref() {
            match self.call_sub(&sub, vec![], WantarrayCtx::Scalar, 0) {
                Ok(_) => Ok(()),
                Err(FlowOrError::Flow(_)) => Ok(()),
                Err(FlowOrError::Error(e)) => Err(e),
            }
        } else {
            Self::default_sig_action(sig)
        }
    }

    /// POSIX default for signals we deliver via `perl_signal::poll` (Unix).
    #[inline]
    fn default_sig_action(sig: &str) -> PerlResult<()> {
        match sig {
            // 128 + signal number (common shell convention)
            "INT" => std::process::exit(130),
            "TERM" => std::process::exit(143),
            "ALRM" => std::process::exit(142),
            // Default for SIGCHLD is ignore
            "CHLD" => Ok(()),
            _ => Ok(()),
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

    /// Effective minimum log level (`log_level()` override, else `$ENV{LOG_LEVEL}`, else `info`).
    pub(crate) fn log_filter_effective(&mut self) -> LogLevelFilter {
        self.materialize_env_if_needed();
        if let Some(x) = self.log_level_override {
            return x;
        }
        let s = self.scope.get_hash_element("ENV", "LOG_LEVEL").to_string();
        LogLevelFilter::parse(&s).unwrap_or(LogLevelFilter::Info)
    }

    /// <https://no-color.org/> — non-empty `$ENV{NO_COLOR}` disables ANSI in `log_*`.
    pub(crate) fn no_color_effective(&mut self) -> bool {
        self.materialize_env_if_needed();
        let v = self.scope.get_hash_element("ENV", "NO_COLOR");
        if v.is_undef() {
            return false;
        }
        !v.to_string().is_empty()
    }

    #[inline]
    pub(crate) fn touch_env_hash(&mut self, hash_name: &str) {
        if hash_name == "ENV" {
            self.materialize_env_if_needed();
        }
    }

    /// `exists $href->{k}` / `exists $obj->{k}` — container is a hash ref or blessed hash-like value.
    pub(crate) fn exists_arrow_hash_element(
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
    pub(crate) fn delete_arrow_hash_element(
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

    /// `exists $aref->[$i]` — plain array ref only (same index rules as [`Self::read_arrow_array_element`]).
    pub(crate) fn exists_arrow_array_element(
        &self,
        container: PerlValue,
        idx: i64,
        line: usize,
    ) -> PerlResult<bool> {
        if let Some(a) = container.as_array_ref() {
            let arr = a.read();
            let i = if idx < 0 {
                (arr.len() as i64 + idx) as usize
            } else {
                idx as usize
            };
            return Ok(i < arr.len());
        }
        Err(PerlError::runtime(
            "exists argument is not an ARRAY reference",
            line,
        ))
    }

    /// `delete $aref->[$i]` — sets element to undef, returns previous value (Perl array `delete`).
    pub(crate) fn delete_arrow_array_element(
        &self,
        container: PerlValue,
        idx: i64,
        line: usize,
    ) -> PerlResult<PerlValue> {
        if let Some(a) = container.as_array_ref() {
            let mut arr = a.write();
            let i = if idx < 0 {
                (arr.len() as i64 + idx) as usize
            } else {
                idx as usize
            };
            if i >= arr.len() {
                return Ok(PerlValue::UNDEF);
            }
            let old = arr.get(i).cloned().unwrap_or(PerlValue::UNDEF);
            arr[i] = PerlValue::UNDEF;
            return Ok(old);
        }
        Err(PerlError::runtime(
            "delete argument is not an ARRAY reference",
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
    pub(crate) fn strict_scalar_exempt(name: &str) -> bool {
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
            || (name.starts_with('#') && name.len() > 1)
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
    /// `sub Q::name { }` is already fully qualified — do not prepend the current package.
    pub(crate) fn qualify_sub_key(&self, name: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        let pkg = self.current_package();
        if pkg.is_empty() || pkg == "main" {
            name.to_string()
        } else {
            format!("{}::{}", pkg, name)
        }
    }

    /// `Undefined subroutine &name` (bare calls) with optional `strict subs` hint.
    pub(crate) fn undefined_subroutine_call_message(&self, name: &str) -> String {
        let mut msg = format!("Undefined subroutine &{}", name);
        if self.strict_subs {
            msg.push_str(
                " (strict subs: declare the sub or use a fully qualified name before calling)",
            );
        }
        msg
    }

    /// `Undefined subroutine pkg::name` (coderef resolution) with optional `strict subs` hint.
    pub(crate) fn undefined_subroutine_resolve_message(&self, name: &str) -> String {
        let mut msg = format!("Undefined subroutine {}", self.qualify_sub_key(name));
        if self.strict_subs {
            msg.push_str(
                " (strict subs: declare the sub or use a fully qualified name before calling)",
            );
        }
        msg
    }

    /// Where `use` imports a symbol: `main` → short name; otherwise `Pkg::sym`.
    fn import_alias_key(&self, short: &str) -> String {
        self.qualify_sub_key(short)
    }

    /// `use Module qw()` / `use Module ()` — explicit empty list (not the same as `use Module`).
    fn is_explicit_empty_import_list(imports: &[Expr]) -> bool {
        if imports.len() == 1 {
            match &imports[0].kind {
                ExprKind::QW(ws) => return ws.is_empty(),
                // Parser: `use Carp ()` → one import that is an empty `List` (see `parse_use`).
                ExprKind::List(xs) => return xs.is_empty(),
                _ => {}
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
                let mut q = String::with_capacity(pkg.len() + 2 + name.len());
                q.push_str(&pkg);
                q.push_str("::");
                q.push_str(name);
                return self.subs.get(&q).cloned();
            }
        }
        None
    }

    /// `use Module VERSION LIST` — numeric `VERSION` is not part of the import list (Perl strips it
    /// before calling `import`).
    fn imports_after_leading_use_version(imports: &[Expr]) -> &[Expr] {
        if let Some(first) = imports.first() {
            if matches!(first.kind, ExprKind::Integer(_) | ExprKind::Float(_)) {
                return &imports[1..];
            }
        }
        imports
    }

    /// Compile-time pragma import list (`'refs'`, `qw(refs subs)`, version integers).
    fn pragma_import_strings(imports: &[Expr], default_line: usize) -> PerlResult<Vec<String>> {
        let mut out = Vec::new();
        for e in imports {
            match &e.kind {
                ExprKind::String(s) => out.push(s.clone()),
                ExprKind::QW(ws) => out.extend(ws.iter().cloned()),
                ExprKind::Integer(n) => out.push(n.to_string()),
                // `use Env "@PATH"` / `use Env "$HOME"` — double-quoted string containing
                // a single interpolated variable.  Reconstruct the sigil+name form.
                ExprKind::InterpolatedString(parts) => {
                    let mut s = String::new();
                    for p in parts {
                        match p {
                            StringPart::Literal(l) => s.push_str(l),
                            StringPart::ScalarVar(v) => {
                                s.push('$');
                                s.push_str(v);
                            }
                            StringPart::ArrayVar(v) => {
                                s.push('@');
                                s.push_str(v);
                            }
                            _ => {
                                return Err(PerlError::runtime(
                                    "pragma import must be a compile-time string, qw(), or integer",
                                    e.line.max(default_line),
                                ));
                            }
                        }
                    }
                    out.push(s);
                }
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
            // Features that perlrs accepts as known but tracks no separate bit for —
            // either always-on, always-off, or syntactic sugar already enabled.
            // Keeps `use feature 'X'` from erroring on common Perl 5.20+ pragmas.
            "postderef"
            | "postderef_qq"
            | "evalbytes"
            | "current_sub"
            | "fc"
            | "lexical_subs"
            | "signatures"
            | "refaliasing"
            | "bitwise"
            | "isa"
            | "indirect"
            | "multidimensional"
            | "bareword_filehandles"
            | "try"
            | "defer"
            | "extra_paired_delimiters"
            | "module_true"
            | "class"
            | "array_base" => return Ok(()),
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
        let code = read_file_text_perl_compat(&canon).map_err(|e| {
            PerlError::runtime(
                format!("Can't open {} for reading: {}", canon.display(), e),
                line,
            )
        })?;
        let code = crate::data_section::strip_perl_end_marker(&code);
        self.scope
            .set_hash_element("INC", &key, PerlValue::string(key.clone()))?;
        let saved_pkg = self.scope.get_scalar("__PACKAGE__");
        let r = crate::parse_and_run_string_in_file(code, self, &key);
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
                let code = read_file_text_perl_compat(&full).map_err(|e| {
                    PerlError::runtime(
                        format!("Can't open {} for reading: {}", full.display(), e),
                        line,
                    )
                })?;
                let code = crate::data_section::strip_perl_end_marker(&code);
                let abs = full.canonicalize().unwrap_or(full);
                let abs_s = abs.to_string_lossy().into_owned();
                self.scope
                    .set_hash_element("INC", relpath, PerlValue::string(abs_s.clone()))?;
                let saved_pkg = self.scope.get_scalar("__PACKAGE__");
                let r = crate::parse_and_run_string_in_file(code, self, &abs_s);
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
                let args = Self::pragma_import_strings(imports, line)?;
                let no_match = args.iter().any(|a| a == "-no_match_vars");
                // Once match vars are exported (use English without -no_match_vars),
                // they stay available for the rest of the program — Perl exports them
                // into the caller's namespace and later pragmas cannot un-export them.
                if !no_match {
                    self.english_match_vars_ever_enabled = true;
                }
                self.english_no_match_vars = no_match && !self.english_match_vars_ever_enabled;
                Ok(())
            }
            "Env" => self.apply_use_env(imports, line),
            "open" => self.apply_use_open(imports, line),
            "constant" => self.apply_use_constant(imports, line),
            "threads" | "Thread::Pool" | "Parallel::ForkManager" => Ok(()),
            _ => {
                self.require_execute(module, line)?;
                let imports = Self::imports_after_leading_use_version(imports);
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
                // Don't reset no_match_vars here — if match vars were ever enabled,
                // they persist (Perl's export cannot be un-exported).
                if !self.english_match_vars_ever_enabled {
                    self.english_no_match_vars = false;
                }
                Ok(())
            }
            "open" => {
                self.open_pragma_utf8 = false;
                Ok(())
            }
            "threads" | "Thread::Pool" | "Parallel::ForkManager" => Ok(()),
            _ => Ok(()),
        }
    }

    /// `use Env qw(@PATH)` / `use Env '@PATH'` — populate `%ENV`-style paths from the process environment.
    fn apply_use_env(&mut self, imports: &[Expr], line: usize) -> PerlResult<()> {
        let names = Self::pragma_import_strings(imports, line)?;
        for n in names {
            let key = n.trim_start_matches('@');
            if key.eq_ignore_ascii_case("PATH") {
                let path_env = std::env::var("PATH").unwrap_or_default();
                let path_vec: Vec<PerlValue> = std::env::split_paths(&path_env)
                    .map(|p| PerlValue::string(p.to_string_lossy().into_owned()))
                    .collect();
                let aname = self.stash_array_name_for_package("PATH");
                self.scope.declare_array(&aname, path_vec);
            }
        }
        Ok(())
    }

    /// `use open ':encoding(UTF-8)'`, `qw(:std :encoding(UTF-8))`, `:utf8`, etc.
    fn apply_use_open(&mut self, imports: &[Expr], line: usize) -> PerlResult<()> {
        let items = Self::pragma_import_strings(imports, line)?;
        for item in items {
            let s = item.trim();
            if s.eq_ignore_ascii_case(":utf8") || s == ":std" || s.eq_ignore_ascii_case("std") {
                self.open_pragma_utf8 = true;
                continue;
            }
            if let Some(rest) = s.strip_prefix(":encoding(") {
                if let Some(inner) = rest.strip_suffix(')') {
                    if inner.eq_ignore_ascii_case("UTF-8") || inner.eq_ignore_ascii_case("utf8") {
                        self.open_pragma_utf8 = true;
                    }
                }
            }
        }
        Ok(())
    }

    /// `use constant NAME => EXPR` / `use constant 1.03` — do not load core `constant.pm` (it uses syntax we do not parse yet).
    fn apply_use_constant(&mut self, imports: &[Expr], line: usize) -> PerlResult<()> {
        if imports.is_empty() {
            return Ok(());
        }
        // `use constant 1.03;` — version check only (ignored here).
        if imports.len() == 1 {
            match &imports[0].kind {
                ExprKind::Float(_) | ExprKind::Integer(_) => return Ok(()),
                _ => {}
            }
        }
        for imp in imports {
            match &imp.kind {
                ExprKind::List(items) => {
                    if items.len() % 2 != 0 {
                        return Err(PerlError::runtime(
                            format!(
                                "use constant: expected even-length list of NAME => VALUE pairs, got {}",
                                items.len()
                            ),
                            line,
                        ));
                    }
                    let mut i = 0;
                    while i < items.len() {
                        let name = match &items[i].kind {
                            ExprKind::String(s) => s.clone(),
                            _ => {
                                return Err(PerlError::runtime(
                                    "use constant: constant name must be a string literal",
                                    line,
                                ));
                            }
                        };
                        let val = match self.eval_expr(&items[i + 1]) {
                            Ok(v) => v,
                            Err(FlowOrError::Error(e)) => return Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                return Err(PerlError::runtime(
                                    "use constant: unexpected control flow in initializer",
                                    line,
                                ));
                            }
                        };
                        self.install_constant_sub(&name, &val, line)?;
                        i += 2;
                    }
                }
                _ => {
                    return Err(PerlError::runtime(
                        "use constant: expected list of NAME => VALUE pairs",
                        line,
                    ));
                }
            }
        }
        Ok(())
    }

    fn install_constant_sub(&mut self, name: &str, val: &PerlValue, line: usize) -> PerlResult<()> {
        let key = self.qualify_sub_key(name);
        let ret_expr = self.perl_value_to_const_literal_expr(val, line)?;
        let body = vec![Statement {
            label: None,
            kind: StmtKind::Return(Some(ret_expr)),
            line,
        }];
        self.subs.insert(
            key.clone(),
            Arc::new(PerlSub {
                name: key,
                params: vec![],
                body,
                prototype: None,
                closure_env: None,
                fib_like: None,
            }),
        );
        Ok(())
    }

    /// Build a literal expression for `return EXPR` in a constant sub (scalar/aggregate only).
    fn perl_value_to_const_literal_expr(&self, v: &PerlValue, line: usize) -> PerlResult<Expr> {
        if v.is_undef() {
            return Ok(Expr {
                kind: ExprKind::Undef,
                line,
            });
        }
        if let Some(n) = v.as_integer() {
            return Ok(Expr {
                kind: ExprKind::Integer(n),
                line,
            });
        }
        if let Some(f) = v.as_float() {
            return Ok(Expr {
                kind: ExprKind::Float(f),
                line,
            });
        }
        if let Some(s) = v.as_str() {
            return Ok(Expr {
                kind: ExprKind::String(s),
                line,
            });
        }
        if let Some(arr) = v.as_array_vec() {
            let mut elems = Vec::with_capacity(arr.len());
            for e in &arr {
                elems.push(self.perl_value_to_const_literal_expr(e, line)?);
            }
            return Ok(Expr {
                kind: ExprKind::ArrayRef(elems),
                line,
            });
        }
        if let Some(h) = v.as_hash_map() {
            let mut pairs = Vec::with_capacity(h.len());
            for (k, vv) in h.iter() {
                pairs.push((
                    Expr {
                        kind: ExprKind::String(k.clone()),
                        line,
                    },
                    self.perl_value_to_const_literal_expr(vv, line)?,
                ));
            }
            return Ok(Expr {
                kind: ExprKind::HashRef(pairs),
                line,
            });
        }
        if let Some(aref) = v.as_array_ref() {
            let arr = aref.read();
            let mut elems = Vec::with_capacity(arr.len());
            for e in arr.iter() {
                elems.push(self.perl_value_to_const_literal_expr(e, line)?);
            }
            return Ok(Expr {
                kind: ExprKind::ArrayRef(elems),
                line,
            });
        }
        if let Some(href) = v.as_hash_ref() {
            let h = href.read();
            let mut pairs = Vec::with_capacity(h.len());
            for (k, vv) in h.iter() {
                pairs.push((
                    Expr {
                        kind: ExprKind::String(k.clone()),
                        line,
                    },
                    self.perl_value_to_const_literal_expr(vv, line)?,
                ));
            }
            return Ok(Expr {
                kind: ExprKind::HashRef(pairs),
                line,
            });
        }
        Err(PerlError::runtime(
            format!("use constant: unsupported value type ({v:?})"),
            line,
        ))
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
                StmtKind::UsePerlVersion { .. } => {}
                StmtKind::Use { module, imports } => {
                    self.exec_use_stmt(module, imports, stmt.line)?;
                }
                StmtKind::UseOverload { pairs } => {
                    self.install_use_overload_pairs(pairs);
                }
                StmtKind::FormatDecl { name, lines } => {
                    self.install_format_decl(name, lines, stmt.line)?;
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
    ///
    /// Two-arg `open $fh, EXPR` with a single string: Perl treats a leading `|` as pipe-to-command
    /// (`|-`) and a trailing `|` as pipe-from-command (`-|`), both via `sh -c` / `cmd /C` (see
    /// [`piped_shell_command`]).
    pub(crate) fn open_builtin_execute(
        &mut self,
        handle_name: String,
        mode_s: String,
        file_opt: Option<String>,
        line: usize,
    ) -> PerlResult<PerlValue> {
        // Perl two-arg `open $fh, EXPR` when EXPR is a single string:
        // - leading `|`  → pipe to command (write to child's stdin)
        // - trailing `|` → pipe from command (read child's stdout)
        // (Must run before `<` / `>` so `"| cmd"` is not treated as a filename.)
        let (actual_mode, path) = if let Some(f) = file_opt {
            (mode_s, f)
        } else {
            let trimmed = mode_s.trim();
            if let Some(rest) = trimmed.strip_prefix('|') {
                ("|-".to_string(), rest.trim_start().to_string())
            } else if trimmed.ends_with('|') {
                let mut cmd = trimmed.to_string();
                cmd.pop(); // trailing `|` that selects pipe-from-command
                ("-|".to_string(), cmd.trim_end().to_string())
            } else if let Some(rest) = trimmed.strip_prefix(">>") {
                (">>".to_string(), rest.trim().to_string())
            } else if let Some(rest) = trimmed.strip_prefix('>') {
                (">".to_string(), rest.trim().to_string())
            } else if let Some(rest) = trimmed.strip_prefix('<') {
                ("<".to_string(), rest.trim().to_string())
            } else {
                ("<".to_string(), trimmed.to_string())
            }
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
                let shared = Arc::new(Mutex::new(file));
                self.io_file_slots
                    .insert(handle_name.clone(), Arc::clone(&shared));
                self.input_handles.insert(
                    handle_name.clone(),
                    BufReader::new(Box::new(IoSharedFile(Arc::clone(&shared)))),
                );
            }
            ">" => {
                let file = std::fs::File::create(&path).map_err(|e| {
                    self.apply_io_error_to_errno(&e);
                    PerlError::runtime(format!("Can't open '{}': {}", path, e), line)
                })?;
                let shared = Arc::new(Mutex::new(file));
                self.io_file_slots
                    .insert(handle_name.clone(), Arc::clone(&shared));
                self.output_handles.insert(
                    handle_name.clone(),
                    Box::new(IoSharedFileWrite(Arc::clone(&shared))),
                );
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
                let shared = Arc::new(Mutex::new(file));
                self.io_file_slots
                    .insert(handle_name.clone(), Arc::clone(&shared));
                self.output_handles.insert(
                    handle_name.clone(),
                    Box::new(IoSharedFileWrite(Arc::clone(&shared))),
                );
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

    /// `group_by` / `chunk_by` — consecutive runs where the key (block or `EXPR` with `$_`)
    /// matches the previous key under [`PerlValue::str_eq`]. Returns a list of arrayrefs
    /// (same outer shape as `chunked`).
    pub(crate) fn eval_chunk_by_builtin(
        &mut self,
        key_spec: &Expr,
        list_expr: &Expr,
        ctx: WantarrayCtx,
        line: usize,
    ) -> ExecResult {
        let list = self.eval_expr_ctx(list_expr, WantarrayCtx::List)?.to_list();
        let chunks = match &key_spec.kind {
            ExprKind::CodeRef { .. } => {
                let cr = self.eval_expr(key_spec)?;
                let Some(sub) = cr.as_code_ref() else {
                    return Err(PerlError::runtime(
                        "group_by/chunk_by: first argument must be { BLOCK }",
                        line,
                    )
                    .into());
                };
                let sub = sub.clone();
                let mut chunks: Vec<PerlValue> = Vec::new();
                let mut run: Vec<PerlValue> = Vec::new();
                let mut prev_key: Option<PerlValue> = None;
                for item in list {
                    self.scope.set_topic(item.clone());
                    let key = match self.call_sub(&sub, vec![], WantarrayCtx::Scalar, line) {
                        Ok(k) => k,
                        Err(FlowOrError::Error(e)) => return Err(FlowOrError::Error(e)),
                        Err(FlowOrError::Flow(Flow::Return(v))) => v,
                        Err(_) => PerlValue::UNDEF,
                    };
                    match &prev_key {
                        None => {
                            run.push(item);
                            prev_key = Some(key);
                        }
                        Some(pk) => {
                            if key.str_eq(pk) {
                                run.push(item);
                            } else {
                                chunks.push(PerlValue::array_ref(Arc::new(RwLock::new(
                                    std::mem::take(&mut run),
                                ))));
                                run.push(item);
                                prev_key = Some(key);
                            }
                        }
                    }
                }
                if !run.is_empty() {
                    chunks.push(PerlValue::array_ref(Arc::new(RwLock::new(run))));
                }
                chunks
            }
            _ => {
                let mut chunks: Vec<PerlValue> = Vec::new();
                let mut run: Vec<PerlValue> = Vec::new();
                let mut prev_key: Option<PerlValue> = None;
                for item in list {
                    self.scope.set_topic(item.clone());
                    let key = self.eval_expr_ctx(key_spec, WantarrayCtx::Scalar)?;
                    match &prev_key {
                        None => {
                            run.push(item);
                            prev_key = Some(key);
                        }
                        Some(pk) => {
                            if key.str_eq(pk) {
                                run.push(item);
                            } else {
                                chunks.push(PerlValue::array_ref(Arc::new(RwLock::new(
                                    std::mem::take(&mut run),
                                ))));
                                run.push(item);
                                prev_key = Some(key);
                            }
                        }
                    }
                }
                if !run.is_empty() {
                    chunks.push(PerlValue::array_ref(Arc::new(RwLock::new(run))));
                }
                chunks
            }
        };
        Ok(match ctx {
            WantarrayCtx::List => PerlValue::array(chunks),
            WantarrayCtx::Scalar => PerlValue::integer(chunks.len() as i64),
            WantarrayCtx::Void => PerlValue::UNDEF,
        })
    }

    /// `take_while` / `drop_while` / `tap` / `peek` — block + list as [`ExprKind::FuncCall`].
    pub(crate) fn list_higher_order_block_builtin(
        &mut self,
        name: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match self.list_higher_order_block_builtin_exec(name, args, line) {
            Ok(v) => Ok(v),
            Err(FlowOrError::Error(e)) => Err(e),
            Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
            Err(FlowOrError::Flow(_)) => Err(PerlError::runtime(
                format!("{name}: unsupported control flow in block"),
                line,
            )),
        }
    }

    fn list_higher_order_block_builtin_exec(
        &mut self,
        name: &str,
        args: &[PerlValue],
        line: usize,
    ) -> ExecResult {
        if args.is_empty() {
            return Err(
                PerlError::runtime(format!("{name}: expected {{ BLOCK }}, LIST"), line).into(),
            );
        }
        let Some(sub) = args[0].as_code_ref() else {
            return Err(PerlError::runtime(
                format!("{name}: first argument must be {{ BLOCK }}"),
                line,
            )
            .into());
        };
        let sub = sub.clone();
        let items: Vec<PerlValue> = args[1..].to_vec();
        if matches!(name, "tap" | "peek") && items.len() == 1 {
            if let Some(p) = items[0].as_pipeline() {
                self.pipeline_push(&p, PipelineOp::Tap(sub), line)?;
                return Ok(PerlValue::pipeline(Arc::clone(&p)));
            }
            let v = &items[0];
            if v.is_iterator() || v.as_array_vec().is_some() {
                let source = crate::map_stream::into_pull_iter(v.clone());
                let (capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
                return Ok(PerlValue::iterator(Arc::new(
                    crate::map_stream::TapIterator::new(
                        source,
                        sub,
                        self.subs.clone(),
                        capture,
                        atomic_arrays,
                        atomic_hashes,
                    ),
                )));
            }
        }
        // Streaming optimization disabled for these functions because the pre-captured
        // coderef from args[0] has its closure_env populated at parse time, which causes
        // $_ to get stale values on subsequent calls. These functions work correctly in
        // the non-streaming eager path below.
        let wa = self.wantarray_kind;
        match name {
            "take_while" => {
                let mut out = Vec::new();
                for item in items {
                    self.scope_push_hook();
                    self.scope.set_topic(item.clone());
                    let pred = self.exec_block(&sub.body)?;
                    self.scope_pop_hook();
                    if !pred.is_true() {
                        break;
                    }
                    out.push(item);
                }
                Ok(match wa {
                    WantarrayCtx::List => PerlValue::array(out),
                    WantarrayCtx::Scalar => PerlValue::integer(out.len() as i64),
                    WantarrayCtx::Void => PerlValue::UNDEF,
                })
            }
            "drop_while" | "skip_while" => {
                let mut i = 0usize;
                while i < items.len() {
                    self.scope_push_hook();
                    self.scope.set_topic(items[i].clone());
                    let pred = self.exec_block(&sub.body)?;
                    self.scope_pop_hook();
                    if !pred.is_true() {
                        break;
                    }
                    i += 1;
                }
                let rest = items[i..].to_vec();
                Ok(match wa {
                    WantarrayCtx::List => PerlValue::array(rest),
                    WantarrayCtx::Scalar => PerlValue::integer(rest.len() as i64),
                    WantarrayCtx::Void => PerlValue::UNDEF,
                })
            }
            "reject" => {
                let mut out = Vec::new();
                for item in items {
                    self.scope_push_hook();
                    self.scope.set_topic(item.clone());
                    let pred = self.exec_block(&sub.body)?;
                    self.scope_pop_hook();
                    if !pred.is_true() {
                        out.push(item);
                    }
                }
                Ok(match wa {
                    WantarrayCtx::List => PerlValue::array(out),
                    WantarrayCtx::Scalar => PerlValue::integer(out.len() as i64),
                    WantarrayCtx::Void => PerlValue::UNDEF,
                })
            }
            "tap" | "peek" => {
                let _ = self.call_sub(&sub, items.clone(), WantarrayCtx::Void, line)?;
                Ok(match wa {
                    WantarrayCtx::List => PerlValue::array(items),
                    WantarrayCtx::Scalar => PerlValue::integer(items.len() as i64),
                    WantarrayCtx::Void => PerlValue::UNDEF,
                })
            }
            "partition" => {
                let mut yes = Vec::new();
                let mut no = Vec::new();
                for item in items {
                    self.scope.set_topic(item.clone());
                    let pred = self.call_sub(&sub, vec![], WantarrayCtx::Scalar, line)?;
                    if pred.is_true() {
                        yes.push(item);
                    } else {
                        no.push(item);
                    }
                }
                let yes_ref = PerlValue::array_ref(Arc::new(RwLock::new(yes)));
                let no_ref = PerlValue::array_ref(Arc::new(RwLock::new(no)));
                Ok(match wa {
                    WantarrayCtx::List => PerlValue::array(vec![yes_ref, no_ref]),
                    WantarrayCtx::Scalar => PerlValue::integer(2),
                    WantarrayCtx::Void => PerlValue::UNDEF,
                })
            }
            "min_by" => {
                let mut best: Option<(PerlValue, PerlValue)> = None;
                for item in items {
                    self.scope.set_topic(item.clone());
                    let key = self.call_sub(&sub, vec![], WantarrayCtx::Scalar, line)?;
                    best = Some(match best {
                        None => (item, key),
                        Some((bv, bk)) => {
                            if key.num_cmp(&bk) == std::cmp::Ordering::Less {
                                (item, key)
                            } else {
                                (bv, bk)
                            }
                        }
                    });
                }
                Ok(best.map(|(v, _)| v).unwrap_or(PerlValue::UNDEF))
            }
            "max_by" => {
                let mut best: Option<(PerlValue, PerlValue)> = None;
                for item in items {
                    self.scope.set_topic(item.clone());
                    let key = self.call_sub(&sub, vec![], WantarrayCtx::Scalar, line)?;
                    best = Some(match best {
                        None => (item, key),
                        Some((bv, bk)) => {
                            if key.num_cmp(&bk) == std::cmp::Ordering::Greater {
                                (item, key)
                            } else {
                                (bv, bk)
                            }
                        }
                    });
                }
                Ok(best.map(|(v, _)| v).unwrap_or(PerlValue::UNDEF))
            }
            "zip_with" => {
                // zip_with { BLOCK } \@a, \@b — apply block to paired elements
                // Flatten items, then treat each array ref/binding as a separate list.
                let flat: Vec<PerlValue> = items.into_iter().flat_map(|a| a.to_list()).collect();
                let refs: Vec<Vec<PerlValue>> = flat
                    .iter()
                    .map(|el| {
                        if let Some(ar) = el.as_array_ref() {
                            ar.read().clone()
                        } else if let Some(name) = el.as_array_binding_name() {
                            self.scope.get_array(&name)
                        } else {
                            vec![el.clone()]
                        }
                    })
                    .collect();
                let max_len = refs.iter().map(|l| l.len()).max().unwrap_or(0);
                let mut out = Vec::with_capacity(max_len);
                for i in 0..max_len {
                    let pair: Vec<PerlValue> = refs
                        .iter()
                        .map(|l| l.get(i).cloned().unwrap_or(PerlValue::UNDEF))
                        .collect();
                    let result = self.call_sub(&sub, pair, WantarrayCtx::Scalar, line)?;
                    out.push(result);
                }
                Ok(match wa {
                    WantarrayCtx::List => PerlValue::array(out),
                    WantarrayCtx::Scalar => PerlValue::integer(out.len() as i64),
                    WantarrayCtx::Void => PerlValue::UNDEF,
                })
            }
            "count_by" => {
                let mut counts = indexmap::IndexMap::new();
                for item in items {
                    self.scope.set_topic(item.clone());
                    let key = self.call_sub(&sub, vec![], WantarrayCtx::Scalar, line)?;
                    let k = key.to_string();
                    let entry = counts.entry(k).or_insert(PerlValue::integer(0));
                    *entry = PerlValue::integer(entry.to_int() + 1);
                }
                Ok(PerlValue::hash_ref(Arc::new(RwLock::new(counts))))
            }
            _ => Err(PerlError::runtime(
                format!("internal: unknown list block builtin `{name}`"),
                line,
            )
            .into()),
        }
    }

    /// `rmdir LIST` — remove empty directories; returns count removed.
    pub(crate) fn builtin_rmdir_execute(
        &mut self,
        args: &[PerlValue],
        _line: usize,
    ) -> PerlResult<PerlValue> {
        let mut count = 0i64;
        for a in args {
            let p = a.to_string();
            if p.is_empty() {
                continue;
            }
            if std::fs::remove_dir(&p).is_ok() {
                count += 1;
            }
        }
        Ok(PerlValue::integer(count))
    }

    /// `touch FILE, ...` — create if absent, update timestamps to now.
    pub(crate) fn builtin_touch_execute(
        &mut self,
        args: &[PerlValue],
        _line: usize,
    ) -> PerlResult<PerlValue> {
        let paths: Vec<String> = args.iter().map(|v| v.to_string()).collect();
        Ok(PerlValue::integer(crate::perl_fs::touch_paths(&paths)))
    }

    /// `utime ATIME, MTIME, LIST`
    pub(crate) fn builtin_utime_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        if args.len() < 3 {
            return Err(PerlError::runtime(
                "utime requires at least three arguments (atime, mtime, files...)",
                line,
            ));
        }
        let at = args[0].to_int();
        let mt = args[1].to_int();
        let paths: Vec<String> = args.iter().skip(2).map(|v| v.to_string()).collect();
        let n = crate::perl_fs::utime_paths(at, mt, &paths);
        #[cfg(not(unix))]
        if !paths.is_empty() && n == 0 {
            return Err(PerlError::runtime(
                "utime is not supported on this platform",
                line,
            ));
        }
        Ok(PerlValue::integer(n))
    }

    /// `umask EXPR` / `umask()` — returns previous mask when setting; current mask when called with no arguments.
    pub(crate) fn builtin_umask_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        #[cfg(unix)]
        {
            let _ = line;
            if args.is_empty() {
                let cur = unsafe { libc::umask(0) };
                unsafe { libc::umask(cur) };
                return Ok(PerlValue::integer(cur as i64));
            }
            let new_m = args[0].to_int() as libc::mode_t;
            let old = unsafe { libc::umask(new_m) };
            Ok(PerlValue::integer(old as i64))
        }
        #[cfg(not(unix))]
        {
            let _ = args;
            Err(PerlError::runtime(
                "umask is not supported on this platform",
                line,
            ))
        }
    }

    /// `getcwd` — current directory or undef on failure.
    pub(crate) fn builtin_getcwd_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        if !args.is_empty() {
            return Err(PerlError::runtime("getcwd takes no arguments", line));
        }
        match std::env::current_dir() {
            Ok(p) => Ok(PerlValue::string(p.to_string_lossy().into_owned())),
            Err(e) => {
                self.apply_io_error_to_errno(&e);
                Ok(PerlValue::UNDEF)
            }
        }
    }

    /// `realpath PATH` — [`std::fs::canonicalize`]; sets `$!` / errno on failure, returns undef.
    pub(crate) fn builtin_realpath_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        let path = args
            .first()
            .ok_or_else(|| PerlError::runtime("realpath: need path", line))?
            .to_string();
        if path.is_empty() {
            return Err(PerlError::runtime("realpath: need path", line));
        }
        match crate::perl_fs::realpath_resolved(&path) {
            Ok(s) => Ok(PerlValue::string(s)),
            Err(e) => {
                self.apply_io_error_to_errno(&e);
                Ok(PerlValue::UNDEF)
            }
        }
    }

    /// `pipe READHANDLE, WRITEHANDLE` — install OS pipe ends as buffered read / write handles (Unix).
    pub(crate) fn builtin_pipe_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        if args.len() != 2 {
            return Err(PerlError::runtime(
                "pipe requires exactly two arguments",
                line,
            ));
        }
        #[cfg(unix)]
        {
            use std::fs::File;
            use std::os::unix::io::FromRawFd;

            let read_name = args[0].to_string();
            let write_name = args[1].to_string();
            if read_name.is_empty() || write_name.is_empty() {
                return Err(PerlError::runtime("pipe: invalid handle name", line));
            }
            let mut fds = [0i32; 2];
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                let e = std::io::Error::last_os_error();
                self.apply_io_error_to_errno(&e);
                return Ok(PerlValue::integer(0));
            }
            let read_file = unsafe { File::from_raw_fd(fds[0]) };
            let write_file = unsafe { File::from_raw_fd(fds[1]) };

            let read_shared = Arc::new(Mutex::new(read_file));
            let write_shared = Arc::new(Mutex::new(write_file));

            self.close_builtin_execute(read_name.clone()).ok();
            self.close_builtin_execute(write_name.clone()).ok();

            self.io_file_slots
                .insert(read_name.clone(), Arc::clone(&read_shared));
            self.input_handles.insert(
                read_name,
                BufReader::new(Box::new(IoSharedFile(Arc::clone(&read_shared)))),
            );

            self.io_file_slots
                .insert(write_name.clone(), Arc::clone(&write_shared));
            self.output_handles
                .insert(write_name, Box::new(IoSharedFileWrite(write_shared)));

            Ok(PerlValue::integer(1))
        }
        #[cfg(not(unix))]
        {
            let _ = args;
            Err(PerlError::runtime(
                "pipe is not supported on this platform",
                line,
            ))
        }
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

    /// `eof` with no arguments: true while processing the last line from the current `-n`/`-p` input
    /// source (see [`Self::line_mode_eof_pending`]). Other contexts still return false until
    /// readline-level EOF tracking exists.
    pub(crate) fn eof_without_arg_is_true(&self) -> bool {
        self.line_mode_eof_pending
    }

    /// `eof` / `eof()` / `eof FH` — shared by the tree walker, [`crate::vm::VM`], and
    /// [`crate::builtins::try_builtin`] (`CORE::eof`, `builtin::eof`, which parse as [`ExprKind::FuncCall`],
    /// not [`ExprKind::Eof`]).
    pub(crate) fn eof_builtin_execute(
        &self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match args.len() {
            0 => Ok(PerlValue::integer(if self.eof_without_arg_is_true() {
                1
            } else {
                0
            })),
            1 => {
                let name = args[0].to_string();
                let at_eof = !self.has_input_handle(&name);
                Ok(PerlValue::integer(if at_eof { 1 } else { 0 }))
            }
            _ => Err(PerlError::runtime("eof: too many arguments", line)),
        }
    }

    /// `study EXPR` — Perl returns `1` for non-empty strings and a defined empty value (numifies to
    /// `0`, stringifies to `""`) for `""`.
    pub(crate) fn study_return_value(s: &str) -> PerlValue {
        if s.is_empty() {
            PerlValue::string(String::new())
        } else {
            PerlValue::integer(1)
        }
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
                    let read_result: Result<usize, io::Error> =
                        if let Some(reader) = self.diamond_reader.as_mut() {
                            if self.open_pragma_utf8 {
                                let mut buf = Vec::new();
                                reader.read_until(b'\n', &mut buf).inspect(|n| {
                                    if *n > 0 {
                                        line_str = String::from_utf8_lossy(&buf).into_owned();
                                    }
                                })
                            } else {
                                let mut buf = Vec::new();
                                match reader.read_until(b'\n', &mut buf) {
                                    Ok(n) => {
                                        if n > 0 {
                                            line_str =
                                            crate::perl_decode::decode_utf8_or_latin1_read_until(
                                                &buf,
                                            );
                                        }
                                        Ok(n)
                                    }
                                    Err(e) => Err(e),
                                }
                            }
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
            if let Some(queued) = self.line_mode_stdin_pending.pop_front() {
                self.last_stdin_die_bracket = if handle.is_none() {
                    "<>".to_string()
                } else {
                    "<STDIN>".to_string()
                };
                self.bump_line_for_handle("STDIN");
                return Ok(PerlValue::string(queued));
            }
            let r: Result<usize, io::Error> = if self.open_pragma_utf8 {
                let mut buf = Vec::new();
                io::stdin().lock().read_until(b'\n', &mut buf).inspect(|n| {
                    if *n > 0 {
                        line_str = String::from_utf8_lossy(&buf).into_owned();
                    }
                })
            } else {
                let mut buf = Vec::new();
                let mut lock = io::stdin().lock();
                match lock.read_until(b'\n', &mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            line_str = crate::perl_decode::decode_utf8_or_latin1_read_until(&buf);
                        }
                        Ok(n)
                    }
                    Err(e) => Err(e),
                }
            };
            match r {
                Ok(0) => Ok(PerlValue::UNDEF),
                Ok(_) => {
                    self.last_stdin_die_bracket = if handle.is_none() {
                        "<>".to_string()
                    } else {
                        "<STDIN>".to_string()
                    };
                    self.bump_line_for_handle("STDIN");
                    Ok(PerlValue::string(line_str))
                }
                Err(e) => {
                    self.apply_io_error_to_errno(&e);
                    Ok(PerlValue::UNDEF)
                }
            }
        } else if let Some(reader) = self.input_handles.get_mut(handle_name) {
            let r: Result<usize, io::Error> = if self.open_pragma_utf8 {
                let mut buf = Vec::new();
                reader.read_until(b'\n', &mut buf).inspect(|n| {
                    if *n > 0 {
                        line_str = String::from_utf8_lossy(&buf).into_owned();
                    }
                })
            } else {
                let mut buf = Vec::new();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            line_str = crate::perl_decode::decode_utf8_or_latin1_read_until(&buf);
                        }
                        Ok(n)
                    }
                    Err(e) => Err(e),
                }
            };
            match r {
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

    /// `<HANDLE>` / `readline` in **list** context: all lines until EOF (same as repeated scalar readline).
    pub(crate) fn readline_builtin_execute_list(
        &mut self,
        handle: Option<&str>,
    ) -> PerlResult<PerlValue> {
        let mut lines = Vec::new();
        loop {
            let v = self.readline_builtin_execute(handle)?;
            if v.is_undef() {
                break;
            }
            lines.push(v);
        }
        Ok(PerlValue::array(lines))
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

    /// List-context `readdir`: all directory entries not yet consumed (advances cursor to end).
    pub(crate) fn readdir_handle_list(&mut self, handle: &str) -> PerlValue {
        if let Some(dh) = self.dir_handles.get_mut(handle) {
            let rest: Vec<PerlValue> = dh.entries[dh.pos..]
                .iter()
                .cloned()
                .map(PerlValue::string)
                .collect();
            dh.pos = dh.entries.len();
            PerlValue::array(rest)
        } else {
            PerlValue::array(Vec::new())
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
    /// Scalar name names a regex capture variable (`$&`, `` $` ``, `$'`, `$+`, `$-`, `$1`..`$N`).
    /// Writing to any of these from non-regex code must invalidate [`Self::regex_capture_scope_fresh`]
    /// so the [`Self::regex_match_memo`] fast path re-applies `apply_regex_captures` on the next hit.
    #[inline]
    pub(crate) fn is_regex_capture_scope_var(name: &str) -> bool {
        crate::special_vars::is_regex_match_scalar_name(name)
    }

    /// Invalidate the capture-variable side of [`Self::regex_match_memo`]. Call from name-based
    /// scope writes (e.g. `Op::SetScalar`) so the next memoized regex match replays
    /// `apply_regex_captures` instead of short-circuiting.
    #[inline]
    pub(crate) fn maybe_invalidate_regex_capture_memo(&mut self, name: &str) {
        if self.regex_capture_scope_fresh && Self::is_regex_capture_scope_var(name) {
            self.regex_capture_scope_fresh = false;
        }
    }

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

    pub(crate) fn clear_flip_flop_state(&mut self) {
        self.flip_flop_active.clear();
        self.flip_flop_exclusive_left_line.clear();
        self.flip_flop_sequence.clear();
        self.flip_flop_last_dot.clear();
        self.flip_flop_tree.clear();
    }

    pub(crate) fn prepare_flip_flop_vm_slots(&mut self, slots: u16) {
        self.flip_flop_active.resize(slots as usize, false);
        self.flip_flop_active.fill(false);
        self.flip_flop_exclusive_left_line
            .resize(slots as usize, None);
        self.flip_flop_exclusive_left_line.fill(None);
        self.flip_flop_sequence.resize(slots as usize, 0);
        self.flip_flop_sequence.fill(0);
        self.flip_flop_last_dot.resize(slots as usize, None);
        self.flip_flop_last_dot.fill(None);
    }

    /// Input line number used by scalar `..` flip-flop — matches Perl `$.` (`-n`/`-p` use
    /// [`Self::line_number`]; [`Self::readline_builtin_execute`] updates `$.` via
    /// [`Self::handle_line_numbers`]).
    #[inline]
    pub(crate) fn scalar_flipflop_dot_line(&self) -> i64 {
        if self.last_readline_handle.is_empty() {
            self.line_number
        } else {
            *self
                .handle_line_numbers
                .get(&self.last_readline_handle)
                .unwrap_or(&0)
        }
    }

    /// Scalar `..` / `...` flip-flop vs `$.` (numeric bounds). `exclusive` matches Perl `...` (do not
    /// treat the right bound as satisfied on the same `$.` line as the left match; see `perlop`).
    ///
    /// Perl `pp_flop` stringifies the false state as `""` (not `0`) so `my $x = 1..5; print "[$x]"`
    /// prints `[]` when `$.` hasn't reached the left bound. True values are sequence numbers
    /// starting at `1`; the result on the closing line of an exclusive `...` has `E0` appended
    /// (represented here as the string `"<n>E0"`). Callers that need the numeric form still
    /// get `0` / `N` from [`PerlValue::to_int`].
    pub(crate) fn scalar_flip_flop_eval(
        &mut self,
        left: i64,
        right: i64,
        slot: usize,
        exclusive: bool,
    ) -> PerlResult<PerlValue> {
        if self.flip_flop_active.len() <= slot {
            self.flip_flop_active.resize(slot + 1, false);
        }
        if self.flip_flop_exclusive_left_line.len() <= slot {
            self.flip_flop_exclusive_left_line.resize(slot + 1, None);
        }
        if self.flip_flop_sequence.len() <= slot {
            self.flip_flop_sequence.resize(slot + 1, 0);
        }
        if self.flip_flop_last_dot.len() <= slot {
            self.flip_flop_last_dot.resize(slot + 1, None);
        }
        let dot = self.scalar_flipflop_dot_line();
        let active = &mut self.flip_flop_active[slot];
        let excl_left = &mut self.flip_flop_exclusive_left_line[slot];
        let seq = &mut self.flip_flop_sequence[slot];
        let last_dot = &mut self.flip_flop_last_dot[slot];
        if !*active {
            if dot == left {
                *active = true;
                *seq = 1;
                *last_dot = Some(dot);
                if exclusive {
                    *excl_left = Some(dot);
                } else {
                    *excl_left = None;
                    if dot == right {
                        *active = false;
                        return Ok(PerlValue::string(format!("{}E0", *seq)));
                    }
                }
                return Ok(PerlValue::string(seq.to_string()));
            }
            *last_dot = Some(dot);
            return Ok(PerlValue::string(String::new()));
        }
        // Already active: increment the sequence once per new `$.`, so a second evaluation on
        // the same line reads the same number (matches Perl `pp_flop`).
        if *last_dot != Some(dot) {
            *seq += 1;
            *last_dot = Some(dot);
        }
        let cur_seq = *seq;
        if let Some(ll) = *excl_left {
            if dot == right && dot > ll {
                *active = false;
                *excl_left = None;
                *seq = 0;
                return Ok(PerlValue::string(format!("{}E0", cur_seq)));
            }
        } else if dot == right {
            *active = false;
            *seq = 0;
            return Ok(PerlValue::string(format!("{}E0", cur_seq)));
        }
        Ok(PerlValue::string(cur_seq.to_string()))
    }

    fn regex_flip_flop_transition(
        active: &mut bool,
        excl_left: &mut Option<i64>,
        exclusive: bool,
        dot: i64,
        left_m: bool,
        right_m: bool,
    ) -> i64 {
        if !*active {
            if left_m {
                *active = true;
                if exclusive {
                    *excl_left = Some(dot);
                } else {
                    *excl_left = None;
                    if right_m {
                        *active = false;
                    }
                }
                return 1;
            }
            return 0;
        }
        if let Some(ll) = *excl_left {
            if right_m && dot > ll {
                *active = false;
                *excl_left = None;
            }
        } else if right_m {
            *active = false;
        }
        1
    }

    /// Scalar `..` / `...` when both operands are regex literals: match against `$_`; `$.`
    /// ([`Self::scalar_flipflop_dot_line`]) drives exclusive `...` (right not tested on the same line as
    /// left until `$.` advances), mirroring [`Self::scalar_flip_flop_eval`].
    #[allow(clippy::too_many_arguments)] // left/right pattern + flags + VM state is inherently eight params
    pub(crate) fn regex_flip_flop_eval(
        &mut self,
        left_pat: &str,
        left_flags: &str,
        right_pat: &str,
        right_flags: &str,
        slot: usize,
        exclusive: bool,
        line: usize,
    ) -> PerlResult<PerlValue> {
        let dot = self.scalar_flipflop_dot_line();
        let subject = self.scope.get_scalar("_").to_string();
        let left_re = self
            .compile_regex(left_pat, left_flags, line)
            .map_err(|e| match e {
                FlowOrError::Error(err) => err,
                FlowOrError::Flow(_) => {
                    PerlError::runtime("unexpected flow in regex flip-flop", line)
                }
            })?;
        let right_re = self
            .compile_regex(right_pat, right_flags, line)
            .map_err(|e| match e {
                FlowOrError::Error(err) => err,
                FlowOrError::Flow(_) => {
                    PerlError::runtime("unexpected flow in regex flip-flop", line)
                }
            })?;
        let left_m = left_re.is_match(&subject);
        let right_m = right_re.is_match(&subject);
        if self.flip_flop_active.len() <= slot {
            self.flip_flop_active.resize(slot + 1, false);
        }
        if self.flip_flop_exclusive_left_line.len() <= slot {
            self.flip_flop_exclusive_left_line.resize(slot + 1, None);
        }
        let active = &mut self.flip_flop_active[slot];
        let excl_left = &mut self.flip_flop_exclusive_left_line[slot];
        Ok(PerlValue::integer(Self::regex_flip_flop_transition(
            active, excl_left, exclusive, dot, left_m, right_m,
        )))
    }

    /// Regex `..` / `...` with a dynamic right operand (evaluated in boolean context vs `$_` / `eof` / etc.).
    pub(crate) fn regex_flip_flop_eval_dynamic_right(
        &mut self,
        left_pat: &str,
        left_flags: &str,
        slot: usize,
        exclusive: bool,
        line: usize,
        right_m: bool,
    ) -> PerlResult<PerlValue> {
        let dot = self.scalar_flipflop_dot_line();
        let subject = self.scope.get_scalar("_").to_string();
        let left_re = self
            .compile_regex(left_pat, left_flags, line)
            .map_err(|e| match e {
                FlowOrError::Error(err) => err,
                FlowOrError::Flow(_) => {
                    PerlError::runtime("unexpected flow in regex flip-flop", line)
                }
            })?;
        let left_m = left_re.is_match(&subject);
        if self.flip_flop_active.len() <= slot {
            self.flip_flop_active.resize(slot + 1, false);
        }
        if self.flip_flop_exclusive_left_line.len() <= slot {
            self.flip_flop_exclusive_left_line.resize(slot + 1, None);
        }
        let active = &mut self.flip_flop_active[slot];
        let excl_left = &mut self.flip_flop_exclusive_left_line[slot];
        Ok(PerlValue::integer(Self::regex_flip_flop_transition(
            active, excl_left, exclusive, dot, left_m, right_m,
        )))
    }

    /// Regex left bound vs `$_`; right bound is a fixed `$.` line (Perl `m/a/...N`).
    pub(crate) fn regex_flip_flop_eval_dot_line_rhs(
        &mut self,
        left_pat: &str,
        left_flags: &str,
        slot: usize,
        exclusive: bool,
        line: usize,
        rhs_line: i64,
    ) -> PerlResult<PerlValue> {
        let dot = self.scalar_flipflop_dot_line();
        let subject = self.scope.get_scalar("_").to_string();
        let left_re = self
            .compile_regex(left_pat, left_flags, line)
            .map_err(|e| match e {
                FlowOrError::Error(err) => err,
                FlowOrError::Flow(_) => {
                    PerlError::runtime("unexpected flow in regex flip-flop", line)
                }
            })?;
        let left_m = left_re.is_match(&subject);
        let right_m = dot == rhs_line;
        if self.flip_flop_active.len() <= slot {
            self.flip_flop_active.resize(slot + 1, false);
        }
        if self.flip_flop_exclusive_left_line.len() <= slot {
            self.flip_flop_exclusive_left_line.resize(slot + 1, None);
        }
        let active = &mut self.flip_flop_active[slot];
        let excl_left = &mut self.flip_flop_exclusive_left_line[slot];
        Ok(PerlValue::integer(Self::regex_flip_flop_transition(
            active, excl_left, exclusive, dot, left_m, right_m,
        )))
    }

    /// Regex `..` / `...` flip-flop when the right operand is bare `eof` (Perl: right side is `eof`, not a
    /// pattern). Uses [`Self::eof_without_arg_is_true`] like `eof` in `-n`/`-p`; exclusive `...` defers the
    /// right test until `$.` is strictly past the line where the left regex matched (same as
    /// [`Self::regex_flip_flop_eval`]).
    pub(crate) fn regex_eof_flip_flop_eval(
        &mut self,
        left_pat: &str,
        left_flags: &str,
        slot: usize,
        exclusive: bool,
        line: usize,
    ) -> PerlResult<PerlValue> {
        let dot = self.scalar_flipflop_dot_line();
        let subject = self.scope.get_scalar("_").to_string();
        let left_re = self
            .compile_regex(left_pat, left_flags, line)
            .map_err(|e| match e {
                FlowOrError::Error(err) => err,
                FlowOrError::Flow(_) => {
                    PerlError::runtime("unexpected flow in regex/eof flip-flop", line)
                }
            })?;
        let left_m = left_re.is_match(&subject);
        let right_m = self.eof_without_arg_is_true();
        if self.flip_flop_active.len() <= slot {
            self.flip_flop_active.resize(slot + 1, false);
        }
        if self.flip_flop_exclusive_left_line.len() <= slot {
            self.flip_flop_exclusive_left_line.resize(slot + 1, None);
        }
        let active = &mut self.flip_flop_active[slot];
        let excl_left = &mut self.flip_flop_exclusive_left_line[slot];
        Ok(PerlValue::integer(Self::regex_flip_flop_transition(
            active, excl_left, exclusive, dot, left_m, right_m,
        )))
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
        // Fast path: identical inputs to the previous non-`g` match → reuse the cached result.
        // Only safe for the non-`g`/non-`scalar_g` branch; `g` matches mutate `$&`/`@+`/etc. and
        // also keep per-pattern `pos()` state that the memo doesn't track.
        //
        // On hit AND `regex_capture_scope_fresh == true`, skip `apply_regex_captures` entirely:
        // the scope's `$&`/`$1`/... still reflect the memoized match. `regex_capture_scope_fresh`
        // is cleared by any scope write to a capture variable (see `invalidate_regex_capture_scope`).
        if !flags.contains('g') && !scalar_g {
            let memo_hit = {
                if let Some(ref mem) = self.regex_match_memo {
                    mem.pattern == pattern
                        && mem.flags == flags
                        && mem.multiline == self.multiline_match
                        && mem.haystack == s
                } else {
                    false
                }
            };
            if memo_hit {
                if self.regex_capture_scope_fresh {
                    return Ok(self.regex_match_memo.as_ref().expect("memo").result.clone());
                }
                // Memo hit but scope side effects were invalidated. Re-apply captures
                // from the memoized haystack + a fresh compiled regex.
                let (memo_s, memo_result) = {
                    let mem = self.regex_match_memo.as_ref().expect("memo");
                    (mem.haystack.clone(), mem.result.clone())
                };
                let re = self.compile_regex(pattern, flags, line)?;
                if let Some(caps) = re.captures(&memo_s) {
                    self.apply_regex_captures(&memo_s, 0, &re, &caps, CaptureAllMode::Empty)?;
                }
                self.regex_capture_scope_fresh = true;
                return Ok(memo_result);
            }
        }
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
            let result = PerlValue::integer(1);
            self.regex_match_memo = Some(RegexMatchMemo {
                pattern: pattern.to_string(),
                flags: flags.to_string(),
                multiline: self.multiline_match,
                haystack: s,
                result: result.clone(),
            });
            self.regex_capture_scope_fresh = true;
            Ok(result)
        } else {
            let result = PerlValue::integer(0);
            // Memoize negative results too — they don't set capture vars, so scope_fresh stays true.
            self.regex_match_memo = Some(RegexMatchMemo {
                pattern: pattern.to_string(),
                flags: flags.to_string(),
                multiline: self.multiline_match,
                haystack: s,
                result: result.clone(),
            });
            // A no-match leaves `$&` / `$1` as they were, which is still "fresh" from whatever
            // the last successful match (if any) set them to. Don't flip the flag.
            Ok(result)
        }
    }

    /// Expand `$ENV{KEY}` in an `s///` pattern or replacement string (Perl treats these like
    /// double-quoted interpolations; required for `s@$ENV{HOME}@~@` and for replacements like
    /// `"$ENV{HOME}$2"` before the regex engine sees the pattern).
    pub(crate) fn expand_env_braces_in_subst(
        &mut self,
        raw: &str,
        line: usize,
    ) -> PerlResult<String> {
        self.materialize_env_if_needed();
        let mut out = String::new();
        let mut rest = raw;
        while let Some(idx) = rest.find("$ENV{") {
            out.push_str(&rest[..idx]);
            let after = &rest[idx + 5..];
            let end = after
                .find('}')
                .ok_or_else(|| PerlError::runtime("Unclosed $ENV{...} in s///", line))?;
            let key = &after[..end];
            let val = self.scope.get_hash_element("ENV", key);
            out.push_str(&val.to_string());
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
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
        let re_flags: String = flags.chars().filter(|c| *c != 'e').collect();
        let pattern = self.expand_env_braces_in_subst(pattern, line)?;
        let re = self.compile_regex(&pattern, &re_flags, line)?;
        if flags.contains('e') {
            return self.regex_subst_execute_eval(s, re.as_ref(), replacement, flags, target, line);
        }
        let replacement = self.expand_env_braces_in_subst(replacement, line)?;
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
            (re.replace_all(&s, replacement.as_str()), count)
        } else {
            let count = if re.is_match(&s) { 1 } else { 0 };
            (re.replace(&s, replacement.as_str()), count)
        };
        if flags.contains('r') {
            // /r — non-destructive: return the modified string, leave target unchanged
            Ok(PerlValue::string(new_s))
        } else {
            self.assign_value(target, PerlValue::string(new_s))?;
            Ok(PerlValue::integer(count as i64))
        }
    }

    /// Run the `s///…e…` replacement side: `e_count` stacked `eval`s like Perl (each round parses
    /// and executes the string; the next round uses [`PerlValue::to_string`] of the prior value).
    fn regex_subst_run_eval_rounds(&mut self, replacement: &str, e_count: usize) -> ExecResult {
        let prep_source = |raw: &str| -> String {
            let mut code = raw.trim().to_string();
            if !code.ends_with(';') {
                code.push(';');
            }
            code
        };
        let mut cur = prep_source(replacement);
        let mut last = PerlValue::UNDEF;
        for round in 0..e_count {
            last = crate::parse_and_run_string(&cur, self)?;
            if round + 1 < e_count {
                cur = prep_source(&last.to_string());
            }
        }
        Ok(last)
    }

    fn regex_subst_execute_eval(
        &mut self,
        s: String,
        re: &PerlCompiledRegex,
        replacement: &str,
        flags: &str,
        target: &Expr,
        line: usize,
    ) -> ExecResult {
        let e_count = flags.chars().filter(|c| *c == 'e').count();
        if e_count == 0 {
            return Err(PerlError::runtime("s///e: internal error (no e flag)", line).into());
        }

        if flags.contains('g') {
            let mut rows = Vec::new();
            let mut out = String::new();
            let mut last = 0usize;
            let mut count = 0usize;
            for caps in re.captures_iter(&s) {
                let m0 = caps.get(0).expect("regex capture 0");
                out.push_str(&s[last..m0.start]);
                self.apply_regex_captures(&s, 0, re, &caps, CaptureAllMode::Empty)?;
                let repl_val = self.regex_subst_run_eval_rounds(replacement, e_count)?;
                out.push_str(&repl_val.to_string());
                last = m0.end;
                count += 1;
                rows.push(PerlValue::array(crate::perl_regex::numbered_capture_flat(
                    &caps,
                )));
            }
            self.scope.set_array("^CAPTURE_ALL", rows)?;
            out.push_str(&s[last..]);
            if flags.contains('r') {
                return Ok(PerlValue::string(out));
            }
            self.assign_value(target, PerlValue::string(out))?;
            return Ok(PerlValue::integer(count as i64));
        }
        if let Some(caps) = re.captures(&s) {
            let m0 = caps.get(0).expect("regex capture 0");
            self.apply_regex_captures(&s, 0, re, &caps, CaptureAllMode::Empty)?;
            let repl_val = self.regex_subst_run_eval_rounds(replacement, e_count)?;
            let mut out = String::new();
            out.push_str(&s[..m0.start]);
            out.push_str(&repl_val.to_string());
            out.push_str(&s[m0.end..]);
            if flags.contains('r') {
                return Ok(PerlValue::string(out));
            }
            self.assign_value(target, PerlValue::string(out))?;
            return Ok(PerlValue::integer(1));
        }
        if flags.contains('r') {
            return Ok(PerlValue::string(s));
        }
        self.assign_value(target, PerlValue::string(s))?;
        Ok(PerlValue::integer(0))
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
        let from_chars = Self::tr_expand_ranges(from);
        let to_chars = Self::tr_expand_ranges(to);
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
        if flags.contains('r') {
            // /r — non-destructive: return the modified string, leave target unchanged
            Ok(PerlValue::string(new_s))
        } else {
            if !flags.contains('d') {
                self.assign_value(target, PerlValue::string(new_s))?;
            }
            Ok(PerlValue::integer(count))
        }
    }

    /// Expand Perl `tr///` range notation: `a-z` → `a`, `b`, …, `z`.
    /// A literal `-` at the start or end of the spec is kept as-is.
    pub(crate) fn tr_expand_ranges(spec: &str) -> Vec<char> {
        let raw: Vec<char> = spec.chars().collect();
        let mut out = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            if i + 2 < raw.len() && raw[i + 1] == '-' && raw[i] <= raw[i + 2] {
                let start = raw[i] as u32;
                let end = raw[i + 2] as u32;
                for code in start..=end {
                    if let Some(c) = char::from_u32(code) {
                        out.push(c);
                    }
                }
                i += 3;
            } else {
                out.push(raw[i]);
                i += 1;
            }
        }
        out
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
        let arr_len = self.scope.array_len(&arr_name);
        let offset_val = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| PerlValue::integer(0));
        let length_val = match args.get(2) {
            None => PerlValue::UNDEF,
            Some(v) => v.clone(),
        };
        let (off, end) = splice_compute_range(arr_len, &offset_val, &length_val);
        let rep_vals: Vec<PerlValue> = args.iter().skip(3).cloned().collect();
        let arr = self.scope.get_array_mut(&arr_name)?;
        let removed: Vec<PerlValue> = arr.drain(off..end).collect();
        for (i, v) in rep_vals.into_iter().enumerate() {
            arr.insert(off + i, v);
        }
        Ok(match self.wantarray_kind {
            WantarrayCtx::Scalar => removed.last().cloned().unwrap_or(PerlValue::UNDEF),
            WantarrayCtx::List | WantarrayCtx::Void => PerlValue::array(removed),
        })
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
        let mut flat_vals: Vec<PerlValue> = Vec::new();
        for a in args.iter().skip(1) {
            if let Some(items) = a.as_array_vec() {
                flat_vals.extend(items);
            } else {
                flat_vals.push(a.clone());
            }
        }
        let arr = self.scope.get_array_mut(&arr_name)?;
        for (i, v) in flat_vals.into_iter().enumerate() {
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
        let warmup = (n / 10).clamp(1, 10);
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
        // `-n`/`-p`: main must run only inside [`Self::process_line`], not as a full-program VM/tree
        // run (would execute `print` once before any input, etc.).
        if self.line_mode_skip_main {
            return self.execute_tree(program);
        }
        // With `--profile`, the VM records per-opcode line times and sub enter/return (JIT off).
        // Try bytecode VM first — falls back to tree-walker on unsupported features
        if let Some(result) = crate::try_vm_execute(program, self) {
            return result;
        }

        // Tree-walker fallback
        self.execute_tree(program)
    }

    /// Run `END` blocks (after `-n`/`-p` line loop when prelude used [`Self::line_mode_skip_main`]).
    pub fn run_end_blocks(&mut self) -> PerlResult<()> {
        self.global_phase = "END".to_string();
        let ends = std::mem::take(&mut self.end_blocks);
        for block in &ends {
            self.exec_block(block).map_err(|e| match e {
                FlowOrError::Error(e) => e,
                FlowOrError::Flow(_) => PerlError::runtime("Unexpected flow control in END", 0),
            })?;
        }
        Ok(())
    }

    /// After a **top-level** program finishes (post-`END`), set `${^GLOBAL_PHASE}` to **`DESTRUCT`**
    /// and drain remaining `DESTROY` callbacks.
    pub fn run_global_teardown(&mut self) -> PerlResult<()> {
        self.global_phase = "DESTRUCT".to_string();
        self.drain_pending_destroys(0)
    }

    /// Run queued `DESTROY` methods from blessed objects whose last reference was dropped.
    pub(crate) fn drain_pending_destroys(&mut self, line: usize) -> PerlResult<()> {
        loop {
            let batch = crate::pending_destroy::take_queue();
            if batch.is_empty() {
                break;
            }
            for (class, payload) in batch {
                let fq = format!("{}::DESTROY", class);
                let Some(sub) = self.subs.get(&fq).cloned() else {
                    continue;
                };
                let inv = PerlValue::blessed(Arc::new(
                    crate::value::BlessedRef::new_for_destroy_invocant(class, payload),
                ));
                match self.call_sub(&sub, vec![inv], WantarrayCtx::Void, line) {
                    Ok(_) => {}
                    Err(FlowOrError::Error(e)) => return Err(e),
                    Err(FlowOrError::Flow(Flow::Return(_))) => {}
                    Err(FlowOrError::Flow(other)) => {
                        return Err(PerlError::runtime(
                            format!("DESTROY: unexpected control flow ({other:?})"),
                            line,
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Tree-walking execution (fallback when bytecode compilation fails).
    pub fn execute_tree(&mut self, program: &Program) -> PerlResult<PerlValue> {
        // `${^GLOBAL_PHASE}` — each program starts in `RUN` (Perl before any `BEGIN` runs).
        self.global_phase = "RUN".to_string();
        self.clear_flip_flop_state();
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
        // Perl keeps `${^GLOBAL_PHASE}` as **`START`** during these blocks (not `UNITCHECK`).
        let ucs = std::mem::take(&mut self.unit_check_blocks);
        for block in ucs.iter().rev() {
            self.exec_block(block).map_err(|e| match e {
                FlowOrError::Error(e) => e,
                FlowOrError::Flow(_) => {
                    PerlError::runtime("Unexpected flow control in UNITCHECK", 0)
                }
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

        if self.line_mode_skip_main {
            // Body runs once per input line in [`Self::process_line`]; `END` runs after the loop
            // via [`Self::run_end_blocks`].
            return Ok(PerlValue::UNDEF);
        }

        // Execute main program
        let mut last = PerlValue::UNDEF;
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::Begin(_)
                | StmtKind::UnitCheck(_)
                | StmtKind::Check(_)
                | StmtKind::Init(_)
                | StmtKind::End(_)
                | StmtKind::UsePerlVersion { .. }
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

        self.drain_pending_destroys(0)?;
        Ok(last)
    }

    pub(crate) fn exec_block(&mut self, block: &Block) -> ExecResult {
        self.exec_block_with_tail(block, WantarrayCtx::Void)
    }

    /// Run a block; the **last** statement is evaluated in `tail` wantarray (Perl `do { }` / `eval { }` value).
    /// Non-final statements stay void context.
    pub(crate) fn exec_block_with_tail(&mut self, block: &Block, tail: WantarrayCtx) -> ExecResult {
        let uses_goto = block
            .iter()
            .any(|s| matches!(s.kind, StmtKind::Goto { .. }));
        if uses_goto {
            self.scope_push_hook();
            let r = self.exec_block_with_goto_tail(block, tail);
            self.scope_pop_hook();
            r
        } else {
            self.scope_push_hook();
            let result = self.exec_block_no_scope_with_tail(block, tail);
            self.scope_pop_hook();
            result
        }
    }

    fn exec_block_with_goto_tail(&mut self, block: &Block, tail: WantarrayCtx) -> ExecResult {
        let mut map: HashMap<String, usize> = HashMap::new();
        for (i, s) in block.iter().enumerate() {
            if let Some(l) = &s.label {
                map.insert(l.clone(), i);
            }
        }
        let mut pc = 0usize;
        let mut last = PerlValue::UNDEF;
        let last_idx = block.len().saturating_sub(1);
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
            let v = if pc == last_idx {
                match &block[pc].kind {
                    StmtKind::Expression(expr) => self.eval_expr_ctx(expr, tail)?,
                    _ => self.exec_statement(&block[pc])?,
                }
            } else {
                self.exec_statement(&block[pc])?
            };
            last = v;
            pc += 1;
        }
        Ok(last)
    }

    /// Execute block statements without pushing/popping a scope frame.
    /// Used internally by loops and the VM for sub calls.
    #[inline]
    pub(crate) fn exec_block_no_scope(&mut self, block: &Block) -> ExecResult {
        self.exec_block_no_scope_with_tail(block, WantarrayCtx::Void)
    }

    pub(crate) fn exec_block_no_scope_with_tail(
        &mut self,
        block: &Block,
        tail: WantarrayCtx,
    ) -> ExecResult {
        if block.is_empty() {
            return Ok(PerlValue::UNDEF);
        }
        let last_i = block.len() - 1;
        for (i, stmt) in block.iter().enumerate() {
            if i < last_i {
                self.exec_statement(stmt)?;
            } else {
                return match &stmt.kind {
                    StmtKind::Expression(expr) => self.eval_expr_ctx(expr, tail),
                    _ => self.exec_statement(stmt),
                };
            }
        }
        Ok(PerlValue::UNDEF)
    }

    /// Spawn `block` on a worker thread; returns an [`PerlValue::AsyncTask`] handle (`async { }` / `spawn { }`).
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
                Err(FlowOrError::Flow(Flow::Yield(_))) => {
                    Err(PerlError::runtime("yield inside async/spawn block", 0))
                }
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
                Err(FlowOrError::Flow(Flow::Yield(_))) => {
                    Err(PerlError::runtime("yield inside eval_timeout block", 0))
                }
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

    /// `given` after the topic has been evaluated to a value (VM bytecode path or direct use).
    pub(crate) fn exec_given_with_topic_value(
        &mut self,
        topic: PerlValue,
        body: &Block,
    ) -> ExecResult {
        self.scope_push_hook();
        self.scope.declare_scalar("_", topic);
        self.english_note_lexical_scalar("_");
        let r = self.exec_given_body(body);
        self.scope_pop_hook();
        r
    }

    pub(crate) fn exec_given(&mut self, topic: &Expr, body: &Block) -> ExecResult {
        let t = self.eval_expr(topic)?;
        self.exec_given_with_topic_value(t, body)
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

    /// Boolean rvalue: bare `/.../` is `$_ =~ /.../` (Perl). Does not assign `$_`; sets `$1`… like `=~`.
    pub(crate) fn eval_boolean_rvalue_condition(
        &mut self,
        cond: &Expr,
    ) -> Result<bool, FlowOrError> {
        match &cond.kind {
            ExprKind::Regex(pattern, flags) => {
                let topic = self.scope.get_scalar("_");
                let line = cond.line;
                let s = topic.to_string();
                let v = self.regex_match_execute(s, pattern, flags, false, "_", line)?;
                Ok(v.is_true())
            }
            // `while (<STDIN>)` / `if (<>)` — Perl assigns the line to `$_` before testing (definedness).
            ExprKind::ReadLine(_) => {
                let v = self.eval_expr(cond)?;
                self.scope.set_topic(v.clone());
                Ok(!v.is_undef())
            }
            _ => {
                let v = self.eval_expr(cond)?;
                Ok(v.is_true())
            }
        }
    }

    /// Boolean condition for postfix `if` / `unless` / `while` / `until`.
    fn eval_postfix_condition(&mut self, cond: &Expr) -> Result<bool, FlowOrError> {
        self.eval_boolean_rvalue_condition(cond)
    }

    pub(crate) fn eval_algebraic_match(
        &mut self,
        subject: &Expr,
        arms: &[MatchArm],
        line: usize,
    ) -> ExecResult {
        let val = self.eval_algebraic_match_subject(subject, line)?;
        self.eval_algebraic_match_with_subject_value(val, arms, line)
    }

    /// Value used as `match` / `if let` subject: bare `@name` / `%name` bind like `\@name` / `\%name`.
    fn eval_algebraic_match_subject(&mut self, subject: &Expr, line: usize) -> ExecResult {
        match &subject.kind {
            ExprKind::ArrayVar(name) => {
                self.check_strict_array_var(name, line)?;
                let aname = self.stash_array_name_for_package(name);
                Ok(PerlValue::array_binding_ref(aname))
            }
            ExprKind::HashVar(name) => {
                self.check_strict_hash_var(name, line)?;
                self.touch_env_hash(name);
                Ok(PerlValue::hash_binding_ref(name.clone()))
            }
            _ => self.eval_expr(subject),
        }
    }

    /// Algebraic `match` after the subject has been evaluated (VM bytecode path).
    pub(crate) fn eval_algebraic_match_with_subject_value(
        &mut self,
        val: PerlValue,
        arms: &[MatchArm],
        line: usize,
    ) -> ExecResult {
        for arm in arms {
            if let MatchPattern::Regex { pattern, flags } = &arm.pattern {
                let re = self.compile_regex(pattern, flags, line)?;
                let s = val.to_string();
                if let Some(caps) = re.captures(&s) {
                    self.scope_push_hook();
                    self.scope.declare_scalar("_", val.clone());
                    self.english_note_lexical_scalar("_");
                    self.apply_regex_captures(&s, 0, re.as_ref(), &caps, CaptureAllMode::Empty)?;
                    let guard_ok = if let Some(g) = &arm.guard {
                        self.eval_expr(g)?.is_true()
                    } else {
                        true
                    };
                    if !guard_ok {
                        self.scope_pop_hook();
                        continue;
                    }
                    let out = self.eval_expr(&arm.body);
                    self.scope_pop_hook();
                    return out;
                }
                continue;
            }
            if let Some(bindings) = self.match_pattern_try(&val, &arm.pattern, line)? {
                self.scope_push_hook();
                self.scope.declare_scalar("_", val.clone());
                self.english_note_lexical_scalar("_");
                for b in bindings {
                    match b {
                        PatternBinding::Scalar(name, v) => {
                            self.scope.declare_scalar(&name, v);
                            self.english_note_lexical_scalar(&name);
                        }
                        PatternBinding::Array(name, elems) => {
                            self.scope.declare_array(&name, elems);
                        }
                    }
                }
                let guard_ok = if let Some(g) = &arm.guard {
                    self.eval_expr(g)?.is_true()
                } else {
                    true
                };
                if !guard_ok {
                    self.scope_pop_hook();
                    continue;
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

    fn parse_duration_seconds(pv: &PerlValue) -> Option<f64> {
        let s = pv.to_string();
        let s = s.trim();
        if let Some(rest) = s.strip_suffix("ms") {
            return rest.trim().parse::<f64>().ok().map(|x| x / 1000.0);
        }
        if let Some(rest) = s.strip_suffix('s') {
            return rest.trim().parse::<f64>().ok();
        }
        if let Some(rest) = s.strip_suffix('m') {
            return rest.trim().parse::<f64>().ok().map(|x| x * 60.0);
        }
        s.parse::<f64>().ok()
    }

    fn eval_retry_block(
        &mut self,
        body: &Block,
        times: &Expr,
        backoff: RetryBackoff,
        _line: usize,
    ) -> ExecResult {
        let max = self.eval_expr(times)?.to_int().max(1) as usize;
        let base_ms: u64 = 10;
        let mut attempt = 0usize;
        loop {
            attempt += 1;
            match self.exec_block(body) {
                Ok(v) => return Ok(v),
                Err(FlowOrError::Error(e)) => {
                    if attempt >= max {
                        return Err(FlowOrError::Error(e));
                    }
                    let delay_ms = match backoff {
                        RetryBackoff::None => 0,
                        RetryBackoff::Linear => base_ms.saturating_mul(attempt as u64),
                        RetryBackoff::Exponential => {
                            base_ms.saturating_mul(1u64 << (attempt as u32 - 1).min(30))
                        }
                    };
                    if delay_ms > 0 {
                        std::thread::sleep(Duration::from_millis(delay_ms));
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn eval_rate_limit_block(
        &mut self,
        slot: u32,
        max: &Expr,
        window: &Expr,
        body: &Block,
        _line: usize,
    ) -> ExecResult {
        let max_n = self.eval_expr(max)?.to_int().max(0) as usize;
        let window_sec = Self::parse_duration_seconds(&self.eval_expr(window)?)
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0);
        let window_d = Duration::from_secs_f64(window_sec);
        let slot = slot as usize;
        while self.rate_limit_slots.len() <= slot {
            self.rate_limit_slots.push(VecDeque::new());
        }
        {
            let dq = &mut self.rate_limit_slots[slot];
            loop {
                let now = Instant::now();
                while let Some(t0) = dq.front().copied() {
                    if now.duration_since(t0) >= window_d {
                        dq.pop_front();
                    } else {
                        break;
                    }
                }
                if dq.len() < max_n || max_n == 0 {
                    break;
                }
                let t0 = dq.front().copied().unwrap();
                let wait = window_d.saturating_sub(now.duration_since(t0));
                if wait.is_zero() {
                    dq.pop_front();
                    continue;
                }
                std::thread::sleep(wait);
            }
            dq.push_back(Instant::now());
        }
        self.exec_block(body)
    }

    fn eval_every_block(&mut self, interval: &Expr, body: &Block, _line: usize) -> ExecResult {
        let sec = Self::parse_duration_seconds(&self.eval_expr(interval)?)
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0);
        loop {
            match self.exec_block(body) {
                Ok(_) => {}
                Err(e) => return Err(e),
            }
            std::thread::sleep(Duration::from_secs_f64(sec));
        }
    }

    /// `->next` on a `gen { }` value: two-element **array ref** `(value, more)`; `more` is 0 when done.
    pub(crate) fn generator_next(&mut self, gen: &Arc<PerlGenerator>) -> PerlResult<PerlValue> {
        let pair = |value: PerlValue, more: i64| {
            PerlValue::array_ref(Arc::new(RwLock::new(vec![value, PerlValue::integer(more)])))
        };
        let mut exhausted = gen.exhausted.lock();
        if *exhausted {
            return Ok(pair(PerlValue::UNDEF, 0));
        }
        let mut pc = gen.pc.lock();
        let mut scope_started = gen.scope_started.lock();
        if *pc >= gen.block.len() {
            if *scope_started {
                self.scope_pop_hook();
                *scope_started = false;
            }
            *exhausted = true;
            return Ok(pair(PerlValue::UNDEF, 0));
        }
        if !*scope_started {
            self.scope_push_hook();
            *scope_started = true;
        }
        self.in_generator = true;
        while *pc < gen.block.len() {
            let stmt = &gen.block[*pc];
            match self.exec_statement(stmt) {
                Ok(_) => {
                    *pc += 1;
                }
                Err(FlowOrError::Flow(Flow::Yield(v))) => {
                    *pc += 1;
                    self.in_generator = false;
                    // Suspend: pop the generator frame before returning so outer `my $x = $g->next`
                    // binds in the caller block, not inside a frame left across yield.
                    if *scope_started {
                        self.scope_pop_hook();
                        *scope_started = false;
                    }
                    return Ok(pair(v, 1));
                }
                Err(e) => {
                    self.in_generator = false;
                    if *scope_started {
                        self.scope_pop_hook();
                        *scope_started = false;
                    }
                    return Err(match e {
                        FlowOrError::Error(ee) => ee,
                        FlowOrError::Flow(Flow::Yield(_)) => {
                            unreachable!("yield handled above")
                        }
                        FlowOrError::Flow(flow) => PerlError::runtime(
                            format!("unexpected control flow in generator: {:?}", flow),
                            0,
                        ),
                    });
                }
            }
        }
        self.in_generator = false;
        if *scope_started {
            self.scope_pop_hook();
            *scope_started = false;
        }
        *exhausted = true;
        Ok(pair(PerlValue::UNDEF, 0))
    }

    fn match_pattern_try(
        &mut self,
        subject: &PerlValue,
        pattern: &MatchPattern,
        line: usize,
    ) -> Result<Option<Vec<PatternBinding>>, FlowOrError> {
        match pattern {
            MatchPattern::Any => Ok(Some(vec![])),
            MatchPattern::Regex { .. } => {
                unreachable!("regex arms are handled in eval_algebraic_match")
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
                let Some(arr) = self.match_subject_as_array(subject) else {
                    return Ok(None);
                };
                self.match_array_pattern_elems(&arr, elems, line)
            }
            MatchPattern::Hash(pairs) => {
                let Some(h) = self.match_subject_as_hash(subject) else {
                    return Ok(None);
                };
                self.match_hash_pattern_pairs(&h, pairs, line)
            }
            MatchPattern::OptionSome(name) => {
                let Some(arr) = self.match_subject_as_array(subject) else {
                    return Ok(None);
                };
                if arr.len() < 2 {
                    return Ok(None);
                }
                if !arr[1].is_true() {
                    return Ok(None);
                }
                Ok(Some(vec![PatternBinding::Scalar(
                    name.clone(),
                    arr[0].clone(),
                )]))
            }
        }
    }

    /// Array value for algebraic `match`, including `\@name` array references (binding refs).
    fn match_subject_as_array(&self, v: &PerlValue) -> Option<Vec<PerlValue>> {
        if let Some(a) = v.as_array_vec() {
            return Some(a);
        }
        if let Some(r) = v.as_array_ref() {
            return Some(r.read().clone());
        }
        if let Some(name) = v.as_array_binding_name() {
            return Some(self.scope.get_array(&name));
        }
        None
    }

    fn match_subject_as_hash(&mut self, v: &PerlValue) -> Option<IndexMap<String, PerlValue>> {
        if let Some(h) = v.as_hash_map() {
            return Some(h);
        }
        if let Some(r) = v.as_hash_ref() {
            return Some(r.read().clone());
        }
        if let Some(name) = v.as_hash_binding_name() {
            self.touch_env_hash(&name);
            return Some(self.scope.get_hash(&name));
        }
        None
    }

    /// `@$href{k1,k2}` rvalue — `key_values` are already-evaluated key expressions (each may be an
    /// array to expand, like [`Self::eval_hash_slice_key_components`]). Shared by VM [`Op::HashSliceDeref`](crate::bytecode::Op::HashSliceDeref).
    pub(crate) fn hash_slice_deref_values(
        &mut self,
        container: &PerlValue,
        key_values: &[PerlValue],
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let h = if let Some(m) = self.match_subject_as_hash(container) {
            m
        } else {
            return Err(PerlError::runtime(
                "Hash slice dereference needs a hash or hash reference value",
                line,
            )
            .into());
        };
        let mut result = Vec::new();
        for kv in key_values {
            let key_strings: Vec<String> = if let Some(vv) = kv.as_array_vec() {
                vv.iter().map(|x| x.to_string()).collect()
            } else {
                vec![kv.to_string()]
            };
            for k in key_strings {
                result.push(h.get(&k).cloned().unwrap_or(PerlValue::UNDEF));
            }
        }
        Ok(PerlValue::array(result))
    }

    /// Single-key write for a hash slice container (hash ref or package hash name).
    /// Perl applies slice updates (`+=`, `++`, …) only to the **last** key for multi-key slices.
    pub(crate) fn assign_hash_slice_one_key(
        &mut self,
        container: PerlValue,
        key: &str,
        val: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if let Some(r) = container.as_hash_ref() {
            r.write().insert(key.to_string(), val);
            return Ok(PerlValue::UNDEF);
        }
        if let Some(name) = container.as_hash_binding_name() {
            self.touch_env_hash(&name);
            self.scope
                .set_hash_element(&name, key, val)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            return Ok(PerlValue::UNDEF);
        }
        if let Some(s) = container.as_str() {
            self.touch_env_hash(&s);
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
            self.scope
                .set_hash_element(&s, key, val)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            return Ok(PerlValue::UNDEF);
        }
        Err(PerlError::runtime(
            "Hash slice assignment needs a hash or hash reference value",
            line,
        )
        .into())
    }

    /// `%name{k1,k2} = LIST` — element-wise like [`Self::assign_hash_slice_deref`] on a stash hash.
    /// Shared by VM [`crate::bytecode::Op::SetHashSlice`].
    pub(crate) fn assign_named_hash_slice(
        &mut self,
        hash: &str,
        key_values: Vec<PerlValue>,
        val: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        self.touch_env_hash(hash);
        let mut ks: Vec<String> = Vec::new();
        for kv in key_values {
            if let Some(vv) = kv.as_array_vec() {
                ks.extend(vv.iter().map(|x| x.to_string()));
            } else {
                ks.push(kv.to_string());
            }
        }
        if ks.is_empty() {
            return Err(PerlError::runtime("assign to empty hash slice", line).into());
        }
        let items = val.to_list();
        for (i, k) in ks.iter().enumerate() {
            let v = items.get(i).cloned().unwrap_or(PerlValue::UNDEF);
            self.scope
                .set_hash_element(hash, k, v)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
        }
        Ok(PerlValue::UNDEF)
    }

    /// `@$href{k1,k2} = LIST` — shared by VM [`Op::SetHashSliceDeref`](crate::bytecode::Op::SetHashSliceDeref) and [`Self::assign_value`].
    pub(crate) fn assign_hash_slice_deref(
        &mut self,
        container: PerlValue,
        key_values: Vec<PerlValue>,
        val: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let mut ks: Vec<String> = Vec::new();
        for kv in key_values {
            if let Some(vv) = kv.as_array_vec() {
                ks.extend(vv.iter().map(|x| x.to_string()));
            } else {
                ks.push(kv.to_string());
            }
        }
        if ks.is_empty() {
            return Err(PerlError::runtime("assign to empty hash slice", line).into());
        }
        let items = val.to_list();
        if let Some(r) = container.as_hash_ref() {
            let mut h = r.write();
            for (i, k) in ks.iter().enumerate() {
                let v = items.get(i).cloned().unwrap_or(PerlValue::UNDEF);
                h.insert(k.clone(), v);
            }
            return Ok(PerlValue::UNDEF);
        }
        if let Some(name) = container.as_hash_binding_name() {
            self.touch_env_hash(&name);
            for (i, k) in ks.iter().enumerate() {
                let v = items.get(i).cloned().unwrap_or(PerlValue::UNDEF);
                self.scope
                    .set_hash_element(&name, k, v)
                    .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            }
            return Ok(PerlValue::UNDEF);
        }
        if let Some(s) = container.as_str() {
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
            for (i, k) in ks.iter().enumerate() {
                let v = items.get(i).cloned().unwrap_or(PerlValue::UNDEF);
                self.scope
                    .set_hash_element(&s, k, v)
                    .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            }
            return Ok(PerlValue::UNDEF);
        }
        Err(PerlError::runtime(
            "Hash slice assignment needs a hash or hash reference value",
            line,
        )
        .into())
    }

    /// `@$href{k1,k2} OP= rhs` — shared by VM [`Op::HashSliceDerefCompound`](crate::bytecode::Op::HashSliceDerefCompound).
    /// Perl 5 applies the compound op only to the **last** slice element.
    pub(crate) fn compound_assign_hash_slice_deref(
        &mut self,
        container: PerlValue,
        key_values: Vec<PerlValue>,
        op: BinOp,
        rhs: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let old_list = self.hash_slice_deref_values(&container, &key_values, line)?;
        let last_old = old_list
            .to_list()
            .last()
            .cloned()
            .unwrap_or(PerlValue::UNDEF);
        let new_val = self.eval_binop(op, &last_old, &rhs, line)?;
        let mut ks: Vec<String> = Vec::new();
        for kv in &key_values {
            if let Some(vv) = kv.as_array_vec() {
                ks.extend(vv.iter().map(|x| x.to_string()));
            } else {
                ks.push(kv.to_string());
            }
        }
        if ks.is_empty() {
            return Err(PerlError::runtime("assign to empty hash slice", line).into());
        }
        let last_key = ks.last().expect("non-empty ks");
        self.assign_hash_slice_one_key(container, last_key, new_val.clone(), line)?;
        Ok(new_val)
    }

    /// `++@$href{k1,k2}` / `--…` / `…++` / `…--` — shared by VM [`Op::HashSliceDerefIncDec`](crate::bytecode::Op::HashSliceDerefIncDec).
    /// Perl 5 updates only the **last** key; pre `++`/`--` return the new value, post forms return
    /// the **old** value of that last element.
    ///
    /// `kind` byte: 0 = PreInc, 1 = PreDec, 2 = PostInc, 3 = PostDec.
    pub(crate) fn hash_slice_deref_inc_dec(
        &mut self,
        container: PerlValue,
        key_values: Vec<PerlValue>,
        kind: u8,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let old_list = self.hash_slice_deref_values(&container, &key_values, line)?;
        let last_old = old_list
            .to_list()
            .last()
            .cloned()
            .unwrap_or(PerlValue::UNDEF);
        let new_val = if kind & 1 == 0 {
            PerlValue::integer(last_old.to_int() + 1)
        } else {
            PerlValue::integer(last_old.to_int() - 1)
        };
        let mut ks: Vec<String> = Vec::new();
        for kv in &key_values {
            if let Some(vv) = kv.as_array_vec() {
                ks.extend(vv.iter().map(|x| x.to_string()));
            } else {
                ks.push(kv.to_string());
            }
        }
        let last_key = ks.last().ok_or_else(|| {
            PerlError::runtime("Hash slice increment needs at least one key", line)
        })?;
        self.assign_hash_slice_one_key(container, last_key, new_val.clone(), line)?;
        Ok(if kind < 2 { new_val } else { last_old })
    }

    fn hash_slice_named_values(&mut self, hash: &str, key_values: &[PerlValue]) -> PerlValue {
        self.touch_env_hash(hash);
        let h = self.scope.get_hash(hash);
        let mut result = Vec::new();
        for kv in key_values {
            let key_strings: Vec<String> = if let Some(vv) = kv.as_array_vec() {
                vv.iter().map(|x| x.to_string()).collect()
            } else {
                vec![kv.to_string()]
            };
            for k in key_strings {
                result.push(h.get(&k).cloned().unwrap_or(PerlValue::UNDEF));
            }
        }
        PerlValue::array(result)
    }

    /// `@h{k1,k2} OP= rhs` on a stash hash — shared by VM [`crate::bytecode::Op::NamedHashSliceCompound`].
    pub(crate) fn compound_assign_named_hash_slice(
        &mut self,
        hash: &str,
        key_values: Vec<PerlValue>,
        op: BinOp,
        rhs: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let old_list = self.hash_slice_named_values(hash, &key_values);
        let last_old = old_list
            .to_list()
            .last()
            .cloned()
            .unwrap_or(PerlValue::UNDEF);
        let new_val = self.eval_binop(op, &last_old, &rhs, line)?;
        let mut ks: Vec<String> = Vec::new();
        for kv in &key_values {
            if let Some(vv) = kv.as_array_vec() {
                ks.extend(vv.iter().map(|x| x.to_string()));
            } else {
                ks.push(kv.to_string());
            }
        }
        if ks.is_empty() {
            return Err(PerlError::runtime("assign to empty hash slice", line).into());
        }
        let last_key = ks.last().expect("non-empty ks");
        let container = PerlValue::string(hash.to_string());
        self.assign_hash_slice_one_key(container, last_key, new_val.clone(), line)?;
        Ok(new_val)
    }

    /// `++@h{k1,k2}` / … on a stash hash — shared by VM [`crate::bytecode::Op::NamedHashSliceIncDec`].
    pub(crate) fn named_hash_slice_inc_dec(
        &mut self,
        hash: &str,
        key_values: Vec<PerlValue>,
        kind: u8,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let old_list = self.hash_slice_named_values(hash, &key_values);
        let last_old = old_list
            .to_list()
            .last()
            .cloned()
            .unwrap_or(PerlValue::UNDEF);
        let new_val = if kind & 1 == 0 {
            PerlValue::integer(last_old.to_int() + 1)
        } else {
            PerlValue::integer(last_old.to_int() - 1)
        };
        let mut ks: Vec<String> = Vec::new();
        for kv in &key_values {
            if let Some(vv) = kv.as_array_vec() {
                ks.extend(vv.iter().map(|x| x.to_string()));
            } else {
                ks.push(kv.to_string());
            }
        }
        let last_key = ks.last().ok_or_else(|| {
            PerlError::runtime("Hash slice increment needs at least one key", line)
        })?;
        let container = PerlValue::string(hash.to_string());
        self.assign_hash_slice_one_key(container, last_key, new_val.clone(), line)?;
        Ok(if kind < 2 { new_val } else { last_old })
    }

    fn match_array_pattern_elems(
        &mut self,
        arr: &[PerlValue],
        elems: &[MatchArrayElem],
        line: usize,
    ) -> Result<Option<Vec<PatternBinding>>, FlowOrError> {
        let has_rest = elems
            .iter()
            .any(|e| matches!(e, MatchArrayElem::Rest | MatchArrayElem::RestBind(_)));
        let mut binds: Vec<PatternBinding> = Vec::new();
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
                    return Ok(Some(binds));
                }
                MatchArrayElem::RestBind(name) => {
                    if i != elems.len() - 1 {
                        return Err(PerlError::runtime(
                            "internal: `@name` rest bind must be last in array match pattern",
                            line,
                        )
                        .into());
                    }
                    let tail = arr[idx..].to_vec();
                    binds.push(PatternBinding::Array(name.clone(), tail));
                    return Ok(Some(binds));
                }
                MatchArrayElem::CaptureScalar(name) => {
                    if idx >= arr.len() {
                        return Ok(None);
                    }
                    binds.push(PatternBinding::Scalar(name.clone(), arr[idx].clone()));
                    idx += 1;
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
        Ok(Some(binds))
    }

    fn match_hash_pattern_pairs(
        &mut self,
        h: &IndexMap<String, PerlValue>,
        pairs: &[MatchHashPair],
        _line: usize,
    ) -> Result<Option<Vec<PatternBinding>>, FlowOrError> {
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
                    binds.push(PatternBinding::Scalar(name.clone(), v.clone()));
                }
            }
        }
        Ok(Some(binds))
    }

    /// Check if a block declares variables (needs its own scope frame).
    #[inline]
    fn block_needs_scope(block: &Block) -> bool {
        block.iter().any(|s| match &s.kind {
            StmtKind::My(_)
            | StmtKind::Our(_)
            | StmtKind::Local(_)
            | StmtKind::State(_)
            | StmtKind::LocalExpr { .. } => true,
            StmtKind::StmtGroup(inner) => Self::block_needs_scope(inner),
            _ => false,
        })
    }

    /// Execute block, only pushing a scope frame if needed.
    #[inline]
    pub(crate) fn exec_block_smart(&mut self, block: &Block) -> ExecResult {
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
        if let Err(e) = self.drain_pending_destroys(stmt.line) {
            return Err(FlowOrError::Error(e));
        }
        match &stmt.kind {
            StmtKind::StmtGroup(block) => self.exec_block_no_scope(block),
            StmtKind::Expression(expr) => self.eval_expr_ctx(expr, WantarrayCtx::Void),
            StmtKind::If {
                condition,
                body,
                elsifs,
                else_block,
            } => {
                if self.eval_boolean_rvalue_condition(condition)? {
                    return self.exec_block(body);
                }
                for (c, b) in elsifs {
                    if self.eval_boolean_rvalue_condition(c)? {
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
                if !self.eval_boolean_rvalue_condition(condition)? {
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
                    if !self.eval_boolean_rvalue_condition(condition)? {
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
                    if self.eval_boolean_rvalue_condition(condition)? {
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
                    if !self.eval_boolean_rvalue_condition(condition)? {
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
                        if !self.eval_boolean_rvalue_condition(cond)? {
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
                let list_val = self.eval_expr_ctx(list, WantarrayCtx::List)?;
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
                                let skey = if is_our {
                                    self.stash_scalar_name_for_package(&decl.name)
                                } else {
                                    decl.name.clone()
                                };
                                self.scope.declare_scalar_frozen(
                                    &skey,
                                    v,
                                    decl.frozen,
                                    decl.type_annotation,
                                )?;
                                self.english_note_lexical_scalar(&decl.name);
                                if is_our {
                                    self.note_our_scalar(&decl.name);
                                }
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
                        // `our $Verbose ||= 0` / `my $x //= 1` — Perl declares the variable before
                        // evaluating `||=` / `//=` / `+=` … so strict sees a binding when the
                        // compound op reads the lhs (see system Exporter.pm).
                        let compound_init = decl
                            .initializer
                            .as_ref()
                            .is_some_and(|i| matches!(i.kind, ExprKind::CompoundAssign { .. }));

                        if compound_init {
                            match decl.sigil {
                                Sigil::Typeglob => {
                                    return Err(PerlError::runtime(
                                        "compound assignment on typeglob declaration is not supported",
                                        stmt.line,
                                    )
                                    .into());
                                }
                                Sigil::Scalar => {
                                    let skey = if is_our {
                                        self.stash_scalar_name_for_package(&decl.name)
                                    } else {
                                        decl.name.clone()
                                    };
                                    self.scope.declare_scalar_frozen(
                                        &skey,
                                        PerlValue::UNDEF,
                                        decl.frozen,
                                        decl.type_annotation,
                                    )?;
                                    self.english_note_lexical_scalar(&decl.name);
                                    if is_our {
                                        self.note_our_scalar(&decl.name);
                                    }
                                    let init = decl.initializer.as_ref().unwrap();
                                    self.eval_expr_ctx(init, WantarrayCtx::Void)?;
                                }
                                Sigil::Array => {
                                    let aname = self.stash_array_name_for_package(&decl.name);
                                    self.scope.declare_array_frozen(&aname, vec![], decl.frozen);
                                    let init = decl.initializer.as_ref().unwrap();
                                    self.eval_expr_ctx(init, WantarrayCtx::Void)?;
                                    if is_our {
                                        let items = self.scope.get_array(&aname);
                                        self.record_exporter_our_array_name(&decl.name, &items);
                                    }
                                }
                                Sigil::Hash => {
                                    self.scope.declare_hash_frozen(
                                        &decl.name,
                                        IndexMap::new(),
                                        decl.frozen,
                                    );
                                    let init = decl.initializer.as_ref().unwrap();
                                    self.eval_expr_ctx(init, WantarrayCtx::Void)?;
                                }
                            }
                            continue;
                        }

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
                                let skey = if is_our {
                                    self.stash_scalar_name_for_package(&decl.name)
                                } else {
                                    decl.name.clone()
                                };
                                self.scope.declare_scalar_frozen(
                                    &skey,
                                    val,
                                    decl.frozen,
                                    decl.type_annotation,
                                )?;
                                self.english_note_lexical_scalar(&decl.name);
                                if is_our {
                                    self.note_our_scalar(&decl.name);
                                }
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
            StmtKind::State(decls) => {
                // `state` variables persist across subroutine calls.
                // Key by source line + name for uniqueness.
                for decl in decls {
                    let state_key = format!("{}:{}", stmt.line, decl.name);
                    match decl.sigil {
                        Sigil::Scalar => {
                            if let Some(prev) = self.state_vars.get(&state_key).cloned() {
                                // Already initialized — declare with persisted value
                                self.scope.declare_scalar(&decl.name, prev);
                            } else {
                                // First encounter — evaluate initializer
                                let val = if let Some(init) = &decl.initializer {
                                    self.eval_expr(init)?
                                } else {
                                    PerlValue::UNDEF
                                };
                                self.state_vars.insert(state_key.clone(), val.clone());
                                self.scope.declare_scalar(&decl.name, val);
                            }
                            // Register for save-back when scope pops
                            if let Some(frame) = self.state_bindings_stack.last_mut() {
                                frame.push((decl.name.clone(), state_key));
                            }
                        }
                        _ => {
                            // For arrays/hashes, fall back to simple my-like behavior
                            let val = if let Some(init) = &decl.initializer {
                                self.eval_expr(init)?
                            } else {
                                PerlValue::UNDEF
                            };
                            match decl.sigil {
                                Sigil::Array => self.scope.declare_array(&decl.name, val.to_list()),
                                Sigil::Hash => {
                                    let items = val.to_list();
                                    let mut map = IndexMap::new();
                                    let mut i = 0;
                                    while i + 1 < items.len() {
                                        map.insert(items[i].to_string(), items[i + 1].clone());
                                        i += 2;
                                    }
                                    self.scope.declare_hash(&decl.name, map);
                                }
                                _ => {}
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
                    Ok(val)
                } else {
                    let mut last_val = PerlValue::UNDEF;
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
                        last_val = val.clone();
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
                                // `local $X = …` on a special var (`$/`, `$\`, `$,`, `$"`, …)
                                // must update the interpreter's backing field too — these are
                                // not stored in `Scope`. Save the prior value for restoration
                                // on `scope_pop_hook` so the block-exit restore is visible to
                                // print/I/O code.
                                if Self::is_special_scalar_name_for_set(&decl.name) {
                                    let old = self.get_special_var(&decl.name);
                                    if let Some(frame) = self.special_var_restore_frames.last_mut()
                                    {
                                        frame.push((decl.name.clone(), old));
                                    }
                                    self.set_special_var(&decl.name, &val)
                                        .map_err(|e| e.at_line(stmt.line))?;
                                }
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
                    Ok(last_val)
                }
            }
            StmtKind::LocalExpr {
                target,
                initializer,
            } => {
                let rhs_name = |init: &Expr| -> PerlResult<Option<String>> {
                    match &init.kind {
                        ExprKind::Typeglob(rhs) => Ok(Some(rhs.clone())),
                        _ => Err(PerlError::runtime(
                            "local *GLOB = *OTHER — right side must be a typeglob",
                            stmt.line,
                        )),
                    }
                };
                match &target.kind {
                    ExprKind::Typeglob(name) => {
                        let rhs = if let Some(init) = initializer {
                            rhs_name(init)?
                        } else {
                            None
                        };
                        self.local_declare_typeglob(name, rhs.as_deref(), stmt.line)?;
                        return Ok(PerlValue::UNDEF);
                    }
                    ExprKind::Deref {
                        expr,
                        kind: Sigil::Typeglob,
                    } => {
                        let lhs = self.eval_expr(expr)?.to_string();
                        let rhs = if let Some(init) = initializer {
                            rhs_name(init)?
                        } else {
                            None
                        };
                        self.local_declare_typeglob(lhs.as_str(), rhs.as_deref(), stmt.line)?;
                        return Ok(PerlValue::UNDEF);
                    }
                    ExprKind::TypeglobExpr(e) => {
                        let lhs = self.eval_expr(e)?.to_string();
                        let rhs = if let Some(init) = initializer {
                            rhs_name(init)?
                        } else {
                            None
                        };
                        self.local_declare_typeglob(lhs.as_str(), rhs.as_deref(), stmt.line)?;
                        return Ok(PerlValue::UNDEF);
                    }
                    _ => {}
                }
                let val = if let Some(init) = initializer {
                    let ctx = match &target.kind {
                        ExprKind::HashVar(_) | ExprKind::ArrayVar(_) => WantarrayCtx::List,
                        _ => WantarrayCtx::Scalar,
                    };
                    self.eval_expr_ctx(init, ctx)?
                } else {
                    PerlValue::UNDEF
                };
                match &target.kind {
                    ExprKind::ScalarVar(name) => {
                        // `local $X = …` on a special var — see twin block in
                        // `StmtKind::Local` (`Sigil::Scalar`) for rationale.
                        if Self::is_special_scalar_name_for_set(name) {
                            let old = self.get_special_var(name);
                            if let Some(frame) = self.special_var_restore_frames.last_mut() {
                                frame.push((name.clone(), old));
                            }
                            self.set_special_var(name, &val)
                                .map_err(|e| e.at_line(stmt.line))?;
                        }
                        self.scope.local_set_scalar(name, val.clone())?;
                    }
                    ExprKind::ArrayVar(name) => {
                        self.scope.local_set_array(name, val.to_list())?;
                    }
                    ExprKind::HashVar(name) => {
                        if name == "ENV" {
                            self.materialize_env_if_needed();
                        }
                        let items = val.to_list();
                        let mut map = IndexMap::new();
                        let mut i = 0;
                        while i + 1 < items.len() {
                            map.insert(items[i].to_string(), items[i + 1].clone());
                            i += 2;
                        }
                        self.scope.local_set_hash(name, map)?;
                    }
                    ExprKind::HashElement { hash, key } => {
                        let ks = self.eval_expr(key)?.to_string();
                        self.scope.local_set_hash_element(hash, &ks, val.clone())?;
                    }
                    ExprKind::ArrayElement { array, index } => {
                        self.check_strict_array_var(array, stmt.line)?;
                        let aname = self.stash_array_name_for_package(array);
                        let idx = self.eval_expr(index)?.to_int();
                        self.scope
                            .local_set_array_element(&aname, idx, val.clone())?;
                    }
                    _ => {
                        return Err(PerlError::runtime(
                            format!(
                                "local on this lvalue is not supported yet ({:?})",
                                target.kind
                            ),
                            stmt.line,
                        )
                        .into());
                    }
                }
                Ok(val)
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
            StmtKind::UsePerlVersion { .. } => Ok(PerlValue::UNDEF),
            StmtKind::Use { .. } => {
                // Handled in `prepare_program_top_level` before BEGIN / main.
                Ok(PerlValue::UNDEF)
            }
            StmtKind::UseOverload { pairs } => {
                self.install_use_overload_pairs(pairs);
                Ok(PerlValue::UNDEF)
            }
            StmtKind::No { .. } => {
                // Handled in `prepare_program_top_level` (same phase as `use`).
                Ok(PerlValue::UNDEF)
            }
            StmtKind::Return(val) => {
                let v = if let Some(e) = val {
                    // `return EXPR` evaluates EXPR in the caller's wantarray context so
                    // list-producing constructs like `1..$n`, `grep`, or `map` flatten rather
                    // than collapsing to a scalar flip-flop / count (`perlsyn` `return`).
                    self.eval_expr_ctx(e, self.wantarray_kind)?
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
            StmtKind::Goto { target } => {
                // goto &sub — tail call
                if let ExprKind::SubroutineRef(name) = &target.kind {
                    return Err(Flow::GotoSub(name.clone()).into());
                }
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
                let kind = match &target {
                    TieTarget::Scalar(_) => 0u8,
                    TieTarget::Array(_) => 1u8,
                    TieTarget::Hash(_) => 2u8,
                };
                let name = match &target {
                    TieTarget::Scalar(s) => s.as_str(),
                    TieTarget::Array(a) => a.as_str(),
                    TieTarget::Hash(h) => h.as_str(),
                };
                let mut vals = vec![self.eval_expr(class)?];
                for a in args {
                    vals.push(self.eval_expr(a)?);
                }
                self.tie_execute(kind, name, vals, stmt.line)
                    .map_err(Into::into)
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
    /// For `.=`, uses [`Scope::scalar_concat_inplace`] so the LHS is not cloned via
    /// [`Scope::get_scalar`] and `old.to_string()` on every iteration.
    pub(crate) fn scalar_compound_assign_scalar_target(
        &mut self,
        name: &str,
        op: BinOp,
        rhs: PerlValue,
    ) -> Result<PerlValue, PerlError> {
        if op == BinOp::Concat {
            return self.scope.scalar_concat_inplace(name, &rhs);
        }
        Ok(self
            .scope
            .atomic_mutate(name, |old| Self::compound_scalar_binop(old, op, &rhs)))
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
            BinOp::LogOr => {
                if old.is_true() {
                    old.clone()
                } else {
                    rhs.clone()
                }
            }
            BinOp::DefinedOr => {
                if !old.is_undef() {
                    old.clone()
                } else {
                    rhs.clone()
                }
            }
            BinOp::LogAnd => {
                if old.is_true() {
                    rhs.clone()
                } else {
                    old.clone()
                }
            }
            _ => PerlValue::float(old.to_number() + rhs.to_number()),
        }
    }

    /// One `{ ... }` entry in `@h{k1,k2}` may expand to several keys (`qw/a b/` → two keys,
    /// `'a'..'c'` → three keys). Hash-slice subscripts are evaluated in list context so that
    /// `..` expands via [`crate::value::perl_list_range_expand`] rather than flip-flopping.
    fn eval_hash_slice_key_components(
        &mut self,
        key_expr: &Expr,
    ) -> Result<Vec<String>, FlowOrError> {
        let v = if matches!(key_expr.kind, ExprKind::Range { .. }) {
            self.eval_expr_ctx(key_expr, WantarrayCtx::List)?
        } else {
            self.eval_expr(key_expr)?
        };
        if let Some(vv) = v.as_array_vec() {
            Ok(vv.iter().map(|x| x.to_string()).collect())
        } else {
            Ok(vec![v.to_string()])
        }
    }

    /// Symbolic ref deref (`$$r`, `@{...}`, `%{...}`, `*{...}`) — shared by [`Self::eval_expr_ctx`] and the VM.
    pub(crate) fn symbolic_deref(
        &mut self,
        val: PerlValue,
        kind: Sigil,
        line: usize,
    ) -> ExecResult {
        match kind {
            Sigil::Scalar => {
                if let Some(name) = val.as_scalar_binding_name() {
                    return Ok(self.get_special_var(&name));
                }
                if let Some(r) = val.as_scalar_ref() {
                    return Ok(r.read().clone());
                }
                // `${$cref}` / `$$href{k}` outer deref — array or hash ref (incl. binding refs).
                if let Some(r) = val.as_array_ref() {
                    return Ok(PerlValue::array(r.read().clone()));
                }
                if let Some(name) = val.as_array_binding_name() {
                    return Ok(PerlValue::array(self.scope.get_array(&name)));
                }
                if let Some(r) = val.as_hash_ref() {
                    return Ok(PerlValue::hash(r.read().clone()));
                }
                if let Some(name) = val.as_hash_binding_name() {
                    self.touch_env_hash(&name);
                    return Ok(PerlValue::hash(self.scope.get_hash(&name)));
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
                Err(PerlError::runtime("Can't dereference non-reference as scalar", line).into())
            }
            Sigil::Array => {
                if let Some(r) = val.as_array_ref() {
                    return Ok(PerlValue::array(r.read().clone()));
                }
                if let Some(name) = val.as_array_binding_name() {
                    return Ok(PerlValue::array(self.scope.get_array(&name)));
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
                Err(PerlError::runtime("Can't dereference non-reference as array", line).into())
            }
            Sigil::Hash => {
                if let Some(r) = val.as_hash_ref() {
                    return Ok(PerlValue::hash(r.read().clone()));
                }
                if let Some(name) = val.as_hash_binding_name() {
                    self.touch_env_hash(&name);
                    return Ok(PerlValue::hash(self.scope.get_hash(&name)));
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
                Err(PerlError::runtime("Can't dereference non-reference as hash", line).into())
            }
            Sigil::Typeglob => {
                if let Some(s) = val.as_str() {
                    return Ok(PerlValue::string(self.resolve_io_handle_name(&s)));
                }
                Err(PerlError::runtime("Can't dereference non-reference as typeglob", line).into())
            }
        }
    }

    /// `qq` list join expects a plain array; if a bare [`PerlValue::array_ref`] reaches join, peel
    /// one level so elements stringify like Perl (`"@$r"`).
    #[inline]
    pub(crate) fn peel_array_ref_for_list_join(&self, v: PerlValue) -> PerlValue {
        if let Some(r) = v.as_array_ref() {
            return PerlValue::array(r.read().clone());
        }
        v
    }

    /// `\@{EXPR}` / alias of an existing array ref — shared by [`crate::bytecode::Op::MakeArrayRefAlias`].
    pub(crate) fn make_array_ref_alias(&self, val: PerlValue, line: usize) -> ExecResult {
        if let Some(a) = val.as_array_ref() {
            return Ok(PerlValue::array_ref(Arc::clone(&a)));
        }
        if let Some(name) = val.as_array_binding_name() {
            return Ok(PerlValue::array_binding_ref(name));
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
            return Ok(PerlValue::array_binding_ref(s.to_string()));
        }
        if let Some(r) = val.as_scalar_ref() {
            let inner = r.read().clone();
            return self.make_array_ref_alias(inner, line);
        }
        Err(PerlError::runtime("Can't make array reference from value", line).into())
    }

    /// `\%{EXPR}` — shared by [`crate::bytecode::Op::MakeHashRefAlias`].
    pub(crate) fn make_hash_ref_alias(&self, val: PerlValue, line: usize) -> ExecResult {
        if let Some(h) = val.as_hash_ref() {
            return Ok(PerlValue::hash_ref(Arc::clone(&h)));
        }
        if let Some(name) = val.as_hash_binding_name() {
            return Ok(PerlValue::hash_binding_ref(name));
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
            return Ok(PerlValue::hash_binding_ref(s.to_string()));
        }
        if let Some(r) = val.as_scalar_ref() {
            let inner = r.read().clone();
            return self.make_hash_ref_alias(inner, line);
        }
        Err(PerlError::runtime("Can't make hash reference from value", line).into())
    }

    pub(crate) fn eval_expr_ctx(&mut self, expr: &Expr, ctx: WantarrayCtx) -> ExecResult {
        let line = expr.line;
        match &expr.kind {
            ExprKind::Integer(n) => Ok(PerlValue::integer(*n)),
            ExprKind::Float(f) => Ok(PerlValue::float(*f)),
            ExprKind::String(s) => Ok(PerlValue::string(s.clone())),
            ExprKind::Bareword(s) => {
                if s == "__PACKAGE__" {
                    return Ok(PerlValue::string(self.current_package()));
                }
                if let Some(sub) = self.resolve_sub_by_name(s) {
                    return self.call_sub(&sub, vec![], ctx, line);
                }
                Ok(PerlValue::string(s.clone()))
            }
            ExprKind::Undef => Ok(PerlValue::UNDEF),
            ExprKind::MagicConst(MagicConstKind::File) => Ok(PerlValue::string(self.file.clone())),
            ExprKind::MagicConst(MagicConstKind::Line) => Ok(PerlValue::integer(expr.line as i64)),
            ExprKind::MagicConst(MagicConstKind::Sub) => {
                if let Some(sub) = self.current_sub_stack.last().cloned() {
                    Ok(PerlValue::code_ref(sub))
                } else {
                    Ok(PerlValue::UNDEF)
                }
            }
            ExprKind::Regex(pattern, flags) => {
                if ctx == WantarrayCtx::Void {
                    // Expression statement: bare `/pat/;` is `$_ =~ /pat/` (Perl), not a regex object.
                    let topic = self.scope.get_scalar("_");
                    let s = topic.to_string();
                    self.regex_match_execute(s, pattern, flags, false, "_", line)
                } else {
                    let re = self.compile_regex(pattern, flags, line)?;
                    Ok(PerlValue::regex(re, pattern.clone(), flags.clone()))
                }
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
                            if let ExprKind::ArraySlice { array, .. } = &e.kind {
                                self.check_strict_array_var(array, line)?;
                                let val = self.eval_expr_ctx(e, WantarrayCtx::List)?;
                                let val = self.peel_array_ref_for_list_join(val);
                                let list = val.to_list();
                                let sep = self.list_separator.clone();
                                let mut parts = Vec::with_capacity(list.len());
                                for v in list {
                                    parts.push(self.stringify_value(v, line)?);
                                }
                                result.push_str(&parts.join(&sep));
                            } else if let ExprKind::Deref {
                                kind: Sigil::Array, ..
                            } = &e.kind
                            {
                                let val = self.eval_expr_ctx(e, WantarrayCtx::List)?;
                                let val = self.peel_array_ref_for_list_join(val);
                                let list = val.to_list();
                                let sep = self.list_separator.clone();
                                let mut parts = Vec::with_capacity(list.len());
                                for v in list {
                                    parts.push(self.stringify_value(v, line)?);
                                }
                                result.push_str(&parts.join(&sep));
                            } else {
                                let val = self.eval_expr(e)?;
                                let s = self.stringify_value(val, line)?;
                                result.push_str(&s);
                            }
                        }
                    }
                }
                Ok(PerlValue::string(result))
            }

            // Variables
            ExprKind::ScalarVar(name) => {
                self.check_strict_scalar_var(name, line)?;
                let stor = self.tree_scalar_storage_name(name);
                if let Some(obj) = self.tied_scalars.get(&stor).cloned() {
                    let class = obj
                        .as_blessed_ref()
                        .map(|b| b.class.clone())
                        .unwrap_or_default();
                    let full = format!("{}::FETCH", class);
                    if let Some(sub) = self.subs.get(&full).cloned() {
                        return self.call_sub(&sub, vec![obj], ctx, line);
                    }
                }
                Ok(self.get_special_var(&stor))
            }
            ExprKind::ArrayVar(name) => {
                self.check_strict_array_var(name, line)?;
                let aname = self.stash_array_name_for_package(name);
                let arr = self.scope.get_array(&aname);
                if ctx == WantarrayCtx::List {
                    Ok(PerlValue::array(arr))
                } else {
                    Ok(PerlValue::integer(arr.len() as i64))
                }
            }
            ExprKind::HashVar(name) => {
                self.check_strict_hash_var(name, line)?;
                self.touch_env_hash(name);
                let h = self.scope.get_hash(name);
                let pv = PerlValue::hash(h);
                if ctx == WantarrayCtx::List {
                    Ok(pv)
                } else {
                    Ok(pv.scalar_context())
                }
            }
            ExprKind::Typeglob(name) => {
                let n = self.resolve_io_handle_name(name);
                Ok(PerlValue::string(n))
            }
            ExprKind::TypeglobExpr(e) => {
                let name = self.eval_expr(e)?.to_string();
                let n = self.resolve_io_handle_name(&name);
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
                let aname = self.stash_array_name_for_package(array);
                let flat = self.flatten_array_slice_index_specs(indices)?;
                let mut result = Vec::with_capacity(flat.len());
                for idx in flat {
                    result.push(self.scope.get_array_element(&aname, idx));
                }
                Ok(PerlValue::array(result))
            }
            ExprKind::HashSlice { hash, keys } => {
                self.check_strict_hash_var(hash, line)?;
                self.touch_env_hash(hash);
                let mut result = Vec::new();
                for key_expr in keys {
                    for k in self.eval_hash_slice_key_components(key_expr)? {
                        result.push(self.scope.get_hash_element(hash, &k));
                    }
                }
                Ok(PerlValue::array(result))
            }
            ExprKind::HashSliceDeref { container, keys } => {
                let hv = self.eval_expr(container)?;
                let mut key_vals = Vec::with_capacity(keys.len());
                for key_expr in keys {
                    let v = if matches!(key_expr.kind, ExprKind::Range { .. }) {
                        self.eval_expr_ctx(key_expr, WantarrayCtx::List)?
                    } else {
                        self.eval_expr(key_expr)?
                    };
                    key_vals.push(v);
                }
                self.hash_slice_deref_values(&hv, &key_vals, line)
            }
            ExprKind::AnonymousListSlice { source, indices } => {
                let list_val = self.eval_expr_ctx(source, WantarrayCtx::List)?;
                let items = list_val.to_list();
                let flat = self.flatten_array_slice_index_specs(indices)?;
                let mut out = Vec::with_capacity(flat.len());
                for idx in flat {
                    let i = if idx < 0 {
                        (items.len() as i64 + idx) as usize
                    } else {
                        idx as usize
                    };
                    out.push(items.get(i).cloned().unwrap_or(PerlValue::UNDEF));
                }
                let arr = PerlValue::array(out);
                if ctx != WantarrayCtx::List {
                    let v = arr.to_list();
                    Ok(v.last().cloned().unwrap_or(PerlValue::UNDEF))
                } else {
                    Ok(arr)
                }
            }

            // References
            ExprKind::ScalarRef(inner) => match &inner.kind {
                ExprKind::ScalarVar(name) => Ok(PerlValue::scalar_binding_ref(name.clone())),
                ExprKind::ArrayVar(name) => {
                    self.check_strict_array_var(name, line)?;
                    let aname = self.stash_array_name_for_package(name);
                    Ok(PerlValue::array_binding_ref(aname))
                }
                ExprKind::HashVar(name) => {
                    self.check_strict_hash_var(name, line)?;
                    Ok(PerlValue::hash_binding_ref(name.clone()))
                }
                ExprKind::Deref {
                    expr: e,
                    kind: Sigil::Array,
                } => {
                    let v = self.eval_expr(e)?;
                    self.make_array_ref_alias(v, line)
                }
                ExprKind::Deref {
                    expr: e,
                    kind: Sigil::Hash,
                } => {
                    let v = self.eval_expr(e)?;
                    self.make_hash_ref_alias(v, line)
                }
                ExprKind::ArraySlice { .. } | ExprKind::HashSlice { .. } => {
                    let list = self.eval_expr_ctx(inner, WantarrayCtx::List)?;
                    Ok(PerlValue::array_ref(Arc::new(RwLock::new(list.to_list()))))
                }
                ExprKind::HashSliceDeref { .. } => {
                    let list = self.eval_expr_ctx(inner, WantarrayCtx::List)?;
                    Ok(PerlValue::array_ref(Arc::new(RwLock::new(list.to_list()))))
                }
                _ => {
                    let val = self.eval_expr(inner)?;
                    Ok(PerlValue::scalar_ref(Arc::new(RwLock::new(val))))
                }
            },
            ExprKind::ArrayRef(elems) => {
                // `[ LIST ]` is list context so `1..5`, `reverse`, `grep`, `map`, and array
                // variables flatten into the ref rather than collapsing to a scalar count /
                // flip-flop value.
                let mut arr = Vec::with_capacity(elems.len());
                for e in elems {
                    let v = self.eval_expr_ctx(e, WantarrayCtx::List)?;
                    if let Some(vec) = v.as_array_vec() {
                        arr.extend(vec);
                    } else {
                        arr.push(v);
                    }
                }
                Ok(PerlValue::array_ref(Arc::new(RwLock::new(arr))))
            }
            ExprKind::HashRef(pairs) => {
                // `{ KEY => VAL, ... }` — keys are scalar-context, but values are list-context
                // so `{ a => [1..3] }` and `{ key => grep/sort/... }` flatten through.
                let mut map = IndexMap::new();
                for (k, v) in pairs {
                    let key = self.eval_expr(k)?.to_string();
                    let val = self.eval_expr_ctx(v, WantarrayCtx::List)?;
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
                    PerlError::runtime(self.undefined_subroutine_resolve_message(name), line)
                })?;
                Ok(PerlValue::code_ref(sub))
            }
            ExprKind::DynamicSubCodeRef(expr) => {
                let name = self.eval_expr(expr)?.to_string();
                let sub = self.resolve_sub_by_name(&name).ok_or_else(|| {
                    PerlError::runtime(self.undefined_subroutine_resolve_message(&name), line)
                })?;
                Ok(PerlValue::code_ref(sub))
            }
            ExprKind::Deref { expr, kind } => {
                if ctx != WantarrayCtx::List && matches!(kind, Sigil::Array) {
                    let val = self.eval_expr(expr)?;
                    let n = self.array_deref_len(val, line)?;
                    return Ok(PerlValue::integer(n));
                }
                if ctx != WantarrayCtx::List && matches!(kind, Sigil::Hash) {
                    let val = self.eval_expr(expr)?;
                    let h = self.symbolic_deref(val, Sigil::Hash, line)?;
                    return Ok(h.scalar_context());
                }
                let val = self.eval_expr(expr)?;
                self.symbolic_deref(val, *kind, line)
            }
            ExprKind::ArrowDeref { expr, index, kind } => {
                match kind {
                    DerefKind::Array => {
                        let container = self.eval_arrow_array_base(expr, line)?;
                        if let ExprKind::List(indices) = &index.kind {
                            let mut out = Vec::with_capacity(indices.len());
                            for ix in indices {
                                let idx = self.eval_expr(ix)?.to_int();
                                out.push(self.read_arrow_array_element(
                                    container.clone(),
                                    idx,
                                    line,
                                )?);
                            }
                            let arr = PerlValue::array(out);
                            if ctx != WantarrayCtx::List {
                                let v = arr.to_list();
                                return Ok(v.last().cloned().unwrap_or(PerlValue::UNDEF));
                            }
                            return Ok(arr);
                        }
                        let idx = self.eval_expr(index)?.to_int();
                        self.read_arrow_array_element(container, idx, line)
                    }
                    DerefKind::Hash => {
                        let val = self.eval_arrow_hash_base(expr, line)?;
                        let key = self.eval_expr(index)?.to_string();
                        self.read_arrow_hash_element(val, key.as_str(), line)
                    }
                    DerefKind::Call => {
                        // $coderef->(args)
                        let val = self.eval_expr(expr)?;
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
                // Short-circuit ops: bare `/.../` in boolean context is `$_ =~`, not a regex object.
                match op {
                    BinOp::BindMatch => {
                        let lv = self.eval_expr(left)?;
                        let rv = self.eval_expr(right)?;
                        let s = lv.to_string();
                        let pat = rv.to_string();
                        return self.regex_match_execute(s, &pat, "", false, "_", line);
                    }
                    BinOp::BindNotMatch => {
                        let lv = self.eval_expr(left)?;
                        let rv = self.eval_expr(right)?;
                        let s = lv.to_string();
                        let pat = rv.to_string();
                        let m = self.regex_match_execute(s, &pat, "", false, "_", line)?;
                        return Ok(PerlValue::integer(if m.is_true() { 0 } else { 1 }));
                    }
                    BinOp::LogAnd | BinOp::LogAndWord => {
                        match &left.kind {
                            ExprKind::Regex(_, _) => {
                                if !self.eval_boolean_rvalue_condition(left)? {
                                    return Ok(PerlValue::string(String::new()));
                                }
                            }
                            _ => {
                                let lv = self.eval_expr(left)?;
                                if !lv.is_true() {
                                    return Ok(lv);
                                }
                            }
                        }
                        return match &right.kind {
                            ExprKind::Regex(_, _) => Ok(PerlValue::integer(
                                if self.eval_boolean_rvalue_condition(right)? {
                                    1
                                } else {
                                    0
                                },
                            )),
                            _ => self.eval_expr(right),
                        };
                    }
                    BinOp::LogOr | BinOp::LogOrWord => {
                        match &left.kind {
                            ExprKind::Regex(_, _) => {
                                if self.eval_boolean_rvalue_condition(left)? {
                                    return Ok(PerlValue::integer(1));
                                }
                            }
                            _ => {
                                let lv = self.eval_expr(left)?;
                                if lv.is_true() {
                                    return Ok(lv);
                                }
                            }
                        }
                        return match &right.kind {
                            ExprKind::Regex(_, _) => Ok(PerlValue::integer(
                                if self.eval_boolean_rvalue_condition(right)? {
                                    1
                                } else {
                                    0
                                },
                            )),
                            _ => self.eval_expr(right),
                        };
                    }
                    BinOp::DefinedOr => {
                        let lv = self.eval_expr(left)?;
                        if !lv.is_undef() {
                            return Ok(lv);
                        }
                        return self.eval_expr(right);
                    }
                    _ => {}
                }
                let lv = self.eval_expr(left)?;
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
                    if let ExprKind::Deref { kind, .. } = &expr.kind {
                        if matches!(kind, Sigil::Array | Sigil::Hash) {
                            return Err(Self::err_modify_symbolic_aggregate_deref_inc_dec(
                                *kind, true, true, line,
                            ));
                        }
                    }
                    if let ExprKind::HashSliceDeref { container, keys } = &expr.kind {
                        let href = self.eval_expr(container)?;
                        let mut key_vals = Vec::with_capacity(keys.len());
                        for key_expr in keys {
                            key_vals.push(self.eval_expr(key_expr)?);
                        }
                        return self.hash_slice_deref_inc_dec(href, key_vals, 0, line);
                    }
                    if let ExprKind::ArrowDeref {
                        expr: arr_expr,
                        index,
                        kind: DerefKind::Array,
                    } = &expr.kind
                    {
                        if let ExprKind::List(indices) = &index.kind {
                            let container = self.eval_arrow_array_base(arr_expr, line)?;
                            let mut idxs = Vec::with_capacity(indices.len());
                            for ix in indices {
                                idxs.push(self.eval_expr(ix)?.to_int());
                            }
                            return self.arrow_array_slice_inc_dec(container, idxs, 0, line);
                        }
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
                    if let ExprKind::Deref { kind, .. } = &expr.kind {
                        if matches!(kind, Sigil::Array | Sigil::Hash) {
                            return Err(Self::err_modify_symbolic_aggregate_deref_inc_dec(
                                *kind, true, false, line,
                            ));
                        }
                    }
                    if let ExprKind::HashSliceDeref { container, keys } = &expr.kind {
                        let href = self.eval_expr(container)?;
                        let mut key_vals = Vec::with_capacity(keys.len());
                        for key_expr in keys {
                            key_vals.push(self.eval_expr(key_expr)?);
                        }
                        return self.hash_slice_deref_inc_dec(href, key_vals, 1, line);
                    }
                    if let ExprKind::ArrowDeref {
                        expr: arr_expr,
                        index,
                        kind: DerefKind::Array,
                    } = &expr.kind
                    {
                        if let ExprKind::List(indices) = &index.kind {
                            let container = self.eval_arrow_array_base(arr_expr, line)?;
                            let mut idxs = Vec::with_capacity(indices.len());
                            for ix in indices {
                                idxs.push(self.eval_expr(ix)?.to_int());
                            }
                            return self.arrow_array_slice_inc_dec(container, idxs, 1, line);
                        }
                    }
                    let val = self.eval_expr(expr)?;
                    let new_val = PerlValue::integer(val.to_int() - 1);
                    self.assign_value(expr, new_val.clone())?;
                    Ok(new_val)
                }
                _ => {
                    match op {
                        UnaryOp::LogNot | UnaryOp::LogNotWord => {
                            if let ExprKind::Regex(pattern, flags) = &expr.kind {
                                let topic = self.scope.get_scalar("_");
                                let rl = expr.line;
                                let s = topic.to_string();
                                let v =
                                    self.regex_match_execute(s, pattern, flags, false, "_", rl)?;
                                return Ok(PerlValue::integer(if v.is_true() { 0 } else { 1 }));
                            }
                        }
                        _ => {}
                    }
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
                        UnaryOp::Ref => {
                            if let ExprKind::ScalarVar(name) = &expr.kind {
                                return Ok(PerlValue::scalar_binding_ref(name.clone()));
                            }
                            Ok(PerlValue::scalar_ref(Arc::new(RwLock::new(val))))
                        }
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
                if let ExprKind::Deref { kind, .. } = &expr.kind {
                    if matches!(kind, Sigil::Array | Sigil::Hash) {
                        let is_inc = matches!(op, PostfixOp::Increment);
                        return Err(Self::err_modify_symbolic_aggregate_deref_inc_dec(
                            *kind, false, is_inc, line,
                        ));
                    }
                }
                if let ExprKind::HashSliceDeref { container, keys } = &expr.kind {
                    let href = self.eval_expr(container)?;
                    let mut key_vals = Vec::with_capacity(keys.len());
                    for key_expr in keys {
                        key_vals.push(self.eval_expr(key_expr)?);
                    }
                    let kind_byte = match op {
                        PostfixOp::Increment => 2u8,
                        PostfixOp::Decrement => 3u8,
                    };
                    return self.hash_slice_deref_inc_dec(href, key_vals, kind_byte, line);
                }
                if let ExprKind::ArrowDeref {
                    expr: arr_expr,
                    index,
                    kind: DerefKind::Array,
                } = &expr.kind
                {
                    if let ExprKind::List(indices) = &index.kind {
                        let container = self.eval_arrow_array_base(arr_expr, line)?;
                        let mut idxs = Vec::with_capacity(indices.len());
                        for ix in indices {
                            idxs.push(self.eval_expr(ix)?.to_int());
                        }
                        let kind_byte = match op {
                            PostfixOp::Increment => 2u8,
                            PostfixOp::Decrement => 3u8,
                        };
                        return self.arrow_array_slice_inc_dec(container, idxs, kind_byte, line);
                    }
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
                let val = self.eval_expr_ctx(value, assign_rhs_wantarray(target))?;
                self.assign_value(target, val.clone())?;
                Ok(val)
            }
            ExprKind::CompoundAssign { target, op, value } => {
                // For scalar targets, use atomic_mutate to hold the lock.
                // `||=` / `//=` short-circuit: do not evaluate RHS if LHS is already true / defined.
                if let ExprKind::ScalarVar(name) = &target.kind {
                    self.check_strict_scalar_var(name, line)?;
                    let n = self.english_scalar_name(name);
                    let op = *op;
                    let rhs = match op {
                        BinOp::LogOr => {
                            let old = self.scope.get_scalar(n);
                            if old.is_true() {
                                return Ok(old);
                            }
                            self.eval_expr(value)?
                        }
                        BinOp::DefinedOr => {
                            let old = self.scope.get_scalar(n);
                            if !old.is_undef() {
                                return Ok(old);
                            }
                            self.eval_expr(value)?
                        }
                        BinOp::LogAnd => {
                            let old = self.scope.get_scalar(n);
                            if !old.is_true() {
                                return Ok(old);
                            }
                            self.eval_expr(value)?
                        }
                        _ => self.eval_expr(value)?,
                    };
                    return Ok(self.scalar_compound_assign_scalar_target(n, op, rhs)?);
                }
                let rhs = self.eval_expr(value)?;
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
                if let ExprKind::HashSliceDeref { container, keys } = &target.kind {
                    let href = self.eval_expr(container)?;
                    let mut key_vals = Vec::with_capacity(keys.len());
                    for key_expr in keys {
                        key_vals.push(self.eval_expr(key_expr)?);
                    }
                    return self.compound_assign_hash_slice_deref(href, key_vals, *op, rhs, line);
                }
                if let ExprKind::AnonymousListSlice { source, indices } = &target.kind {
                    if let ExprKind::Deref {
                        expr: inner,
                        kind: Sigil::Array,
                    } = &source.kind
                    {
                        let container = self.eval_arrow_array_base(inner, line)?;
                        let idxs = self.flatten_array_slice_index_specs(indices)?;
                        return self
                            .compound_assign_arrow_array_slice(container, idxs, *op, rhs, line);
                    }
                }
                if let ExprKind::ArrowDeref {
                    expr: arr_expr,
                    index,
                    kind: DerefKind::Array,
                } = &target.kind
                {
                    if let ExprKind::List(indices) = &index.kind {
                        let container = self.eval_arrow_array_base(arr_expr, line)?;
                        let mut idxs = Vec::with_capacity(indices.len());
                        for ix in indices {
                            idxs.push(self.eval_expr(ix)?.to_int());
                        }
                        return self
                            .compound_assign_arrow_array_slice(container, idxs, *op, rhs, line);
                    }
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
                if self.eval_boolean_rvalue_condition(condition)? {
                    self.eval_expr(then_expr)
                } else {
                    self.eval_expr(else_expr)
                }
            }

            // Range
            ExprKind::Range {
                from,
                to,
                exclusive,
            } => {
                if ctx == WantarrayCtx::List {
                    let f = self.eval_expr(from)?;
                    let t = self.eval_expr(to)?;
                    let list = perl_list_range_expand(f, t);
                    Ok(PerlValue::array(list))
                } else {
                    let key = std::ptr::from_ref(expr) as usize;
                    match (&from.kind, &to.kind) {
                        (
                            ExprKind::Regex(left_pat, left_flags),
                            ExprKind::Regex(right_pat, right_flags),
                        ) => {
                            let dot = self.scalar_flipflop_dot_line();
                            let subject = self.scope.get_scalar("_").to_string();
                            let left_re = self.compile_regex(left_pat, left_flags, line).map_err(
                                |e| match e {
                                    FlowOrError::Error(err) => err,
                                    FlowOrError::Flow(_) => PerlError::runtime(
                                        "unexpected flow in regex flip-flop",
                                        line,
                                    ),
                                },
                            )?;
                            let right_re = self
                                .compile_regex(right_pat, right_flags, line)
                                .map_err(|e| match e {
                                    FlowOrError::Error(err) => err,
                                    FlowOrError::Flow(_) => PerlError::runtime(
                                        "unexpected flow in regex flip-flop",
                                        line,
                                    ),
                                })?;
                            let left_m = left_re.is_match(&subject);
                            let right_m = right_re.is_match(&subject);
                            let st = self.flip_flop_tree.entry(key).or_default();
                            Ok(PerlValue::integer(Self::regex_flip_flop_transition(
                                &mut st.active,
                                &mut st.exclusive_left_line,
                                *exclusive,
                                dot,
                                left_m,
                                right_m,
                            )))
                        }
                        (ExprKind::Regex(left_pat, left_flags), ExprKind::Eof(None)) => {
                            let dot = self.scalar_flipflop_dot_line();
                            let subject = self.scope.get_scalar("_").to_string();
                            let left_re = self.compile_regex(left_pat, left_flags, line).map_err(
                                |e| match e {
                                    FlowOrError::Error(err) => err,
                                    FlowOrError::Flow(_) => PerlError::runtime(
                                        "unexpected flow in regex/eof flip-flop",
                                        line,
                                    ),
                                },
                            )?;
                            let left_m = left_re.is_match(&subject);
                            let right_m = self.eof_without_arg_is_true();
                            let st = self.flip_flop_tree.entry(key).or_default();
                            Ok(PerlValue::integer(Self::regex_flip_flop_transition(
                                &mut st.active,
                                &mut st.exclusive_left_line,
                                *exclusive,
                                dot,
                                left_m,
                                right_m,
                            )))
                        }
                        (
                            ExprKind::Regex(left_pat, left_flags),
                            ExprKind::Integer(_) | ExprKind::Float(_),
                        ) => {
                            let dot = self.scalar_flipflop_dot_line();
                            let right = self.eval_expr(to)?.to_int();
                            let subject = self.scope.get_scalar("_").to_string();
                            let left_re = self.compile_regex(left_pat, left_flags, line).map_err(
                                |e| match e {
                                    FlowOrError::Error(err) => err,
                                    FlowOrError::Flow(_) => PerlError::runtime(
                                        "unexpected flow in regex flip-flop",
                                        line,
                                    ),
                                },
                            )?;
                            let left_m = left_re.is_match(&subject);
                            let right_m = dot == right;
                            let st = self.flip_flop_tree.entry(key).or_default();
                            Ok(PerlValue::integer(Self::regex_flip_flop_transition(
                                &mut st.active,
                                &mut st.exclusive_left_line,
                                *exclusive,
                                dot,
                                left_m,
                                right_m,
                            )))
                        }
                        (ExprKind::Regex(left_pat, left_flags), _) => {
                            if let ExprKind::Eof(Some(_)) = &to.kind {
                                return Err(FlowOrError::Error(PerlError::runtime(
                                    "regex flip-flop with eof(HANDLE) is not supported",
                                    line,
                                )));
                            }
                            let dot = self.scalar_flipflop_dot_line();
                            let subject = self.scope.get_scalar("_").to_string();
                            let left_re = self.compile_regex(left_pat, left_flags, line).map_err(
                                |e| match e {
                                    FlowOrError::Error(err) => err,
                                    FlowOrError::Flow(_) => PerlError::runtime(
                                        "unexpected flow in regex flip-flop",
                                        line,
                                    ),
                                },
                            )?;
                            let left_m = left_re.is_match(&subject);
                            let right_m = self.eval_boolean_rvalue_condition(to)?;
                            let st = self.flip_flop_tree.entry(key).or_default();
                            Ok(PerlValue::integer(Self::regex_flip_flop_transition(
                                &mut st.active,
                                &mut st.exclusive_left_line,
                                *exclusive,
                                dot,
                                left_m,
                                right_m,
                            )))
                        }
                        _ => {
                            let left = self.eval_expr(from)?.to_int();
                            let right = self.eval_expr(to)?.to_int();
                            let dot = self.scalar_flipflop_dot_line();
                            let st = self.flip_flop_tree.entry(key).or_default();
                            if !st.active {
                                if dot == left {
                                    st.active = true;
                                    if *exclusive {
                                        st.exclusive_left_line = Some(dot);
                                    } else {
                                        st.exclusive_left_line = None;
                                        if dot == right {
                                            st.active = false;
                                        }
                                    }
                                    return Ok(PerlValue::integer(1));
                                }
                                return Ok(PerlValue::integer(0));
                            }
                            if let Some(ll) = st.exclusive_left_line {
                                if dot == right && dot > ll {
                                    st.active = false;
                                    st.exclusive_left_line = None;
                                }
                            } else if dot == right {
                                st.active = false;
                            }
                            Ok(PerlValue::integer(1))
                        }
                    }
                }
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

            // `my $x = …` / `our` / `state` / `local` used as an expression
            // (e.g. `if (my $line = readline)`).  Declare each variable in the
            // current scope, evaluate the initializer (if any), and return the
            // assigned value(s).  Re-uses the same scope APIs as `StmtKind::My`.
            ExprKind::MyExpr { keyword, decls } => {
                // Build a temporary statement and dispatch to the canonical
                // statement handler so behavior matches `my $x = …;` exactly.
                let stmt_kind = match keyword.as_str() {
                    "my" => StmtKind::My(decls.clone()),
                    "our" => StmtKind::Our(decls.clone()),
                    "state" => StmtKind::State(decls.clone()),
                    "local" => StmtKind::Local(decls.clone()),
                    _ => StmtKind::My(decls.clone()),
                };
                let stmt = Statement {
                    label: None,
                    kind: stmt_kind,
                    line,
                };
                self.exec_statement(&stmt)?;
                // Return the value of the (first) declared variable so the
                // surrounding expression sees the assigned value, matching
                // Perl: `if (my $x = 5) { … }` evaluates the condition as 5.
                let first = decls.first().ok_or_else(|| {
                    FlowOrError::Error(PerlError::runtime("MyExpr: empty decl list", line))
                })?;
                Ok(match first.sigil {
                    Sigil::Scalar => self.scope.get_scalar(&first.name),
                    Sigil::Array => PerlValue::array(self.scope.get_array(&first.name)),
                    Sigil::Hash => {
                        let h = self.scope.get_hash(&first.name);
                        let mut flat: Vec<PerlValue> = Vec::with_capacity(h.len() * 2);
                        for (k, v) in h {
                            flat.push(PerlValue::string(k));
                            flat.push(v);
                        }
                        PerlValue::array(flat)
                    }
                    Sigil::Typeglob => PerlValue::UNDEF,
                })
            }

            // Function calls
            ExprKind::FuncCall { name, args } => {
                // read(FH, $buf, LEN [, OFFSET]) needs special handling: $buf is an lvalue
                if matches!(name.as_str(), "read" | "CORE::read") && args.len() >= 3 {
                    let fh_val = self.eval_expr(&args[0])?;
                    let fh = fh_val
                        .as_io_handle_name()
                        .unwrap_or_else(|| fh_val.to_string());
                    let len = self.eval_expr(&args[2])?.to_int().max(0) as usize;
                    let offset = if args.len() > 3 {
                        self.eval_expr(&args[3])?.to_int().max(0) as usize
                    } else {
                        0
                    };
                    // Extract the variable name from the AST
                    let var_name = match &args[1].kind {
                        ExprKind::ScalarVar(n) => n.clone(),
                        _ => self.eval_expr(&args[1])?.to_string(),
                    };
                    let mut buf = vec![0u8; len];
                    let n = if let Some(slot) = self.io_file_slots.get(&fh).cloned() {
                        slot.lock().read(&mut buf).unwrap_or(0)
                    } else if fh == "STDIN" {
                        std::io::stdin().read(&mut buf).unwrap_or(0)
                    } else {
                        return Err(PerlError::runtime(
                            format!("read: unopened handle {}", fh),
                            line,
                        )
                        .into());
                    };
                    buf.truncate(n);
                    let read_str = crate::perl_fs::decode_utf8_or_latin1(&buf);
                    if offset > 0 {
                        let mut existing = self.scope.get_scalar(&var_name).to_string();
                        while existing.len() < offset {
                            existing.push('\0');
                        }
                        existing.push_str(&read_str);
                        let _ = self
                            .scope
                            .set_scalar(&var_name, PerlValue::string(existing));
                    } else {
                        let _ = self
                            .scope
                            .set_scalar(&var_name, PerlValue::string(read_str));
                    }
                    return Ok(PerlValue::integer(n as i64));
                }
                if matches!(name.as_str(), "group_by" | "chunk_by") {
                    if args.len() != 2 {
                        return Err(PerlError::runtime(
                            "group_by/chunk_by: expected { BLOCK } or EXPR, LIST",
                            line,
                        )
                        .into());
                    }
                    return self.eval_chunk_by_builtin(&args[0], &args[1], ctx, line);
                }
                if matches!(name.as_str(), "puniq" | "pfirst" | "pany") {
                    let mut arg_vals = Vec::with_capacity(args.len());
                    for a in args {
                        arg_vals.push(self.eval_expr(a)?);
                    }
                    let saved_wa = self.wantarray_kind;
                    self.wantarray_kind = ctx;
                    let r = self.eval_par_list_call(name.as_str(), &arg_vals, ctx, line);
                    self.wantarray_kind = saved_wa;
                    return r.map_err(Into::into);
                }
                let arg_vals = if matches!(name.as_str(), "any" | "all" | "none" | "first")
                    || matches!(
                        name.as_str(),
                        "take_while" | "drop_while" | "skip_while" | "reject" | "tap" | "peek"
                    )
                    || matches!(
                        name.as_str(),
                        "partition" | "min_by" | "max_by" | "zip_with" | "count_by"
                    ) {
                    if args.len() != 2 {
                        return Err(PerlError::runtime(
                            format!("{}: expected BLOCK, LIST", name),
                            line,
                        )
                        .into());
                    }
                    let cr = self.eval_expr(&args[0])?;
                    let list_src = self.eval_expr_ctx(&args[1], WantarrayCtx::List)?;
                    let mut v = vec![cr];
                    v.extend(list_src.to_list());
                    v
                } else if matches!(
                    name.as_str(),
                    "zip" | "List::Util::zip" | "List::Util::zip_longest"
                ) {
                    let mut v = Vec::with_capacity(args.len());
                    for a in args {
                        v.push(self.eval_expr_ctx(a, WantarrayCtx::List)?);
                    }
                    v
                } else if matches!(
                    name.as_str(),
                    "uniq"
                        | "distinct"
                        | "uniqstr"
                        | "uniqint"
                        | "uniqnum"
                        | "flatten"
                        | "set"
                        | "list_count"
                        | "list_size"
                        | "count"
                        | "size"
                        | "cnt"
                        | "with_index"
                        | "List::Util::uniq"
                        | "List::Util::uniqstr"
                        | "List::Util::uniqint"
                        | "List::Util::uniqnum"
                        | "shuffle"
                        | "List::Util::shuffle"
                        | "sum"
                        | "sum0"
                        | "product"
                        | "min"
                        | "max"
                        | "minstr"
                        | "maxstr"
                        | "mean"
                        | "median"
                        | "mode"
                        | "stddev"
                        | "variance"
                        | "List::Util::sum"
                        | "List::Util::sum0"
                        | "List::Util::product"
                        | "List::Util::min"
                        | "List::Util::max"
                        | "List::Util::minstr"
                        | "List::Util::maxstr"
                        | "List::Util::mean"
                        | "List::Util::median"
                        | "List::Util::mode"
                        | "List::Util::stddev"
                        | "List::Util::variance"
                        | "pairs"
                        | "unpairs"
                        | "pairkeys"
                        | "pairvalues"
                        | "List::Util::pairs"
                        | "List::Util::unpairs"
                        | "List::Util::pairkeys"
                        | "List::Util::pairvalues"
                ) {
                    // Perl prototype `(@)`: one slurpy list — either one list expr (`uniq @x`) or
                    // multiple actuals (`List::Util::uniq(1, 1, 2)`). Each actual is evaluated in
                    // list context so `@a, @b` flattens like Perl.
                    let mut list_out = Vec::new();
                    if args.len() == 1 {
                        list_out = self.eval_expr_ctx(&args[0], WantarrayCtx::List)?.to_list();
                    } else {
                        for a in args {
                            list_out.extend(self.eval_expr_ctx(a, WantarrayCtx::List)?.to_list());
                        }
                    }
                    list_out
                } else if matches!(
                    name.as_str(),
                    "take" | "head" | "tail" | "drop" | "List::Util::head" | "List::Util::tail"
                ) {
                    if args.is_empty() {
                        return Err(PerlError::runtime(
                            "take/head/tail/drop/List::Util::head|tail: need LIST..., N or unary N",
                            line,
                        )
                        .into());
                    }
                    let mut arg_vals = Vec::with_capacity(args.len());
                    if args.len() == 1 {
                        // head @l == head @l, 1 — evaluate in list context
                        arg_vals.push(self.eval_expr_ctx(&args[0], WantarrayCtx::List)?);
                    } else {
                        for a in &args[..args.len() - 1] {
                            arg_vals.push(self.eval_expr_ctx(a, WantarrayCtx::List)?);
                        }
                        arg_vals.push(self.eval_expr(&args[args.len() - 1])?);
                    }
                    arg_vals
                } else if matches!(
                    name.as_str(),
                    "chunked" | "List::Util::chunked" | "windowed" | "List::Util::windowed"
                ) {
                    let mut list_out = Vec::new();
                    match args.len() {
                        0 => {
                            return Err(PerlError::runtime(
                                format!("{name}: expected (LIST, N) or unary N after |>"),
                                line,
                            )
                            .into());
                        }
                        1 => {
                            // chunked @l / windowed @l — evaluate in list context, default size
                            list_out.push(self.eval_expr_ctx(&args[0], WantarrayCtx::List)?);
                        }
                        2 => {
                            list_out.extend(
                                self.eval_expr_ctx(&args[0], WantarrayCtx::List)?.to_list(),
                            );
                            list_out.push(self.eval_expr(&args[1])?);
                        }
                        _ => {
                            return Err(PerlError::runtime(
                                format!(
                                    "{name}: expected exactly (LIST, N); use one list expression then size"
                                ),
                                line,
                            )
                            .into());
                        }
                    }
                    list_out
                } else {
                    // Generic sub call: args are in list context so `f(1..10)`, `f(@a)`,
                    // `f(reverse LIST)` flatten into `@_` (matches Perl's call list semantics).
                    let mut arg_vals = Vec::with_capacity(args.len());
                    for a in args {
                        let v = self.eval_expr_ctx(a, WantarrayCtx::List)?;
                        if let Some(items) = v.as_array_vec() {
                            arg_vals.extend(items);
                        } else {
                            arg_vals.push(v);
                        }
                    }
                    arg_vals
                };
                // Builtins read [`Self::wantarray_kind`] (VM sets it too); thread `ctx` through.
                let saved_wa = self.wantarray_kind;
                self.wantarray_kind = ctx;
                if matches!(
                    name.as_str(),
                    "take_while" | "drop_while" | "skip_while" | "reject" | "tap" | "peek"
                ) {
                    let r = self.list_higher_order_block_builtin(name.as_str(), &arg_vals, line);
                    self.wantarray_kind = saved_wa;
                    return r.map_err(Into::into);
                }
                if let Some(r) = crate::builtins::try_builtin(self, name.as_str(), &arg_vals, line)
                {
                    self.wantarray_kind = saved_wa;
                    return r.map_err(Into::into);
                }
                self.wantarray_kind = saved_wa;
                self.call_named_sub(name, arg_vals, line, ctx)
            }
            ExprKind::IndirectCall {
                target,
                args,
                ampersand: _,
                pass_caller_arglist,
            } => {
                let tval = self.eval_expr(target)?;
                let arg_vals = if *pass_caller_arglist {
                    self.scope.get_array("_")
                } else {
                    let mut v = Vec::with_capacity(args.len());
                    for a in args {
                        v.push(self.eval_expr(a)?);
                    }
                    v
                };
                self.dispatch_indirect_call(tval, arg_vals, ctx, line)
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
                if method == "VERSION" && !*super_call {
                    if let Some(ver) = self.package_version_scalar(class.as_str())? {
                        return Ok(ver);
                    }
                }
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
                    msg.push_str(&self.die_warn_at_suffix(line));
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
                    msg.push_str(&self.die_warn_at_suffix(line));
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
                delim: _,
            } => {
                let val = self.eval_expr(expr)?;
                if val.is_iterator() {
                    let source = crate::map_stream::into_pull_iter(val);
                    let re = self.compile_regex(pattern, flags, line)?;
                    let global = flags.contains('g');
                    if global {
                        return Ok(PerlValue::iterator(std::sync::Arc::new(
                            crate::map_stream::MatchGlobalStreamIterator::new(source, re),
                        )));
                    } else {
                        return Ok(PerlValue::iterator(std::sync::Arc::new(
                            crate::map_stream::MatchStreamIterator::new(source, re),
                        )));
                    }
                }
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
                delim: _,
            } => {
                let val = self.eval_expr(expr)?;
                if val.is_iterator() {
                    let source = crate::map_stream::into_pull_iter(val);
                    let re = self.compile_regex(pattern, flags, line)?;
                    let global = flags.contains('g');
                    return Ok(PerlValue::iterator(std::sync::Arc::new(
                        crate::map_stream::SubstStreamIterator::new(
                            source,
                            re,
                            replacement.clone(),
                            global,
                        ),
                    )));
                }
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
                delim: _,
            } => {
                let val = self.eval_expr(expr)?;
                if val.is_iterator() {
                    let source = crate::map_stream::into_pull_iter(val);
                    return Ok(PerlValue::iterator(std::sync::Arc::new(
                        crate::map_stream::TransliterateStreamIterator::new(
                            source, from, to, flags,
                        ),
                    )));
                }
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
            ExprKind::MapExpr {
                block,
                list,
                flatten_array_refs,
                stream,
            } => {
                let list_val = self.eval_expr_ctx(list, WantarrayCtx::List)?;
                if *stream {
                    let out =
                        self.map_stream_block_output(list_val, block, *flatten_array_refs, line)?;
                    if ctx == WantarrayCtx::List {
                        return Ok(out);
                    }
                    return Ok(PerlValue::integer(out.to_list().len() as i64));
                }
                let items = list_val.to_list();
                if items.len() == 1 {
                    if let Some(p) = items[0].as_pipeline() {
                        if *flatten_array_refs {
                            return Err(PerlError::runtime(
                                "flat_map onto a pipeline value is not supported in this form — use a pipeline ->map stage",
                                line,
                            )
                            .into());
                        }
                        let sub = self.anon_coderef_from_block(block);
                        self.pipeline_push(&p, PipelineOp::Map(sub), line)?;
                        return Ok(PerlValue::pipeline(Arc::clone(&p)));
                    }
                }
                // `map { BLOCK } LIST` evaluates BLOCK in list context so its tail statement's
                // list value (comma operator, `..`, `reverse`, `grep`, `@array`, `return
                // wantarray-aware sub`, …) flattens into the output instead of collapsing to a
                // scalar. Matches Perl's `perlfunc` note that the block is always list context.
                let mut result = Vec::new();
                for item in items {
                    self.scope.set_topic(item);
                    let val = self.exec_block_with_tail(block, WantarrayCtx::List)?;
                    result.extend(val.map_flatten_outputs(*flatten_array_refs));
                }
                if ctx == WantarrayCtx::List {
                    Ok(PerlValue::array(result))
                } else {
                    Ok(PerlValue::integer(result.len() as i64))
                }
            }
            ExprKind::ForEachExpr { block, list } => {
                let list_val = self.eval_expr_ctx(list, WantarrayCtx::List)?;
                // Lazy: consume iterator one-at-a-time without materializing.
                if list_val.is_iterator() {
                    let iter = list_val.into_iterator();
                    let mut count = 0i64;
                    while let Some(item) = iter.next_item() {
                        count += 1;
                        self.scope.set_topic(item);
                        self.exec_block(block)?;
                    }
                    return Ok(PerlValue::integer(count));
                }
                let items = list_val.to_list();
                let count = items.len();
                for item in items {
                    self.scope.set_topic(item);
                    self.exec_block(block)?;
                }
                Ok(PerlValue::integer(count as i64))
            }
            ExprKind::MapExprComma {
                expr,
                list,
                flatten_array_refs,
                stream,
            } => {
                let list_val = self.eval_expr_ctx(list, WantarrayCtx::List)?;
                if *stream {
                    let out =
                        self.map_stream_expr_output(list_val, expr, *flatten_array_refs, line)?;
                    if ctx == WantarrayCtx::List {
                        return Ok(out);
                    }
                    return Ok(PerlValue::integer(out.to_list().len() as i64));
                }
                let items = list_val.to_list();
                let mut result = Vec::new();
                for item in items {
                    self.scope.set_topic(item.clone());
                    let val = self.eval_expr_ctx(expr, WantarrayCtx::List)?;
                    result.extend(val.map_flatten_outputs(*flatten_array_refs));
                }
                if ctx == WantarrayCtx::List {
                    Ok(PerlValue::array(result))
                } else {
                    Ok(PerlValue::integer(result.len() as i64))
                }
            }
            ExprKind::GrepExpr {
                block,
                list,
                keyword,
            } => {
                let list_val = self.eval_expr_ctx(list, WantarrayCtx::List)?;
                if keyword.is_stream() {
                    let out = self.filter_stream_block_output(list_val, block, line)?;
                    if ctx == WantarrayCtx::List {
                        return Ok(out);
                    }
                    return Ok(PerlValue::integer(out.to_list().len() as i64));
                }
                let items = list_val.to_list();
                if items.len() == 1 {
                    if let Some(p) = items[0].as_pipeline() {
                        let sub = self.anon_coderef_from_block(block);
                        self.pipeline_push(&p, PipelineOp::Filter(sub), line)?;
                        return Ok(PerlValue::pipeline(Arc::clone(&p)));
                    }
                }
                let mut result = Vec::new();
                for item in items {
                    self.scope.set_topic(item.clone());
                    let val = self.exec_block(block)?;
                    if val.is_true() {
                        result.push(item);
                    }
                }
                if ctx == WantarrayCtx::List {
                    Ok(PerlValue::array(result))
                } else {
                    Ok(PerlValue::integer(result.len() as i64))
                }
            }
            ExprKind::GrepExprComma {
                expr,
                list,
                keyword,
            } => {
                let list_val = self.eval_expr_ctx(list, WantarrayCtx::List)?;
                if keyword.is_stream() {
                    let out = self.filter_stream_expr_output(list_val, expr, line)?;
                    if ctx == WantarrayCtx::List {
                        return Ok(out);
                    }
                    return Ok(PerlValue::integer(out.to_list().len() as i64));
                }
                let items = list_val.to_list();
                let mut result = Vec::new();
                for item in items {
                    self.scope.set_topic(item.clone());
                    let val = self.eval_expr(expr)?;
                    if val.is_true() {
                        result.push(item);
                    }
                }
                if ctx == WantarrayCtx::List {
                    Ok(PerlValue::array(result))
                } else {
                    Ok(PerlValue::integer(result.len() as i64))
                }
            }
            ExprKind::SortExpr { cmp, list } => {
                let list_val = self.eval_expr(list)?;
                let mut items = list_val.to_list();
                match cmp {
                    Some(SortComparator::Code(code_expr)) => {
                        let sub = self.eval_expr(code_expr)?;
                        let Some(sub) = sub.as_code_ref() else {
                            return Err(PerlError::runtime(
                                "sort: comparator must be a code reference",
                                line,
                            )
                            .into());
                        };
                        let sub = sub.clone();
                        items.sort_by(|a, b| {
                            let _ = self.scope.set_scalar("a", a.clone());
                            let _ = self.scope.set_scalar("b", b.clone());
                            let _ = self.scope.set_scalar("_0", a.clone());
                            let _ = self.scope.set_scalar("_1", b.clone());
                            match self.call_sub(&sub, vec![], ctx, line) {
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
                            }
                        });
                    }
                    Some(SortComparator::Block(cmp_block)) => {
                        if let Some(mode) = detect_sort_block_fast(cmp_block) {
                            items.sort_by(|a, b| sort_magic_cmp(a, b, mode));
                        } else {
                            let cmp_block = cmp_block.clone();
                            items.sort_by(|a, b| {
                                let _ = self.scope.set_scalar("a", a.clone());
                                let _ = self.scope.set_scalar("b", b.clone());
                                let _ = self.scope.set_scalar("_0", a.clone());
                                let _ = self.scope.set_scalar("_1", b.clone());
                                match self.exec_block(&cmp_block) {
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
                                }
                            });
                        }
                    }
                    None => {
                        items.sort_by_key(|a| a.to_string());
                    }
                }
                Ok(PerlValue::array(items))
            }
            ExprKind::ScalarReverse(expr) => {
                let val = self.eval_expr_ctx(expr, WantarrayCtx::List)?;
                // Lazy: wrap iterator without materializing
                if val.is_iterator() {
                    return Ok(PerlValue::iterator(Arc::new(
                        crate::value::ScalarReverseIterator::new(val.into_iterator()),
                    )));
                }
                let items = val.to_list();
                if items.len() <= 1 {
                    let s = if items.is_empty() {
                        String::new()
                    } else {
                        items[0].to_string()
                    };
                    Ok(PerlValue::string(s.chars().rev().collect()))
                } else {
                    let mut items = items;
                    items.reverse();
                    Ok(PerlValue::array(items))
                }
            }
            ExprKind::ReverseExpr(list) => {
                let val = self.eval_expr_ctx(list, WantarrayCtx::List)?;
                match ctx {
                    WantarrayCtx::List => {
                        let mut items = val.to_list();
                        items.reverse();
                        Ok(PerlValue::array(items))
                    }
                    _ => {
                        let items = val.to_list();
                        let s: String = items.iter().map(|v| v.to_string()).collect();
                        Ok(PerlValue::string(s.chars().rev().collect()))
                    }
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
            } => {
                self.eval_par_walk_expr(path.as_ref(), callback.as_ref(), progress.as_deref(), line)
            }
            ExprKind::PwatchExpr { path, callback } => {
                self.eval_pwatch_expr(path.as_ref(), callback.as_ref(), line)
            }
            ExprKind::PMapExpr {
                block,
                list,
                progress,
                flat_outputs,
                on_cluster,
            } => {
                let show_progress = progress
                    .as_ref()
                    .map(|p| self.eval_expr(p))
                    .transpose()?
                    .map(|v| v.is_true())
                    .unwrap_or(false);
                let list_val = self.eval_expr(list)?;
                if let Some(cluster_e) = on_cluster {
                    let cluster_val = self.eval_expr(cluster_e.as_ref())?;
                    return self.eval_pmap_remote(
                        cluster_val,
                        list_val,
                        show_progress,
                        block,
                        *flat_outputs,
                        line,
                    );
                }
                let items = list_val.to_list();
                let block = block.clone();
                let subs = self.subs.clone();
                let (scope_capture, atomic_arrays, atomic_hashes) =
                    self.scope.capture_with_atomics();
                let pmap_progress = PmapProgress::new(show_progress, items.len());

                if *flat_outputs {
                    let mut indexed: Vec<(usize, Vec<PerlValue>)> = items
                        .into_par_iter()
                        .enumerate()
                        .map(|(i, item)| {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            local_interp
                                .scope
                                .restore_atomics(&atomic_arrays, &atomic_hashes);
                            local_interp.enable_parallel_guard();
                            local_interp.scope.set_topic(item);
                            let val = match local_interp.exec_block(&block) {
                                Ok(val) => val,
                                Err(_) => PerlValue::UNDEF,
                            };
                            let chunk = val.map_flatten_outputs(true);
                            pmap_progress.tick();
                            (i, chunk)
                        })
                        .collect();
                    pmap_progress.finish();
                    indexed.sort_by_key(|(i, _)| *i);
                    let results: Vec<PerlValue> =
                        indexed.into_iter().flat_map(|(_, v)| v).collect();
                    Ok(PerlValue::array(results))
                } else {
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
                            local_interp.scope.set_topic(item);
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
                            local_interp.scope.set_topic(item);
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
                        local_interp.scope.set_topic(item.clone());
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
                    local_interp.scope.set_topic(item);
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
                            local_interp.scope.set_topic(PerlValue::integer(i as i64));
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
                    local_interp.scope.set_topic(PerlValue::integer(i as i64));
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
            ExprKind::RetryBlock {
                body,
                times,
                backoff,
            } => self.eval_retry_block(body, times, *backoff, line),
            ExprKind::RateLimitBlock {
                slot,
                max,
                window,
                body,
            } => self.eval_rate_limit_block(*slot, max, window, body, line),
            ExprKind::EveryBlock { interval, body } => self.eval_every_block(interval, body, line),
            ExprKind::GenBlock { body } => {
                let g = Arc::new(PerlGenerator {
                    block: body.clone(),
                    pc: Mutex::new(0),
                    scope_started: Mutex::new(false),
                    exhausted: Mutex::new(false),
                });
                Ok(PerlValue::generator(g))
            }
            ExprKind::Yield(e) => {
                if !self.in_generator {
                    return Err(PerlError::runtime("yield outside gen block", line).into());
                }
                let v = self.eval_expr(e)?;
                Err(FlowOrError::Flow(Flow::Yield(v)))
            }
            ExprKind::AlgebraicMatch { subject, arms } => {
                self.eval_algebraic_match(subject, arms, line)
            }
            ExprKind::AsyncBlock { body } | ExprKind::SpawnBlock { body } => {
                Ok(self.spawn_async_block(body))
            }
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
                read_file_text_perl_compat(&path)
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
                let stdout = decode_utf8_or_latin1(&output.stdout);
                let stderr = decode_utf8_or_latin1(&output.stderr);
                Ok(PerlValue::capture(Arc::new(CaptureResult {
                    stdout,
                    stderr,
                    exitcode,
                })))
            }
            ExprKind::Qx(e) => {
                let cmd = self.eval_expr(e)?.to_string();
                crate::capture::run_readpipe(self, &cmd, line).map_err(FlowOrError::Error)
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
                            let _ = local_interp.scope.set_scalar("_0", a.clone());
                            let _ = local_interp.scope.set_scalar("_1", b.clone());
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
                    let _ = local_interp.scope.set_scalar("a", acc.clone());
                    let _ = local_interp.scope.set_scalar("b", b.clone());
                    let _ = local_interp.scope.set_scalar("_0", acc);
                    let _ = local_interp.scope.set_scalar("_1", b);
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
                        let _ = local_interp.scope.set_scalar("a", a.clone());
                        let _ = local_interp.scope.set_scalar("b", b.clone());
                        let _ = local_interp.scope.set_scalar("_0", a);
                        let _ = local_interp.scope.set_scalar("_1", b);
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
                    local_interp.scope.set_topic(items[0].clone());
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
                        local_interp.scope.set_topic(item);
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
                        let _ = local_interp.scope.set_scalar("a", a.clone());
                        let _ = local_interp.scope.set_scalar("b", b.clone());
                        let _ = local_interp.scope.set_scalar("_0", a);
                        let _ = local_interp.scope.set_scalar("_1", b);
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
                        local_interp.scope.set_topic(item.clone());
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
                ctx,
                line,
            ),
            ExprKind::Delete(expr) => self.eval_delete_operand(expr.as_ref(), line),
            ExprKind::Exists(expr) => self.eval_exists_operand(expr.as_ref(), line),
            ExprKind::Keys(expr) => {
                let val = self.eval_expr_ctx(expr, WantarrayCtx::List)?;
                let keys = Self::keys_from_value(val, line)?;
                if ctx == WantarrayCtx::List {
                    Ok(keys)
                } else {
                    let n = keys.as_array_vec().map(|a| a.len()).unwrap_or(0);
                    Ok(PerlValue::integer(n as i64))
                }
            }
            ExprKind::Values(expr) => {
                let val = self.eval_expr_ctx(expr, WantarrayCtx::List)?;
                let vals = Self::values_from_value(val, line)?;
                if ctx == WantarrayCtx::List {
                    Ok(vals)
                } else {
                    let n = vals.as_array_vec().map(|a| a.len()).unwrap_or(0);
                    Ok(PerlValue::integer(n as i64))
                }
            }
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
                // sprintf args are Perl list context — splat ranges, arrays, and list-valued
                // builtins into individual format arguments.
                let mut arg_vals = Vec::new();
                for a in args {
                    let v = self.eval_expr_ctx(a, WantarrayCtx::List)?;
                    if let Some(items) = v.as_array_vec() {
                        arg_vals.extend(items);
                    } else {
                        arg_vals.push(v);
                    }
                }
                let s = self.perl_sprintf_stringify(&fmt, &arg_vals, line)?;
                Ok(PerlValue::string(s))
            }
            ExprKind::JoinExpr { separator, list } => {
                let sep = self.eval_expr(separator)?.to_string();
                // Like Perl 5, arguments after the separator are evaluated in list context so
                // `join(",", uniq @x)` passes list context into `uniq`, and `join(",", localtime())`
                // expands `localtime` to nine fields.
                let items = if let ExprKind::List(exprs) = &list.kind {
                    let saved = self.wantarray_kind;
                    self.wantarray_kind = WantarrayCtx::List;
                    let mut vals = Vec::new();
                    for e in exprs {
                        let v = self.eval_expr_ctx(e, self.wantarray_kind)?;
                        if let Some(items) = v.as_array_vec() {
                            vals.extend(items);
                        } else {
                            vals.push(v);
                        }
                    }
                    self.wantarray_kind = saved;
                    vals
                } else {
                    let saved = self.wantarray_kind;
                    self.wantarray_kind = WantarrayCtx::List;
                    let v = self.eval_expr_ctx(list, WantarrayCtx::List)?;
                    self.wantarray_kind = saved;
                    if let Some(items) = v.as_array_vec() {
                        items
                    } else {
                        vec![v]
                    }
                };
                let mut strs = Vec::with_capacity(items.len());
                for v in &items {
                    strs.push(self.stringify_value(v.clone(), line)?);
                }
                Ok(PerlValue::string(strs.join(&sep)))
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
                Ok(Self::study_return_value(&s))
            }

            // Type
            ExprKind::Defined(expr) => {
                // Perl: `defined &foo` / `defined &Pkg::name` — true iff the subroutine exists (no call).
                if let ExprKind::SubroutineRef(name) = &expr.kind {
                    let exists = self.resolve_sub_by_name(name).is_some();
                    return Ok(PerlValue::integer(if exists { 1 } else { 0 }));
                }
                let val = self.eval_expr(expr)?;
                Ok(PerlValue::integer(if val.is_undef() { 0 } else { 1 }))
            }
            ExprKind::Ref(expr) => {
                let val = self.eval_expr(expr)?;
                Ok(val.ref_type())
            }
            ExprKind::ScalarContext(expr) => {
                let v = self.eval_expr_ctx(expr, WantarrayCtx::Scalar)?;
                Ok(v.scalar_context())
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
            ExprKind::OpenMyHandle { .. } => Err(PerlError::runtime(
                "internal: `open my $fh` handle used outside open()",
                line,
            )
            .into()),
            ExprKind::Open { handle, mode, file } => {
                if let ExprKind::OpenMyHandle { name } = &handle.kind {
                    self.scope
                        .declare_scalar_frozen(name, PerlValue::UNDEF, false, None)?;
                    self.english_note_lexical_scalar(name);
                    let mode_s = self.eval_expr(mode)?.to_string();
                    let file_opt = if let Some(f) = file {
                        Some(self.eval_expr(f)?.to_string())
                    } else {
                        None
                    };
                    let ret = self.open_builtin_execute(name.clone(), mode_s, file_opt, line)?;
                    self.scope.set_scalar(name, ret.clone())?;
                    return Ok(ret);
                }
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
            ExprKind::ReadLine(handle) => if ctx == WantarrayCtx::List {
                self.readline_builtin_execute_list(handle.as_deref())
            } else {
                self.readline_builtin_execute(handle.as_deref())
            }
            .map_err(Into::into),
            ExprKind::Eof(expr) => match expr {
                None => self.eof_builtin_execute(&[], line).map_err(Into::into),
                Some(e) => {
                    let name = self.eval_expr(e)?;
                    self.eof_builtin_execute(&[name], line).map_err(Into::into)
                }
            },

            ExprKind::Opendir { handle, path } => {
                let h = self.eval_expr(handle)?.to_string();
                let p = self.eval_expr(path)?.to_string();
                Ok(self.opendir_handle(&h, &p))
            }
            ExprKind::Readdir(e) => {
                let h = self.eval_expr(e)?.to_string();
                Ok(if ctx == WantarrayCtx::List {
                    self.readdir_handle_list(&h)
                } else {
                    self.readdir_handle(&h)
                })
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
                // -M, -A, -C return fractional days (float), not boolean
                if matches!(op, 'M' | 'A' | 'C') {
                    #[cfg(unix)]
                    {
                        return match crate::perl_fs::filetest_age_days(&path, *op) {
                            Some(days) => Ok(PerlValue::float(days)),
                            None => Ok(PerlValue::UNDEF),
                        };
                    }
                    #[cfg(not(unix))]
                    return Ok(PerlValue::UNDEF);
                }
                // -s returns file size (or undef on error)
                if *op == 's' {
                    return match std::fs::metadata(&path) {
                        Ok(m) => Ok(PerlValue::integer(m.len() as i64)),
                        Err(_) => Ok(PerlValue::UNDEF),
                    };
                }
                let result = match op {
                    'e' => std::path::Path::new(&path).exists(),
                    'f' => std::path::Path::new(&path).is_file(),
                    'd' => std::path::Path::new(&path).is_dir(),
                    'l' => std::path::Path::new(&path).is_symlink(),
                    #[cfg(unix)]
                    'r' => crate::perl_fs::filetest_effective_access(&path, 4),
                    #[cfg(not(unix))]
                    'r' => std::fs::metadata(&path).is_ok(),
                    #[cfg(unix)]
                    'w' => crate::perl_fs::filetest_effective_access(&path, 2),
                    #[cfg(not(unix))]
                    'w' => std::fs::metadata(&path).is_ok(),
                    #[cfg(unix)]
                    'x' => crate::perl_fs::filetest_effective_access(&path, 1),
                    #[cfg(not(unix))]
                    'x' => false,
                    #[cfg(unix)]
                    'o' => crate::perl_fs::filetest_owned_effective(&path),
                    #[cfg(not(unix))]
                    'o' => false,
                    #[cfg(unix)]
                    'R' => crate::perl_fs::filetest_real_access(&path, libc::R_OK),
                    #[cfg(not(unix))]
                    'R' => false,
                    #[cfg(unix)]
                    'W' => crate::perl_fs::filetest_real_access(&path, libc::W_OK),
                    #[cfg(not(unix))]
                    'W' => false,
                    #[cfg(unix)]
                    'X' => crate::perl_fs::filetest_real_access(&path, libc::X_OK),
                    #[cfg(not(unix))]
                    'X' => false,
                    #[cfg(unix)]
                    'O' => crate::perl_fs::filetest_owned_real(&path),
                    #[cfg(not(unix))]
                    'O' => false,
                    'z' => std::fs::metadata(&path)
                        .map(|m| m.len() == 0)
                        .unwrap_or(true),
                    't' => crate::perl_fs::filetest_is_tty(&path),
                    #[cfg(unix)]
                    'p' => crate::perl_fs::filetest_is_pipe(&path),
                    #[cfg(not(unix))]
                    'p' => false,
                    #[cfg(unix)]
                    'S' => crate::perl_fs::filetest_is_socket(&path),
                    #[cfg(not(unix))]
                    'S' => false,
                    #[cfg(unix)]
                    'b' => crate::perl_fs::filetest_is_block_device(&path),
                    #[cfg(not(unix))]
                    'b' => false,
                    #[cfg(unix)]
                    'c' => crate::perl_fs::filetest_is_char_device(&path),
                    #[cfg(not(unix))]
                    'c' => false,
                    #[cfg(unix)]
                    'u' => crate::perl_fs::filetest_is_setuid(&path),
                    #[cfg(not(unix))]
                    'u' => false,
                    #[cfg(unix)]
                    'g' => crate::perl_fs::filetest_is_setgid(&path),
                    #[cfg(not(unix))]
                    'g' => false,
                    #[cfg(unix)]
                    'k' => crate::perl_fs::filetest_is_sticky(&path),
                    #[cfg(not(unix))]
                    'k' => false,
                    'T' => crate::perl_fs::filetest_is_text(&path),
                    'B' => crate::perl_fs::filetest_is_binary(&path),
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
                    ExprKind::CodeRef { body, .. } => match self.exec_block_with_tail(body, ctx) {
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
            ExprKind::Do(expr) => match &expr.kind {
                ExprKind::CodeRef { body, .. } => self.exec_block_with_tail(body, ctx),
                _ => {
                    let val = self.eval_expr(expr)?;
                    let filename = val.to_string();
                    match read_file_text_perl_compat(&filename) {
                        Ok(code) => {
                            let code = crate::data_section::strip_perl_end_marker(&code);
                            match crate::parse_and_run_string_in_file(code, self, &filename) {
                                Ok(v) => Ok(v),
                                Err(e) => {
                                    self.set_eval_error(e.to_string());
                                    Ok(PerlValue::UNDEF)
                                }
                            }
                        }
                        Err(e) => {
                            self.apply_io_error_to_errno(&e);
                            Ok(PerlValue::UNDEF)
                        }
                    }
                }
            },
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
            ExprKind::Files(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(crate::perl_fs::list_files(&dir))
            }
            ExprKind::Filesf(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(crate::perl_fs::list_filesf(&dir))
            }
            ExprKind::FilesfRecursive(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(PerlValue::iterator(Arc::new(
                    crate::value::FsWalkIterator::new(&dir, true),
                )))
            }
            ExprKind::Dirs(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(crate::perl_fs::list_dirs(&dir))
            }
            ExprKind::DirsRecursive(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(PerlValue::iterator(Arc::new(
                    crate::value::FsWalkIterator::new(&dir, false),
                )))
            }
            ExprKind::SymLinks(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(crate::perl_fs::list_sym_links(&dir))
            }
            ExprKind::Sockets(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(crate::perl_fs::list_sockets(&dir))
            }
            ExprKind::Pipes(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(crate::perl_fs::list_pipes(&dir))
            }
            ExprKind::BlockDevices(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(crate::perl_fs::list_block_devices(&dir))
            }
            ExprKind::CharDevices(args) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    self.eval_expr(&args[0])?.to_string()
                };
                Ok(crate::perl_fs::list_char_devices(&dir))
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
                Ok(PerlValue::blessed(Arc::new(
                    crate::value::BlessedRef::new_blessed(class_name, val),
                )))
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
                let items = self.eval_expr_ctx(list, WantarrayCtx::List)?.to_list();
                let mut last = PerlValue::UNDEF;
                for item in items {
                    self.scope.set_topic(item);
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
                    buf.push_str(line_out.trim_end());
                    buf.push('\n');
                }
            }
        }
        Ok(buf)
    }

    /// Resolve `write FH` / `write $fh` — same handle shapes as `$fh->print` ([`Self::try_native_method`]).
    pub(crate) fn resolve_write_output_handle(
        &self,
        v: &PerlValue,
        line: usize,
    ) -> PerlResult<String> {
        if let Some(n) = v.as_io_handle_name() {
            let n = self.resolve_io_handle_name(&n);
            if self.is_bound_handle(&n) {
                return Ok(n);
            }
        }
        if let Some(s) = v.as_str() {
            if self.is_bound_handle(&s) {
                return Ok(self.resolve_io_handle_name(&s));
            }
        }
        let s = v.to_string();
        if self.is_bound_handle(&s) {
            return Ok(self.resolve_io_handle_name(&s));
        }
        Err(PerlError::runtime(
            format!("write: invalid or unopened filehandle {}", s),
            line,
        ))
    }

    /// `write` — output one record using `$~` format name in the current package (subset of Perl).
    /// With no args, uses [`Self::default_print_handle`] (Perl `select`); with one arg, writes to
    /// that handle like `write FH`.
    pub(crate) fn write_format_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        let handle_name = match args.len() {
            0 => self.default_print_handle.clone(),
            1 => self.resolve_write_output_handle(&args[0], line)?,
            _ => {
                return Err(PerlError::runtime("write: too many arguments", line));
            }
        };
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
        self.write_formatted_print(handle_name.as_str(), &out, line)?;
        Ok(PerlValue::integer(1))
    }

    pub(crate) fn try_overload_stringify(
        &mut self,
        v: &PerlValue,
        line: usize,
    ) -> Option<ExecResult> {
        let br = v.as_blessed_ref()?;
        let class = br.class.clone();
        let map = self.overload_table.get(&class)?;
        let sub_short = Self::overload_stringify_method(map)?;
        let fq = format!("{}::{}", class, sub_short);
        let sub = self.subs.get(&fq)?.clone();
        Some(self.call_sub(&sub, vec![v.clone()], WantarrayCtx::Scalar, line))
    }

    pub(crate) fn try_overload_binop(
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
    pub(crate) fn try_overload_unary_dispatch(
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
            // Perl `+` is numeric addition only; string concatenation is `.`.
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
                    let int_pow = (b >= 0)
                        .then(|| u32::try_from(b).ok())
                        .flatten()
                        .and_then(|bu| a.checked_pow(bu))
                        .map(PerlValue::integer);
                    int_pow.unwrap_or_else(|| PerlValue::float(lv.to_number().powf(rv.to_number())))
                } else {
                    PerlValue::float(lv.to_number().powf(rv.to_number()))
                }
            }
            BinOp::Concat => {
                let mut s = String::new();
                lv.append_to(&mut s);
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

    /// Perl 5 rejects `++@{...}`, `++%{...}`, postfix `@{...}++`, etc. (`Can't modify array/hash
    /// dereference in pre/postincrement/decrement`). Do not treat these as numeric ops on aggregate
    /// length — that was silently wrong vs `perl`.
    fn err_modify_symbolic_aggregate_deref_inc_dec(
        kind: Sigil,
        is_pre: bool,
        is_inc: bool,
        line: usize,
    ) -> FlowOrError {
        let agg = match kind {
            Sigil::Array => "array",
            Sigil::Hash => "hash",
            _ => unreachable!("expected symbolic @{{}} or %{{}} deref"),
        };
        let op = match (is_pre, is_inc) {
            (true, true) => "preincrement (++)",
            (true, false) => "predecrement (--)",
            (false, true) => "postincrement (++)",
            (false, false) => "postdecrement (--)",
        };
        FlowOrError::Error(PerlError::runtime(
            format!("Can't modify {agg} dereference in {op}"),
            line,
        ))
    }

    /// `$$r++` / `$$r--` — returns old value; shared by the VM.
    pub(crate) fn symbolic_scalar_ref_postfix(
        &mut self,
        ref_val: PerlValue,
        decrement: bool,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let old = self.symbolic_deref(ref_val.clone(), Sigil::Scalar, line)?;
        let new_val = PerlValue::integer(old.to_int() + if decrement { -1 } else { 1 });
        self.assign_scalar_ref_deref(ref_val, new_val, line)?;
        Ok(old)
    }

    /// `$$r = $val` — assign through a scalar reference (or special name ref); shared by
    /// [`Self::assign_value`] and the VM.
    pub(crate) fn assign_scalar_ref_deref(
        &mut self,
        ref_val: PerlValue,
        val: PerlValue,
        line: usize,
    ) -> ExecResult {
        if let Some(name) = ref_val.as_scalar_binding_name() {
            self.set_special_var(&name, &val)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            return Ok(PerlValue::UNDEF);
        }
        if let Some(r) = ref_val.as_scalar_ref() {
            *r.write() = val;
            return Ok(PerlValue::UNDEF);
        }
        Err(PerlError::runtime("Can't assign to non-scalar reference", line).into())
    }

    /// `@{ EXPR } = LIST` — array ref or package name string (mirrors [`Self::symbolic_deref`] for [`Sigil::Array`]).
    pub(crate) fn assign_symbolic_array_ref_deref(
        &mut self,
        ref_val: PerlValue,
        val: PerlValue,
        line: usize,
    ) -> ExecResult {
        if let Some(a) = ref_val.as_array_ref() {
            *a.write() = val.to_list();
            return Ok(PerlValue::UNDEF);
        }
        if let Some(name) = ref_val.as_array_binding_name() {
            self.scope
                .set_array(&name, val.to_list())
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            return Ok(PerlValue::UNDEF);
        }
        if let Some(s) = ref_val.as_str() {
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
            self.scope
                .set_array(&s, val.to_list())
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            return Ok(PerlValue::UNDEF);
        }
        Err(PerlError::runtime("Can't assign to non-array reference", line).into())
    }

    /// `*{ EXPR } = RHS` — symbolic glob name string (like `*{ $name } = …`); coderef via
    /// [`Self::assign_typeglob_value`] or glob-to-glob copy via [`Self::copy_typeglob_slots`].
    pub(crate) fn assign_symbolic_typeglob_ref_deref(
        &mut self,
        ref_val: PerlValue,
        val: PerlValue,
        line: usize,
    ) -> ExecResult {
        let lhs_name = if let Some(s) = ref_val.as_str() {
            if self.strict_refs {
                return Err(PerlError::runtime(
                    format!(
                        "Can't use string (\"{}\") as a symbol ref while \"strict refs\" in use",
                        s
                    ),
                    line,
                )
                .into());
            }
            s.to_string()
        } else {
            return Err(
                PerlError::runtime("Can't assign to non-glob symbolic reference", line).into(),
            );
        };
        let is_coderef = val.as_code_ref().is_some()
            || val
                .as_scalar_ref()
                .map(|r| r.read().as_code_ref().is_some())
                .unwrap_or(false);
        if is_coderef {
            return self.assign_typeglob_value(&lhs_name, val, line);
        }
        let rhs_key = val.to_string();
        self.copy_typeglob_slots(&lhs_name, &rhs_key, line)
            .map_err(FlowOrError::Error)?;
        Ok(PerlValue::UNDEF)
    }

    /// `%{ EXPR } = LIST` — hash ref or package name string (mirrors [`Self::symbolic_deref`] for [`Sigil::Hash`]).
    pub(crate) fn assign_symbolic_hash_ref_deref(
        &mut self,
        ref_val: PerlValue,
        val: PerlValue,
        line: usize,
    ) -> ExecResult {
        let items = val.to_list();
        let mut map = IndexMap::new();
        let mut i = 0;
        while i + 1 < items.len() {
            map.insert(items[i].to_string(), items[i + 1].clone());
            i += 2;
        }
        if let Some(h) = ref_val.as_hash_ref() {
            *h.write() = map;
            return Ok(PerlValue::UNDEF);
        }
        if let Some(name) = ref_val.as_hash_binding_name() {
            self.touch_env_hash(&name);
            self.scope
                .set_hash(&name, map)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            return Ok(PerlValue::UNDEF);
        }
        if let Some(s) = ref_val.as_str() {
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
            self.scope
                .set_hash(&s, map)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            return Ok(PerlValue::UNDEF);
        }
        Err(PerlError::runtime("Can't assign to non-hash reference", line).into())
    }

    /// `$href->{key} = $val` and blessed hash slots — shared by [`Self::assign_value`] and the VM.
    pub(crate) fn assign_arrow_hash_deref(
        &mut self,
        container: PerlValue,
        key: String,
        val: PerlValue,
        line: usize,
    ) -> ExecResult {
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
            return Err(PerlError::runtime("Can't assign into non-hash blessed ref", line).into());
        }
        if let Some(r) = container.as_hash_ref() {
            r.write().insert(key, val);
            return Ok(PerlValue::UNDEF);
        }
        if let Some(name) = container.as_hash_binding_name() {
            self.touch_env_hash(&name);
            self.scope
                .set_hash_element(&name, &key, val)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            return Ok(PerlValue::UNDEF);
        }
        Err(PerlError::runtime("Can't assign to arrow hash deref on non-hash(-ref)", line).into())
    }

    /// For `$aref->[ix]` / `@$r[ix]` arrow-array ops: the container must be the array **reference** (scalar),
    /// not `@{...}` / `@$r` expansion (which yields a plain array value).
    pub(crate) fn eval_arrow_array_base(
        &mut self,
        expr: &Expr,
        _line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        match &expr.kind {
            ExprKind::Deref {
                expr: inner,
                kind: Sigil::Array | Sigil::Scalar,
            } => self.eval_expr(inner),
            _ => self.eval_expr(expr),
        }
    }

    /// For `$href->{k}` / `$$r{k}`: container is the hashref scalar, not `%{ $r }` expansion.
    pub(crate) fn eval_arrow_hash_base(
        &mut self,
        expr: &Expr,
        _line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        match &expr.kind {
            ExprKind::Deref {
                expr: inner,
                kind: Sigil::Scalar,
            } => self.eval_expr(inner),
            _ => self.eval_expr(expr),
        }
    }

    /// Read `$aref->[$i]` — same indexing as the VM [`crate::bytecode::Op::ArrowArray`].
    pub(crate) fn read_arrow_array_element(
        &self,
        container: PerlValue,
        idx: i64,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if let Some(a) = container.as_array_ref() {
            let arr = a.read();
            let i = if idx < 0 {
                (arr.len() as i64 + idx) as usize
            } else {
                idx as usize
            };
            return Ok(arr.get(i).cloned().unwrap_or(PerlValue::UNDEF));
        }
        if let Some(name) = container.as_array_binding_name() {
            return Ok(self.scope.get_array_element(&name, idx));
        }
        if let Some(arr) = container.as_array_vec() {
            let i = if idx < 0 {
                (arr.len() as i64 + idx) as usize
            } else {
                idx as usize
            };
            return Ok(arr.get(i).cloned().unwrap_or(PerlValue::UNDEF));
        }
        // Blessed arrayref (e.g. `List::Util::_Pair`) — Perl allows `->[N]` on
        // blessed arrayrefs; `pairs` returns blessed `_Pair` objects that the
        // doc shows being indexed via `$_->[0]` / `$_->[1]`.
        if let Some(b) = container.as_blessed_ref() {
            let inner = b.data.read().clone();
            if let Some(a) = inner.as_array_ref() {
                let arr = a.read();
                let i = if idx < 0 {
                    (arr.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                return Ok(arr.get(i).cloned().unwrap_or(PerlValue::UNDEF));
            }
        }
        Err(PerlError::runtime("Can't use arrow deref on non-array-ref", line).into())
    }

    /// Read `$href->{key}` — same as the VM [`crate::bytecode::Op::ArrowHash`].
    pub(crate) fn read_arrow_hash_element(
        &mut self,
        container: PerlValue,
        key: &str,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if let Some(r) = container.as_hash_ref() {
            let h = r.read();
            return Ok(h.get(key).cloned().unwrap_or(PerlValue::UNDEF));
        }
        if let Some(name) = container.as_hash_binding_name() {
            self.touch_env_hash(&name);
            return Ok(self.scope.get_hash_element(&name, key));
        }
        if let Some(b) = container.as_blessed_ref() {
            let data = b.data.read();
            if let Some(v) = data.hash_get(key) {
                return Ok(v);
            }
            if let Some(r) = data.as_hash_ref() {
                let h = r.read();
                return Ok(h.get(key).cloned().unwrap_or(PerlValue::UNDEF));
            }
            return Err(PerlError::runtime(
                "Can't access hash field on non-hash blessed ref",
                line,
            )
            .into());
        }
        Err(PerlError::runtime("Can't use arrow deref on non-hash-ref", line).into())
    }

    /// `$aref->[$i]++` / `$aref->[$i]--` — returns old value; shared by the VM.
    pub(crate) fn arrow_array_postfix(
        &mut self,
        container: PerlValue,
        idx: i64,
        decrement: bool,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let old = self.read_arrow_array_element(container.clone(), idx, line)?;
        let new_val = PerlValue::integer(old.to_int() + if decrement { -1 } else { 1 });
        self.assign_arrow_array_deref(container, idx, new_val, line)?;
        Ok(old)
    }

    /// `$href->{k}++` / `$href->{k}--` — returns old value; shared by the VM.
    pub(crate) fn arrow_hash_postfix(
        &mut self,
        container: PerlValue,
        key: String,
        decrement: bool,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let old = self.read_arrow_hash_element(container.clone(), key.as_str(), line)?;
        let new_val = PerlValue::integer(old.to_int() + if decrement { -1 } else { 1 });
        self.assign_arrow_hash_deref(container, key, new_val, line)?;
        Ok(old)
    }

    /// `BAREWORD` as an rvalue — matches `ExprKind::Bareword` in the tree walker. If a nullary
    /// subroutine by that name is defined, call it; otherwise stringify (bareword-as-string).
    /// `strict subs` is enforced transitively: if the bareword is used where a sub is called
    /// explicitly (`&foo` / `foo()`) and the sub is undefined, `call_named_sub` emits the
    /// `strict subs` error — bare rvalue position is lenient (matches tree semantics, which
    /// diverges slightly from Perl 5's compile-time `Bareword "..." not allowed while "strict
    /// subs" in use`).
    pub(crate) fn resolve_bareword_rvalue(
        &mut self,
        name: &str,
        want: WantarrayCtx,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if name == "__PACKAGE__" {
            return Ok(PerlValue::string(self.current_package()));
        }
        if let Some(sub) = self.resolve_sub_by_name(name) {
            return self.call_sub(&sub, vec![], want, line);
        }
        Ok(PerlValue::string(name.to_string()))
    }

    /// `@$aref[i1,i2,...]` rvalue — read a slice through an array reference as a list.
    /// Shared by the VM [`crate::bytecode::Op::ArrowArraySlice`] path already, and by the new
    /// compound / inc-dec / assign helpers below.
    pub(crate) fn arrow_array_slice_values(
        &mut self,
        container: PerlValue,
        indices: &[i64],
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let mut out = Vec::with_capacity(indices.len());
        for &idx in indices {
            let v = self.read_arrow_array_element(container.clone(), idx, line)?;
            out.push(v);
        }
        Ok(PerlValue::array(out))
    }

    /// `@$aref[i1,i2,...] = LIST` — element-wise assignment matching the tree-walker
    /// `assign_value` path for multi-index `ArrowDeref { Array, List }`. Shared by the VM
    /// [`crate::bytecode::Op::SetArrowArraySlice`].
    pub(crate) fn assign_arrow_array_slice(
        &mut self,
        container: PerlValue,
        indices: Vec<i64>,
        val: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if indices.is_empty() {
            return Err(PerlError::runtime("assign to empty array slice", line).into());
        }
        let vals = val.to_list();
        for (i, idx) in indices.iter().enumerate() {
            let v = vals.get(i).cloned().unwrap_or(PerlValue::UNDEF);
            self.assign_arrow_array_deref(container.clone(), *idx, v, line)?;
        }
        Ok(PerlValue::UNDEF)
    }

    /// Flatten `@a[IX,...]` subscripts to integer indices (range / list specs expand like the VM).
    pub(crate) fn flatten_array_slice_index_specs(
        &mut self,
        indices: &[Expr],
    ) -> Result<Vec<i64>, FlowOrError> {
        let mut out = Vec::new();
        for idx_expr in indices {
            let v = if matches!(idx_expr.kind, ExprKind::Range { .. }) {
                self.eval_expr_ctx(idx_expr, WantarrayCtx::List)?
            } else {
                self.eval_expr(idx_expr)?
            };
            if let Some(list) = v.as_array_vec() {
                for idx in list {
                    out.push(idx.to_int());
                }
            } else {
                out.push(v.to_int());
            }
        }
        Ok(out)
    }

    /// `@name[i1,i2,...] = LIST` — element-wise assignment (VM [`crate::bytecode::Op::SetNamedArraySlice`]).
    pub(crate) fn assign_named_array_slice(
        &mut self,
        stash_array_name: &str,
        indices: Vec<i64>,
        val: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if indices.is_empty() {
            return Err(PerlError::runtime("assign to empty array slice", line).into());
        }
        let vals = val.to_list();
        for (i, idx) in indices.iter().enumerate() {
            let v = vals.get(i).cloned().unwrap_or(PerlValue::UNDEF);
            self.scope
                .set_array_element(stash_array_name, *idx, v)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
        }
        Ok(PerlValue::UNDEF)
    }

    /// `@$aref[i1,i2,...] OP= rhs` — Perl 5 applies the compound op only to the **last** index.
    /// Shared by VM [`crate::bytecode::Op::ArrowArraySliceCompound`].
    pub(crate) fn compound_assign_arrow_array_slice(
        &mut self,
        container: PerlValue,
        indices: Vec<i64>,
        op: BinOp,
        rhs: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if indices.is_empty() {
            return Err(PerlError::runtime("assign to empty array slice", line).into());
        }
        let last_idx = *indices.last().expect("non-empty indices");
        let last_old = self.read_arrow_array_element(container.clone(), last_idx, line)?;
        let new_val = self.eval_binop(op, &last_old, &rhs, line)?;
        self.assign_arrow_array_deref(container, last_idx, new_val.clone(), line)?;
        Ok(new_val)
    }

    /// `++@$aref[i1,i2,...]` / `--...` / `...++` / `...--` — Perl updates only the **last** index;
    /// pre forms return the new value, post forms return the old **last** element.
    /// `kind` byte: 0=PreInc, 1=PreDec, 2=PostInc, 3=PostDec.
    /// Shared by VM [`crate::bytecode::Op::ArrowArraySliceIncDec`].
    pub(crate) fn arrow_array_slice_inc_dec(
        &mut self,
        container: PerlValue,
        indices: Vec<i64>,
        kind: u8,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if indices.is_empty() {
            return Err(
                PerlError::runtime("array slice increment needs at least one index", line).into(),
            );
        }
        let last_idx = *indices.last().expect("non-empty indices");
        let last_old = self.read_arrow_array_element(container.clone(), last_idx, line)?;
        let new_val = if kind & 1 == 0 {
            PerlValue::integer(last_old.to_int() + 1)
        } else {
            PerlValue::integer(last_old.to_int() - 1)
        };
        self.assign_arrow_array_deref(container, last_idx, new_val.clone(), line)?;
        Ok(if kind < 2 { new_val } else { last_old })
    }

    /// `++@name[i1,i2,...]` / `--...` / `...++` / `...--` on a stash-qualified array name.
    /// Same semantics as [`Self::arrow_array_slice_inc_dec`] (only the **last** index is updated).
    pub(crate) fn named_array_slice_inc_dec(
        &mut self,
        stash_array_name: &str,
        indices: Vec<i64>,
        kind: u8,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let last_idx = *indices.last().ok_or_else(|| {
            PerlError::runtime("array slice increment needs at least one index", line)
        })?;
        let last_old = self.scope.get_array_element(stash_array_name, last_idx);
        let new_val = if kind & 1 == 0 {
            PerlValue::integer(last_old.to_int() + 1)
        } else {
            PerlValue::integer(last_old.to_int() - 1)
        };
        self.scope
            .set_array_element(stash_array_name, last_idx, new_val.clone())
            .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
        Ok(if kind < 2 { new_val } else { last_old })
    }

    /// `@name[i1,i2,...] OP= rhs` — only the **last** index is updated (VM [`crate::bytecode::Op::NamedArraySliceCompound`]).
    pub(crate) fn compound_assign_named_array_slice(
        &mut self,
        stash_array_name: &str,
        indices: Vec<i64>,
        op: BinOp,
        rhs: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if indices.is_empty() {
            return Err(PerlError::runtime("assign to empty array slice", line).into());
        }
        let last_idx = *indices.last().expect("non-empty indices");
        let last_old = self.scope.get_array_element(stash_array_name, last_idx);
        let new_val = self.eval_binop(op, &last_old, &rhs, line)?;
        self.scope
            .set_array_element(stash_array_name, last_idx, new_val.clone())
            .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
        Ok(new_val)
    }

    /// `$aref->[$i] = $val` — shared by [`Self::assign_value`] and the VM.
    pub(crate) fn assign_arrow_array_deref(
        &mut self,
        container: PerlValue,
        idx: i64,
        val: PerlValue,
        line: usize,
    ) -> ExecResult {
        if let Some(a) = container.as_array_ref() {
            let mut arr = a.write();
            let i = if idx < 0 {
                (arr.len() as i64 + idx) as usize
            } else {
                idx as usize
            };
            if i >= arr.len() {
                arr.resize(i + 1, PerlValue::UNDEF);
            }
            arr[i] = val;
            return Ok(PerlValue::UNDEF);
        }
        if let Some(name) = container.as_array_binding_name() {
            self.scope
                .set_array_element(&name, idx, val)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            return Ok(PerlValue::UNDEF);
        }
        Err(PerlError::runtime("Can't assign to arrow array deref on non-array-ref", line).into())
    }

    /// `*name = $coderef` — install subroutine alias (tree [`assign_value`] and VM [`crate::bytecode::Op::TypeglobAssignFromValue`]).
    pub(crate) fn assign_typeglob_value(
        &mut self,
        name: &str,
        val: PerlValue,
        line: usize,
    ) -> ExecResult {
        let sub = if let Some(c) = val.as_code_ref() {
            Some(c)
        } else if let Some(r) = val.as_scalar_ref() {
            r.read().as_code_ref().map(|c| Arc::clone(&c))
        } else {
            None
        };
        if let Some(sub) = sub {
            let lhs_sub = self.qualify_typeglob_sub_key(name);
            self.subs.insert(lhs_sub, sub);
            return Ok(PerlValue::UNDEF);
        }
        Err(PerlError::runtime(
            "typeglob assignment requires a subroutine reference (e.g. *foo = \\&bar) or another typeglob (*foo = *bar)",
            line,
        )
        .into())
    }

    fn assign_value(&mut self, target: &Expr, val: PerlValue) -> ExecResult {
        match &target.kind {
            ExprKind::ScalarVar(name) => {
                let stor = self.tree_scalar_storage_name(name);
                if self.scope.is_scalar_frozen(&stor) {
                    return Err(FlowOrError::Error(PerlError::runtime(
                        format!("Modification of a frozen value: ${}", name),
                        target.line,
                    )));
                }
                if let Some(obj) = self.tied_scalars.get(&stor).cloned() {
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
                self.set_special_var(&stor, &val)
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
            ExprKind::ArraySlice { array, indices } => {
                if indices.is_empty() {
                    return Err(
                        PerlError::runtime("assign to empty array slice", target.line).into(),
                    );
                }
                self.check_strict_array_var(array, target.line)?;
                if self.scope.is_array_frozen(array) {
                    return Err(PerlError::runtime(
                        format!("Modification of a frozen value: @{}", array),
                        target.line,
                    )
                    .into());
                }
                let aname = self.stash_array_name_for_package(array);
                let flat = self.flatten_array_slice_index_specs(indices)?;
                self.assign_named_array_slice(&aname, flat, val, target.line)
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
            ExprKind::HashSlice { hash, keys } => {
                if keys.is_empty() {
                    return Err(
                        PerlError::runtime("assign to empty hash slice", target.line).into(),
                    );
                }
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
                let mut key_vals = Vec::with_capacity(keys.len());
                for key_expr in keys {
                    let v = if matches!(key_expr.kind, ExprKind::Range { .. }) {
                        self.eval_expr_ctx(key_expr, WantarrayCtx::List)?
                    } else {
                        self.eval_expr(key_expr)?
                    };
                    key_vals.push(v);
                }
                self.assign_named_hash_slice(hash, key_vals, val, target.line)
            }
            ExprKind::Typeglob(name) => self.assign_typeglob_value(name, val, target.line),
            ExprKind::TypeglobExpr(e) => {
                let name = self.eval_expr(e)?.to_string();
                let synthetic = Expr {
                    kind: ExprKind::Typeglob(name),
                    line: target.line,
                };
                self.assign_value(&synthetic, val)
            }
            ExprKind::AnonymousListSlice { source, indices } => {
                if let ExprKind::Deref {
                    expr: inner,
                    kind: Sigil::Array,
                } = &source.kind
                {
                    let container = self.eval_arrow_array_base(inner, target.line)?;
                    let vals = val.to_list();
                    let n = indices.len().min(vals.len());
                    for i in 0..n {
                        let idx = self.eval_expr(&indices[i])?.to_int();
                        self.assign_arrow_array_deref(
                            container.clone(),
                            idx,
                            vals[i].clone(),
                            target.line,
                        )?;
                    }
                    return Ok(PerlValue::UNDEF);
                }
                Err(
                    PerlError::runtime("assign to list slice: unsupported base", target.line)
                        .into(),
                )
            }
            ExprKind::ArrowDeref {
                expr,
                index,
                kind: DerefKind::Hash,
            } => {
                let key = self.eval_expr(index)?.to_string();
                let container = self.eval_expr(expr)?;
                self.assign_arrow_hash_deref(container, key, val, target.line)
            }
            ExprKind::ArrowDeref {
                expr,
                index,
                kind: DerefKind::Array,
            } => {
                let container = self.eval_arrow_array_base(expr, target.line)?;
                if let ExprKind::List(indices) = &index.kind {
                    let vals = val.to_list();
                    let n = indices.len().min(vals.len());
                    for i in 0..n {
                        let idx = self.eval_expr(&indices[i])?.to_int();
                        self.assign_arrow_array_deref(
                            container.clone(),
                            idx,
                            vals[i].clone(),
                            target.line,
                        )?;
                    }
                    return Ok(PerlValue::UNDEF);
                }
                let idx = self.eval_expr(index)?.to_int();
                self.assign_arrow_array_deref(container, idx, val, target.line)
            }
            ExprKind::HashSliceDeref { container, keys } => {
                let href = self.eval_expr(container)?;
                let mut key_vals = Vec::with_capacity(keys.len());
                for key_expr in keys {
                    key_vals.push(self.eval_expr(key_expr)?);
                }
                self.assign_hash_slice_deref(href, key_vals, val, target.line)
            }
            ExprKind::Deref {
                expr,
                kind: Sigil::Scalar,
            } => {
                let ref_val = self.eval_expr(expr)?;
                self.assign_scalar_ref_deref(ref_val, val, target.line)
            }
            ExprKind::Deref {
                expr,
                kind: Sigil::Array,
            } => {
                let ref_val = self.eval_expr(expr)?;
                self.assign_symbolic_array_ref_deref(ref_val, val, target.line)
            }
            ExprKind::Deref {
                expr,
                kind: Sigil::Hash,
            } => {
                let ref_val = self.eval_expr(expr)?;
                self.assign_symbolic_hash_ref_deref(ref_val, val, target.line)
            }
            ExprKind::Deref {
                expr,
                kind: Sigil::Typeglob,
            } => {
                let ref_val = self.eval_expr(expr)?;
                self.assign_symbolic_typeglob_ref_deref(ref_val, val, target.line)
            }
            ExprKind::Pos(inner) => {
                let key = match inner {
                    None => "_".to_string(),
                    Some(expr) => match &expr.kind {
                        ExprKind::ScalarVar(n) => n.clone(),
                        _ => self.eval_expr(expr)?.to_string(),
                    },
                };
                if val.is_undef() {
                    self.regex_pos.insert(key, None);
                } else {
                    let u = val.to_int().max(0) as usize;
                    self.regex_pos.insert(key, Some(u));
                }
                Ok(PerlValue::UNDEF)
            }
            // `($f = EXPR) =~ s///` — assignment returns the target as an lvalue;
            // write the substitution result back to the assignment target.
            ExprKind::Assign { target, .. } => self.assign_value(target, val),
            _ => Ok(PerlValue::UNDEF),
        }
    }

    /// True when [`get_special_var`] must run instead of [`Scope::get_scalar`].
    pub(crate) fn is_special_scalar_name_for_get(name: &str) -> bool {
        (name.starts_with('#') && name.len() > 1)
            || name.starts_with('^')
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
            || crate::english::is_known_alias(name)
    }

    /// Map English long names (`ARG` → [`crate::english::scalar_alias`]) when [`Self::english_enabled`],
    /// except for names registered in [`Self::english_lexical_scalars`] (lexical `my`/`our`/…).
    /// Match aliases (`MATCH`/`PREMATCH`/`POSTMATCH`) are suppressed when
    /// [`Self::english_no_match_vars`] is set.
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
        if let Some(short) = crate::english::scalar_alias(name, self.english_no_match_vars) {
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
                    | "."
            )
            || crate::english::is_known_alias(name)
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
            "/" => match &self.irs {
                Some(s) => PerlValue::string(s.clone()),
                None => PerlValue::UNDEF,
            },
            "\\" => PerlValue::string(self.ors.clone()),
            "," => PerlValue::string(self.ofs.clone()),
            "." => {
                // Perl: `$.` is undefined until a line is read (or `-n`/`-p` advances `line_number`).
                if self.last_readline_handle.is_empty() {
                    if self.line_number == 0 {
                        PerlValue::UNDEF
                    } else {
                        PerlValue::integer(self.line_number)
                    }
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
            "^OPEN" => PerlValue::integer(if self.open_pragma_utf8 { 1 } else { 0 }),
            "^UTF8LOCALE" => PerlValue::integer(0),
            "^UTF8CACHE" => PerlValue::integer(-1),
            _ if name.starts_with('^') && name.len() > 1 => self
                .special_caret_scalars
                .get(name)
                .cloned()
                .unwrap_or(PerlValue::UNDEF),
            _ if name.starts_with('#') && name.len() > 1 => {
                let arr = &name[1..];
                let aname = self.stash_array_name_for_package(arr);
                let len = self.scope.array_len(&aname);
                PerlValue::integer(len as i64 - 1)
            }
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
            "." => {
                // perlvar: assigning to `$.` sets the line number for the last-read filehandle,
                // or the global counter when no handle has been read yet (`-n`/`-p` / pre-read).
                let n = val.to_int();
                if self.last_readline_handle.is_empty() {
                    self.line_number = n;
                } else {
                    self.handle_line_numbers
                        .insert(self.last_readline_handle.clone(), n);
                }
            }
            "0" => self.program_name = val.to_string(),
            "/" => {
                self.irs = if val.is_undef() {
                    None
                } else {
                    Some(val.to_string())
                }
            }
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

    /// `pop (expr)` / `scalar @arr` / one-element list — peel to the real array operand.
    fn peel_array_builtin_operand(expr: &Expr) -> &Expr {
        match &expr.kind {
            ExprKind::ScalarContext(inner) => Self::peel_array_builtin_operand(inner),
            ExprKind::List(es) if es.len() == 1 => Self::peel_array_builtin_operand(&es[0]),
            _ => expr,
        }
    }

    /// `@$aref` / `@{...}` after optional peeling — for tree `SpliceExpr` / `pop` fallbacks.
    fn try_eval_array_deref_container(
        &mut self,
        expr: &Expr,
    ) -> Result<Option<PerlValue>, FlowOrError> {
        let e = Self::peel_array_builtin_operand(expr);
        if let ExprKind::Deref {
            expr: inner,
            kind: Sigil::Array,
        } = &e.kind
        {
            return Ok(Some(self.eval_expr(inner)?));
        }
        Ok(None)
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

    /// `Foo->VERSION` / `$blessed->VERSION` — read `$VERSION` with `__PACKAGE__` set to the invocant
    /// package (our `$VERSION` is not stored under `Foo::VERSION` keys yet).
    pub(crate) fn package_version_scalar(
        &mut self,
        package: &str,
    ) -> PerlResult<Option<PerlValue>> {
        let saved_pkg = self.scope.get_scalar("__PACKAGE__");
        let _ = self
            .scope
            .set_scalar("__PACKAGE__", PerlValue::string(package.to_string()));
        let ver = self.get_special_var("VERSION");
        let _ = self.scope.set_scalar("__PACKAGE__", saved_pkg);
        Ok(if ver.is_undef() { None } else { Some(ver) })
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

    /// `$coderef(...)` / `&$name(...)` / `&$cr` with caller `@_` — shared by tree [`ExprKind::IndirectCall`]
    /// and [`crate::bytecode::Op::IndirectCall`].
    pub(crate) fn dispatch_indirect_call(
        &mut self,
        target: PerlValue,
        arg_vals: Vec<PerlValue>,
        want: WantarrayCtx,
        line: usize,
    ) -> ExecResult {
        if let Some(sub) = target.as_code_ref() {
            return self.call_sub(&sub, arg_vals, want, line);
        }
        if let Some(name) = target.as_str() {
            return self.call_named_sub(&name, arg_vals, line, want);
        }
        Err(PerlError::runtime("Can't use non-code reference as a subroutine", line).into())
    }

    /// Bare `uniq` / `distinct` (alias of `uniq`) / `shuffle` / `chunked` / `windowed` / `zip` /
    /// `sum` / `sum0` /
    /// `product` / `min` / `max` / `mean` / `median` / `mode` / `stddev` / `variance` /
    /// `any` / `all` / `none` / `first` (Ruby `detect` / `find` parse to `first`; same as `List::Util` after
    /// [`crate::list_util::ensure_list_util`]).
    pub(crate) fn call_bare_list_util(
        &mut self,
        name: &str,
        args: Vec<PerlValue>,
        line: usize,
        want: WantarrayCtx,
    ) -> ExecResult {
        crate::list_util::ensure_list_util(self);
        let fq = match name {
            "uniq" | "distinct" | "uq" => "List::Util::uniq",
            "uniqstr" => "List::Util::uniqstr",
            "uniqint" => "List::Util::uniqint",
            "uniqnum" => "List::Util::uniqnum",
            "shuffle" | "shuf" => "List::Util::shuffle",
            "sample" => "List::Util::sample",
            "chunked" | "chk" => "List::Util::chunked",
            "windowed" | "win" => "List::Util::windowed",
            "zip" | "zp" => "List::Util::zip",
            "zip_longest" => "List::Util::zip_longest",
            "zip_shortest" => "List::Util::zip_shortest",
            "mesh" => "List::Util::mesh",
            "mesh_longest" => "List::Util::mesh_longest",
            "mesh_shortest" => "List::Util::mesh_shortest",
            "any" => "List::Util::any",
            "all" => "List::Util::all",
            "none" => "List::Util::none",
            "notall" => "List::Util::notall",
            "first" | "fst" => "List::Util::first",
            "reduce" | "rd" => "List::Util::reduce",
            "reductions" => "List::Util::reductions",
            "sum" => "List::Util::sum",
            "sum0" => "List::Util::sum0",
            "product" => "List::Util::product",
            "min" => "List::Util::min",
            "max" => "List::Util::max",
            "minstr" => "List::Util::minstr",
            "maxstr" => "List::Util::maxstr",
            "mean" => "List::Util::mean",
            "median" | "med" => "List::Util::median",
            "mode" => "List::Util::mode",
            "stddev" | "std" => "List::Util::stddev",
            "variance" | "var" => "List::Util::variance",
            "pairs" => "List::Util::pairs",
            "unpairs" => "List::Util::unpairs",
            "pairkeys" => "List::Util::pairkeys",
            "pairvalues" => "List::Util::pairvalues",
            "pairgrep" => "List::Util::pairgrep",
            "pairmap" => "List::Util::pairmap",
            "pairfirst" => "List::Util::pairfirst",
            _ => {
                return Err(PerlError::runtime(
                    format!("internal: not a bare list-util alias: {name}"),
                    line,
                )
                .into());
            }
        };
        let Some(sub) = self.subs.get(fq).cloned() else {
            return Err(PerlError::runtime(
                format!("internal: missing native stub for {fq}"),
                line,
            )
            .into());
        };
        let args = self.with_topic_default_args(args);
        self.call_sub(&sub, args, want, line)
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
            "uniq" | "distinct" | "uq" | "uniqstr" | "uniqint" | "uniqnum" | "shuffle" | "shuf"
            | "sample" | "chunked" | "chk" | "windowed" | "win" | "zip" | "zp" | "zip_shortest"
            | "zip_longest" | "mesh" | "mesh_shortest" | "mesh_longest" | "any" | "all"
            | "none" | "notall" | "first" | "fst" | "reduce" | "rd" | "reductions" | "sum"
            | "sum0" | "product" | "min" | "max" | "minstr" | "maxstr" | "mean" | "median"
            | "med" | "mode" | "stddev" | "std" | "variance" | "var" | "pairs" | "unpairs"
            | "pairkeys" | "pairvalues" | "pairgrep" | "pairmap" | "pairfirst" => {
                self.call_bare_list_util(name, args, line, want)
            }
            "deque" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("deque() takes no arguments", line).into());
                }
                Ok(PerlValue::deque(Arc::new(Mutex::new(VecDeque::new()))))
            }
            "defer__internal" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "defer__internal expects one coderef argument",
                        line,
                    )
                    .into());
                }
                self.scope.push_defer(args[0].clone());
                Ok(PerlValue::UNDEF)
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
            "cluster" => {
                let items = if args.len() == 1 {
                    args[0].to_list()
                } else {
                    args.to_vec()
                };
                let c = RemoteCluster::from_list_args(&items)
                    .map_err(|msg| PerlError::runtime(msg, line))?;
                Ok(PerlValue::remote_cluster(Arc::new(c)))
            }
            _ => {
                let args = self.with_topic_default_args(args);
                if let Some(r) = self.try_autoload_call(name, args, line, want, None) {
                    return r;
                }
                Err(PerlError::runtime(self.undefined_subroutine_call_message(name), line).into())
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

        self.write_formatted_print(handle_name, &output, line)?;
        Ok(PerlValue::integer(1))
    }

    /// Write a fully formatted `print`/`say` record (`LIST`, optional `say` newline, `$\`) to a handle.
    /// `handle_name` must already be [`Self::resolve_io_handle_name`]-resolved.
    pub(crate) fn write_formatted_print(
        &mut self,
        handle_name: &str,
        output: &str,
        line: usize,
    ) -> PerlResult<()> {
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
        Ok(())
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
        if let Some(s) = crate::value::set_payload(receiver) {
            return Some(self.set_method(s, method, args, line));
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
        if let Some(g) = receiver.as_generator() {
            if method == "next" {
                if !args.is_empty() {
                    return Some(Err(PerlError::runtime(
                        "generator->next takes no arguments",
                        line,
                    )));
                }
                return Some(self.generator_next(&g));
            }
            return None;
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
                    self.scope.set_topic(row);
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

    /// Native `Set` values (`set(LIST)`, `Set->new`, `$a | $b`): membership and views (immutable).
    fn set_method(
        &self,
        s: Arc<crate::value::PerlSet>,
        method: &str,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        match method {
            "has" | "contains" | "member" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "set->has expects one argument (element)",
                        line,
                    ));
                }
                let k = crate::value::set_member_key(&args[0]);
                Ok(PerlValue::integer(if s.contains_key(&k) { 1 } else { 0 }))
            }
            "size" | "len" | "count" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("set->size takes no arguments", line));
                }
                Ok(PerlValue::integer(s.len() as i64))
            }
            "values" | "list" | "elements" => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("set->values takes no arguments", line));
                }
                Ok(PerlValue::array(s.values().cloned().collect()))
            }
            _ => Err(PerlError::runtime(
                format!("Unknown method for set: {}", method),
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

    /// `sub { $_ * k }` used when a map stage is lowered to [`crate::bytecode::Op::MapIntMul`].
    pub(crate) fn pipeline_int_mul_sub(k: i64) -> Arc<PerlSub> {
        let line = 1usize;
        let body = vec![Statement {
            label: None,
            kind: StmtKind::Expression(Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line,
                    }),
                    op: BinOp::Mul,
                    right: Box::new(Expr {
                        kind: ExprKind::Integer(k),
                        line,
                    }),
                },
                line,
            }),
            line,
        }];
        Arc::new(PerlSub {
            name: "__pipeline_int_mul__".into(),
            params: vec![],
            body,
            closure_env: None,
            prototype: None,
            fib_like: None,
        })
    }

    pub(crate) fn anon_coderef_from_block(&self, block: &Block) -> Arc<PerlSub> {
        let captured = self.scope.capture();
        Arc::new(PerlSub {
            name: "__ANON__".into(),
            params: vec![],
            body: block.clone(),
            closure_env: Some(captured),
            prototype: None,
            fib_like: None,
        })
    }

    pub(crate) fn builtin_collect_execute(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        if args.is_empty() {
            return Err(PerlError::runtime(
                "collect() expects at least one argument",
                line,
            ));
        }
        // `Op::Call` uses `pop_call_operands_flattened`: a single array actual becomes
        // many operands. Treat multi-arg as one materialized list (eager `|> … |> collect()`).
        if args.len() == 1 {
            if let Some(p) = args[0].as_pipeline() {
                return self.pipeline_collect(&p, line);
            }
            return Ok(PerlValue::array(args[0].to_list()));
        }
        Ok(PerlValue::array(args.to_vec()))
    }

    pub(crate) fn pipeline_push(
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

    pub(crate) fn pipeline_method(
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
            "tap" | "peek" => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "pipeline tap/peek expects 1 argument (sub)",
                        line,
                    ));
                }
                let Some(sub) = args[0].as_code_ref() else {
                    return Err(PerlError::runtime(
                        "pipeline tap/peek expects a code reference",
                        line,
                    ));
                };
                self.pipeline_push(&p, PipelineOp::Tap(sub), line)?;
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
                local_interp.scope.set_topic(item);
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
                local_interp.scope.set_topic(item.clone());
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
                local_interp.scope.set_topic(item);
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
                            self.scope.set_topic(item.clone());
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
                            self.scope.set_topic(item);
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
                PipelineOp::Tap(sub) => {
                    match self.call_sub(&sub, v.clone(), WantarrayCtx::Void, line) {
                        Ok(_) => {}
                        Err(FlowOrError::Error(e)) => return Err(e),
                        Err(FlowOrError::Flow(_)) => {
                            return Err(PerlError::runtime(
                                "tap: unsupported control flow in block",
                                line,
                            ));
                        }
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
                            local_interp.scope.set_topic(item.clone());
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
                        local_interp.scope.set_topic(item);
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
                                local_interp.scope.set_topic(item);
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
                                    let _ = local_interp.scope.set_scalar("_0", a.clone());
                                    let _ = local_interp.scope.set_scalar("_1", b.clone());
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
                            local_interp.scope.set_topic(item.clone());
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
                            let _ = local_interp.scope.set_scalar("a", a.clone());
                            let _ = local_interp.scope.set_scalar("b", b.clone());
                            let _ = local_interp.scope.set_scalar("_0", a);
                            let _ = local_interp.scope.set_scalar("_1", b);
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
                        local_interp.scope.set_topic(v[0].clone());
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
                            local_interp.scope.set_topic(item);
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
                            let _ = local_interp.scope.set_scalar("a", a.clone());
                            let _ = local_interp.scope.set_scalar("b", b.clone());
                            let _ = local_interp.scope.set_scalar("_0", a);
                            let _ = local_interp.scope.set_scalar("_1", b);
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
                                    interp.scope.set_topic(item.clone());
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
                                    interp.scope.set_topic(item);
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
                                    interp.scope.set_topic(item.clone());
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
                        PipelineOp::Tap(ref sub) => {
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
                                    match interp.call_sub(
                                        &sub,
                                        vec![item.clone()],
                                        WantarrayCtx::Void,
                                        line,
                                    )
                                    {
                                        Ok(_) => {}
                                        Err(e) => {
                                            let msg = match e {
                                                FlowOrError::Error(pe) => pe.to_string(),
                                                FlowOrError::Flow(_) => {
                                                    "unexpected control flow in par_pipeline_stream tap"
                                                        .into()
                                                }
                                            };
                                            let mut g = err_w.lock();
                                            if g.is_none() {
                                                *g = Some(msg);
                                            }
                                            break;
                                        }
                                    }
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
                                        interp.scope.set_topic(item);
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
        let _ = self.scope.set_scalar("_0", a.clone());
        let _ = self.scope.set_scalar("_1", b.clone());
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

    fn hash_for_signature_destruct(
        &mut self,
        v: &PerlValue,
        line: usize,
    ) -> PerlResult<IndexMap<String, PerlValue>> {
        let Some(m) = self.match_subject_as_hash(v) else {
            return Err(PerlError::runtime(
                format!(
                    "sub signature hash destruct: expected HASH or HASH reference, got {}",
                    v.ref_type()
                ),
                line,
            ));
        };
        Ok(m)
    }

    /// Bind perlrs `sub name ($a, { k => $v })` parameters from `@_` before the body runs.
    pub(crate) fn apply_sub_signature(
        &mut self,
        sub: &PerlSub,
        argv: &[PerlValue],
        line: usize,
    ) -> PerlResult<()> {
        if sub.params.is_empty() {
            return Ok(());
        }
        let mut i = 0usize;
        for p in &sub.params {
            match p {
                SubSigParam::Scalar(name, ty) => {
                    let val = argv.get(i).cloned().unwrap_or(PerlValue::UNDEF);
                    i += 1;
                    if let Some(t) = ty {
                        if let Err(e) = t.check_value(&val) {
                            return Err(PerlError::runtime(
                                format!("sub parameter ${}: {}", name, e),
                                line,
                            ));
                        }
                    }
                    let n = self.english_scalar_name(name);
                    self.scope.declare_scalar(n, val);
                }
                SubSigParam::ArrayDestruct(elems) => {
                    let arg = argv.get(i).cloned().unwrap_or(PerlValue::UNDEF);
                    i += 1;
                    let Some(arr) = self.match_subject_as_array(&arg) else {
                        return Err(PerlError::runtime(
                            format!(
                                "sub signature array destruct: expected ARRAY or ARRAY reference, got {}",
                                arg.ref_type()
                            ),
                            line,
                        ));
                    };
                    let binds = self
                        .match_array_pattern_elems(&arr, elems, line)
                        .map_err(|e| match e {
                            FlowOrError::Error(pe) => pe,
                            FlowOrError::Flow(_) => PerlError::runtime(
                                "unexpected flow in sub signature array destruct",
                                line,
                            ),
                        })?;
                    let Some(binds) = binds else {
                        return Err(PerlError::runtime(
                            "sub signature array destruct: length or element mismatch",
                            line,
                        ));
                    };
                    for b in binds {
                        match b {
                            PatternBinding::Scalar(name, v) => {
                                let n = self.english_scalar_name(&name);
                                self.scope.declare_scalar(n, v);
                            }
                            PatternBinding::Array(name, elems) => {
                                self.scope.declare_array(&name, elems);
                            }
                        }
                    }
                }
                SubSigParam::HashDestruct(pairs) => {
                    let arg = argv.get(i).cloned().unwrap_or(PerlValue::UNDEF);
                    i += 1;
                    let map = self.hash_for_signature_destruct(&arg, line)?;
                    for (key, varname) in pairs {
                        let v = map.get(key).cloned().unwrap_or(PerlValue::UNDEF);
                        let n = self.english_scalar_name(varname);
                        self.scope.declare_scalar(n, v);
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn call_sub(
        &mut self,
        sub: &PerlSub,
        args: Vec<PerlValue>,
        want: WantarrayCtx,
        _line: usize,
    ) -> ExecResult {
        // Push current sub for __SUB__ access
        self.current_sub_stack.push(Arc::new(sub.clone()));

        // Single frame for both @_ and the block's local variables —
        // avoids the double push_frame/pop_frame overhead per call.
        self.scope_push_hook();
        self.scope.declare_array("_", args.clone());
        if let Some(ref env) = sub.closure_env {
            self.scope.restore_capture(env);
        }
        // Set $_0, $_1, $_2, ... for all args, and $_ to first arg
        // so `>{ $_ + 1 }` works instead of requiring `>{ $_[0] + 1 }`
        // Must be AFTER restore_capture so we don't get shadowed by captured $_
        self.scope.set_closure_args(&args);
        // Move `@_` out so `native_dispatch` / `fib_like` take `&[PerlValue]` without `get_array` cloning.
        let argv = self.scope.take_sub_underscore().unwrap_or_default();
        self.apply_sub_signature(sub, &argv, _line)?;
        let saved = self.wantarray_kind;
        self.wantarray_kind = want;
        if let Some(r) = crate::list_util::native_dispatch(self, sub, &argv, want) {
            self.wantarray_kind = saved;
            self.scope_pop_hook();
            self.current_sub_stack.pop();
            return match r {
                Ok(v) => Ok(v),
                Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
                Err(e) => Err(e),
            };
        }
        if let Some(pat) = sub.fib_like.as_ref() {
            if argv.len() == 1 {
                if let Some(n0) = argv.first().and_then(|v| v.as_integer()) {
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
                    self.current_sub_stack.pop();
                    return Ok(PerlValue::integer(n));
                }
            }
        }
        self.scope.declare_array("_", argv.clone());
        // Note: set_closure_args was already called at line 15077; don't call it again
        // as that would incorrectly shift the outer topic stack a second time.
        let t0 = self.profiler.is_some().then(std::time::Instant::now);
        if let Some(p) = &mut self.profiler {
            p.enter_sub(&sub.name);
        }
        let result = self.exec_block_no_scope(&sub.body);
        if let (Some(p), Some(t0)) = (&mut self.profiler, t0) {
            p.exit_sub(t0.elapsed());
        }
        // For goto &sub, capture @_ before popping the frame
        let goto_args = if matches!(result, Err(FlowOrError::Flow(Flow::GotoSub(_)))) {
            Some(self.scope.get_array("_"))
        } else {
            None
        };
        self.wantarray_kind = saved;
        self.scope_pop_hook();
        self.current_sub_stack.pop();
        match result {
            Ok(v) => Ok(v),
            Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
            Err(FlowOrError::Flow(Flow::GotoSub(target_name))) => {
                // goto &sub — tail call: look up target and call with same @_
                let goto_args = goto_args.unwrap_or_default();
                let fqn = if target_name.contains("::") {
                    target_name.clone()
                } else {
                    format!("{}::{}", self.current_package(), target_name)
                };
                if let Some(target_sub) = self
                    .subs
                    .get(&fqn)
                    .cloned()
                    .or_else(|| self.subs.get(&target_name).cloned())
                {
                    self.call_sub(&target_sub, goto_args, want, _line)
                } else {
                    Err(
                        PerlError::runtime(format!("Undefined subroutine &{}", target_name), _line)
                            .into(),
                    )
                }
            }
            Err(FlowOrError::Flow(Flow::Yield(_))) => {
                Err(PerlError::runtime("yield is only valid inside gen { }", 0).into())
            }
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
        Ok(PerlValue::blessed(Arc::new(
            crate::value::BlessedRef::new_blessed(class.to_string(), PerlValue::hash(map)),
        )))
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
            // Perl: each comma-separated EXPR is evaluated in list context; `$ofs` is inserted
            // between those top-level expressions only (not between elements of an expanded `@arr`).
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    output.push_str(&self.ofs);
                }
                let val = self.eval_expr_ctx(a, WantarrayCtx::List)?;
                for item in val.to_list() {
                    let s = self.stringify_value(item, line)?;
                    output.push_str(&s);
                }
            }
        }
        if newline {
            output.push('\n');
        }
        output.push_str(&self.ors);

        let handle_name =
            self.resolve_io_handle_name(handle.unwrap_or(self.default_print_handle.as_str()));
        self.write_formatted_print(handle_name.as_str(), &output, line)?;
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
        // printf arg list after the format is Perl list context — `1..5`, `@arr`, `reverse`,
        // `grep`, etc. flatten into the format argument sequence. Scalar context collapses
        // ranges to flip-flop values, so go through list-context eval and splat.
        let mut arg_vals = Vec::new();
        for a in rest {
            let v = self.eval_expr_ctx(a, WantarrayCtx::List)?;
            if let Some(items) = v.as_array_vec() {
                arg_vals.extend(items);
            } else {
                arg_vals.push(v);
            }
        }
        let output = self.perl_sprintf_stringify(&fmt, &arg_vals, line)?;
        let handle_name =
            self.resolve_io_handle_name(handle.unwrap_or(self.default_print_handle.as_str()));
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
        if let Some(aref) = self.try_eval_array_deref_container(array)? {
            for v in values {
                let val = self.eval_expr_ctx(v, WantarrayCtx::List)?;
                self.push_array_deref_value(aref.clone(), val, line)?;
            }
            let len = self.array_deref_len(aref, line)?;
            return Ok(PerlValue::integer(len));
        }
        let arr_name = self.extract_array_name(Self::peel_array_builtin_operand(array))?;
        if self.scope.is_array_frozen(&arr_name) {
            return Err(PerlError::runtime(
                format!("Modification of a frozen value: @{}", arr_name),
                line,
            )
            .into());
        }
        for v in values {
            let val = self.eval_expr_ctx(v, WantarrayCtx::List)?;
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
        if let Some(aref) = self.try_eval_array_deref_container(array)? {
            return self.pop_array_deref(aref, line);
        }
        let arr_name = self.extract_array_name(Self::peel_array_builtin_operand(array))?;
        self.scope
            .pop_from_array(&arr_name)
            .map_err(|e| FlowOrError::Error(e.at_line(line)))
    }

    pub(crate) fn eval_shift_expr(
        &mut self,
        array: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if let Some(aref) = self.try_eval_array_deref_container(array)? {
            return self.shift_array_deref(aref, line);
        }
        let arr_name = self.extract_array_name(Self::peel_array_builtin_operand(array))?;
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
        if let Some(aref) = self.try_eval_array_deref_container(array)? {
            let mut vals = Vec::new();
            for v in values {
                let val = self.eval_expr_ctx(v, WantarrayCtx::List)?;
                if let Some(items) = val.as_array_vec() {
                    vals.extend(items);
                } else {
                    vals.push(val);
                }
            }
            let len = self.unshift_array_deref_multi(aref, vals, line)?;
            return Ok(PerlValue::integer(len));
        }
        let arr_name = self.extract_array_name(Self::peel_array_builtin_operand(array))?;
        let mut vals = Vec::new();
        for v in values {
            let val = self.eval_expr_ctx(v, WantarrayCtx::List)?;
            if let Some(items) = val.as_array_vec() {
                vals.extend(items);
            } else {
                vals.push(val);
            }
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

    /// One `push` element onto an array ref or package array name (symbolic `@{"Pkg::A"}`).
    pub(crate) fn push_array_deref_value(
        &mut self,
        arr_ref: PerlValue,
        val: PerlValue,
        line: usize,
    ) -> Result<(), FlowOrError> {
        if let Some(r) = arr_ref.as_array_ref() {
            let mut w = r.write();
            if let Some(items) = val.as_array_vec() {
                w.extend(items.iter().cloned());
            } else {
                w.push(val);
            }
            return Ok(());
        }
        if let Some(name) = arr_ref.as_array_binding_name() {
            if let Some(items) = val.as_array_vec() {
                for item in items {
                    self.scope
                        .push_to_array(&name, item)
                        .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
                }
            } else {
                self.scope
                    .push_to_array(&name, val)
                    .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            }
            return Ok(());
        }
        if let Some(s) = arr_ref.as_str() {
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
            let name = s.to_string();
            if let Some(items) = val.as_array_vec() {
                for item in items {
                    self.scope
                        .push_to_array(&name, item)
                        .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
                }
            } else {
                self.scope
                    .push_to_array(&name, val)
                    .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            }
            return Ok(());
        }
        Err(PerlError::runtime("push argument is not an ARRAY reference", line).into())
    }

    pub(crate) fn array_deref_len(
        &self,
        arr_ref: PerlValue,
        line: usize,
    ) -> Result<i64, FlowOrError> {
        if let Some(r) = arr_ref.as_array_ref() {
            return Ok(r.read().len() as i64);
        }
        if let Some(name) = arr_ref.as_array_binding_name() {
            return Ok(self.scope.array_len(&name) as i64);
        }
        if let Some(s) = arr_ref.as_str() {
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
            return Ok(self.scope.array_len(&s) as i64);
        }
        Err(PerlError::runtime("argument is not an ARRAY reference", line).into())
    }

    pub(crate) fn pop_array_deref(
        &mut self,
        arr_ref: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if let Some(r) = arr_ref.as_array_ref() {
            let mut w = r.write();
            return Ok(w.pop().unwrap_or(PerlValue::UNDEF));
        }
        if let Some(name) = arr_ref.as_array_binding_name() {
            return self
                .scope
                .pop_from_array(&name)
                .map_err(|e| FlowOrError::Error(e.at_line(line)));
        }
        if let Some(s) = arr_ref.as_str() {
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
            return self
                .scope
                .pop_from_array(&s)
                .map_err(|e| FlowOrError::Error(e.at_line(line)));
        }
        Err(PerlError::runtime("pop argument is not an ARRAY reference", line).into())
    }

    pub(crate) fn shift_array_deref(
        &mut self,
        arr_ref: PerlValue,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if let Some(r) = arr_ref.as_array_ref() {
            let mut w = r.write();
            return Ok(if w.is_empty() {
                PerlValue::UNDEF
            } else {
                w.remove(0)
            });
        }
        if let Some(name) = arr_ref.as_array_binding_name() {
            return self
                .scope
                .shift_from_array(&name)
                .map_err(|e| FlowOrError::Error(e.at_line(line)));
        }
        if let Some(s) = arr_ref.as_str() {
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
            return self
                .scope
                .shift_from_array(&s)
                .map_err(|e| FlowOrError::Error(e.at_line(line)));
        }
        Err(PerlError::runtime("shift argument is not an ARRAY reference", line).into())
    }

    pub(crate) fn unshift_array_deref_multi(
        &mut self,
        arr_ref: PerlValue,
        vals: Vec<PerlValue>,
        line: usize,
    ) -> Result<i64, FlowOrError> {
        let mut flat: Vec<PerlValue> = Vec::new();
        for v in vals {
            if let Some(items) = v.as_array_vec() {
                flat.extend(items);
            } else {
                flat.push(v);
            }
        }
        if let Some(r) = arr_ref.as_array_ref() {
            let mut w = r.write();
            for (i, v) in flat.into_iter().enumerate() {
                w.insert(i, v);
            }
            return Ok(w.len() as i64);
        }
        if let Some(name) = arr_ref.as_array_binding_name() {
            let arr = self
                .scope
                .get_array_mut(&name)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            for (i, v) in flat.into_iter().enumerate() {
                arr.insert(i, v);
            }
            return Ok(arr.len() as i64);
        }
        if let Some(s) = arr_ref.as_str() {
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
            let name = s.to_string();
            let arr = self
                .scope
                .get_array_mut(&name)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            for (i, v) in flat.into_iter().enumerate() {
                arr.insert(i, v);
            }
            return Ok(arr.len() as i64);
        }
        Err(PerlError::runtime("unshift argument is not an ARRAY reference", line).into())
    }

    /// `splice @$aref, OFFSET, LENGTH, LIST` — uses [`Self::wantarray_kind`] (VM [`Op::WantarrayPush`]
    /// / compiler wraps `splice` like other context-sensitive builtins).
    pub(crate) fn splice_array_deref(
        &mut self,
        aref: PerlValue,
        offset_val: PerlValue,
        length_val: PerlValue,
        rep_vals: Vec<PerlValue>,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let ctx = self.wantarray_kind;
        if let Some(r) = aref.as_array_ref() {
            let arr_len = r.read().len();
            let (off, end) = splice_compute_range(arr_len, &offset_val, &length_val);
            let mut w = r.write();
            let removed: Vec<PerlValue> = w.drain(off..end).collect();
            for (i, v) in rep_vals.into_iter().enumerate() {
                w.insert(off + i, v);
            }
            return Ok(match ctx {
                WantarrayCtx::Scalar => removed.last().cloned().unwrap_or(PerlValue::UNDEF),
                WantarrayCtx::List | WantarrayCtx::Void => PerlValue::array(removed),
            });
        }
        if let Some(name) = aref.as_array_binding_name() {
            let arr_len = self.scope.array_len(&name);
            let (off, end) = splice_compute_range(arr_len, &offset_val, &length_val);
            let arr = self
                .scope
                .get_array_mut(&name)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            let removed: Vec<PerlValue> = arr.drain(off..end).collect();
            for (i, v) in rep_vals.into_iter().enumerate() {
                arr.insert(off + i, v);
            }
            return Ok(match ctx {
                WantarrayCtx::Scalar => removed.last().cloned().unwrap_or(PerlValue::UNDEF),
                WantarrayCtx::List | WantarrayCtx::Void => PerlValue::array(removed),
            });
        }
        if let Some(s) = aref.as_str() {
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
            let arr_len = self.scope.array_len(&s);
            let (off, end) = splice_compute_range(arr_len, &offset_val, &length_val);
            let arr = self
                .scope
                .get_array_mut(&s)
                .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
            let removed: Vec<PerlValue> = arr.drain(off..end).collect();
            for (i, v) in rep_vals.into_iter().enumerate() {
                arr.insert(off + i, v);
            }
            return Ok(match ctx {
                WantarrayCtx::Scalar => removed.last().cloned().unwrap_or(PerlValue::UNDEF),
                WantarrayCtx::List | WantarrayCtx::Void => PerlValue::array(removed),
            });
        }
        Err(PerlError::runtime("splice argument is not an ARRAY reference", line).into())
    }

    pub(crate) fn eval_splice_expr(
        &mut self,
        array: &Expr,
        offset: Option<&Expr>,
        length: Option<&Expr>,
        replacement: &[Expr],
        ctx: WantarrayCtx,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        if let Some(aref) = self.try_eval_array_deref_container(array)? {
            let offset_val = if let Some(o) = offset {
                self.eval_expr(o)?
            } else {
                PerlValue::integer(0)
            };
            let length_val = if let Some(l) = length {
                self.eval_expr(l)?
            } else {
                PerlValue::UNDEF
            };
            let mut rep_vals = Vec::new();
            for r in replacement {
                rep_vals.push(self.eval_expr(r)?);
            }
            let saved = self.wantarray_kind;
            self.wantarray_kind = ctx;
            let out = self.splice_array_deref(aref, offset_val, length_val, rep_vals, line);
            self.wantarray_kind = saved;
            return out;
        }
        let arr_name = self.extract_array_name(Self::peel_array_builtin_operand(array))?;
        let arr_len = self.scope.array_len(&arr_name);
        let offset_val = if let Some(o) = offset {
            self.eval_expr(o)?
        } else {
            PerlValue::integer(0)
        };
        let length_val = if let Some(l) = length {
            self.eval_expr(l)?
        } else {
            PerlValue::UNDEF
        };
        let (off, end) = splice_compute_range(arr_len, &offset_val, &length_val);
        let mut rep_vals = Vec::new();
        for r in replacement {
            rep_vals.push(self.eval_expr(r)?);
        }
        let arr = self
            .scope
            .get_array_mut(&arr_name)
            .map_err(|e| FlowOrError::Error(e.at_line(line)))?;
        let removed: Vec<PerlValue> = arr.drain(off..end).collect();
        for (i, v) in rep_vals.into_iter().enumerate() {
            arr.insert(off + i, v);
        }
        Ok(match ctx {
            WantarrayCtx::Scalar => removed.last().cloned().unwrap_or(PerlValue::UNDEF),
            WantarrayCtx::List | WantarrayCtx::Void => PerlValue::array(removed),
        })
    }

    /// Result of `keys EXPR` after `EXPR` has been evaluated (VM opcode path or tests).
    pub(crate) fn keys_from_value(val: PerlValue, line: usize) -> Result<PerlValue, FlowOrError> {
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

    pub(crate) fn eval_keys_expr(
        &mut self,
        expr: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        // Operand must be evaluated in list context so `%h` stays a hash (scalar context would
        // apply `scalar %h`, not a hash value — breaks `keys` / `values` / `each` fallbacks).
        let val = self.eval_expr_ctx(expr, WantarrayCtx::List)?;
        Self::keys_from_value(val, line)
    }

    /// Result of `values EXPR` after `EXPR` has been evaluated.
    pub(crate) fn values_from_value(val: PerlValue, line: usize) -> Result<PerlValue, FlowOrError> {
        if let Some(h) = val.as_hash_map() {
            Ok(PerlValue::array(h.values().cloned().collect()))
        } else if let Some(r) = val.as_hash_ref() {
            Ok(PerlValue::array(r.read().values().cloned().collect()))
        } else {
            Err(PerlError::runtime("values requires hash", line).into())
        }
    }

    pub(crate) fn eval_values_expr(
        &mut self,
        expr: &Expr,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let val = self.eval_expr_ctx(expr, WantarrayCtx::List)?;
        Self::values_from_value(val, line)
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
            ExprKind::ArrayElement { array, index } => {
                self.check_strict_array_var(array, line)?;
                let idx = self.eval_expr(index)?.to_int();
                let aname = self.stash_array_name_for_package(array);
                self.scope
                    .delete_array_element(&aname, idx)
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
            ExprKind::ArrowDeref {
                expr: inner,
                index,
                kind: DerefKind::Array,
            } => {
                if !crate::compiler::arrow_deref_arrow_subscript_is_plain_scalar_index(index) {
                    return Err(PerlError::runtime(
                        "delete on array element needs scalar subscript",
                        line,
                    )
                    .into());
                }
                let container = self.eval_expr(inner)?;
                let idx = self.eval_expr(index)?.to_int();
                self.delete_arrow_array_element(container, idx, line)
                    .map_err(Into::into)
            }
            _ => Err(PerlError::runtime("delete requires hash or array element", line).into()),
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
            ExprKind::ArrayElement { array, index } => {
                self.check_strict_array_var(array, line)?;
                let idx = self.eval_expr(index)?.to_int();
                let aname = self.stash_array_name_for_package(array);
                Ok(PerlValue::integer(
                    if self.scope.exists_array_element(&aname, idx) {
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
            ExprKind::ArrowDeref {
                expr: inner,
                index,
                kind: DerefKind::Array,
            } => {
                if !crate::compiler::arrow_deref_arrow_subscript_is_plain_scalar_index(index) {
                    return Err(PerlError::runtime(
                        "exists on array element needs scalar subscript",
                        line,
                    )
                    .into());
                }
                let container = self.eval_expr(inner)?;
                let idx = self.eval_expr(index)?.to_int();
                let yes = self.exists_arrow_array_element(container, idx, line)?;
                Ok(PerlValue::integer(if yes { 1 } else { 0 }))
            }
            _ => Err(PerlError::runtime("exists requires hash or array element", line).into()),
        }
    }

    /// `pmap_on $cluster { ... } @list` — distributed map over an SSH worker pool.
    ///
    /// Uses the persistent dispatcher in [`crate::cluster`]: one ssh process per slot,
    /// HELLO + SESSION_INIT once per slot lifetime, JOB frames flowing over a shared work
    /// queue, fault tolerance via re-enqueue + retry budget. The basic v1 fan-out (one
    /// ssh per item) was replaced because it spent ~50–200 ms per item on ssh handshakes;
    /// the new path amortizes the handshake across the whole map.
    pub(crate) fn eval_pmap_remote(
        &mut self,
        cluster_pv: PerlValue,
        list_pv: PerlValue,
        show_progress: bool,
        block: &Block,
        flat_outputs: bool,
        line: usize,
    ) -> Result<PerlValue, FlowOrError> {
        let Some(cluster) = cluster_pv.as_remote_cluster() else {
            return Err(PerlError::runtime("pmap_on: expected cluster(...) value", line).into());
        };
        let items = list_pv.to_list();
        let (scope_capture, atomic_arrays, atomic_hashes) = self.scope.capture_with_atomics();
        if !atomic_arrays.is_empty() || !atomic_hashes.is_empty() {
            return Err(PerlError::runtime(
                "pmap_on: mysync/atomic capture is not supported for remote workers",
                line,
            )
            .into());
        }
        let cap_json = crate::remote_wire::capture_entries_to_json(&scope_capture)
            .map_err(|e| PerlError::runtime(e, line))?;
        let subs_prelude = crate::remote_wire::build_subs_prelude(&self.subs);
        let block_src = crate::fmt::format_block(block);
        let item_jsons =
            crate::cluster::perl_items_to_json(&items).map_err(|e| PerlError::runtime(e, line))?;

        // Progress bar (best effort) — ticks once per result. The dispatcher itself is
        // synchronous from the caller's POV, so we drive the bar before/after the call.
        let pmap_progress = PmapProgress::new(show_progress, items.len());
        let result_values =
            crate::cluster::run_cluster(&cluster, subs_prelude, block_src, cap_json, item_jsons)
                .map_err(|e| PerlError::runtime(format!("pmap_on remote: {e}"), line))?;
        for _ in 0..result_values.len() {
            pmap_progress.tick();
        }
        pmap_progress.finish();

        if flat_outputs {
            let flattened: Vec<PerlValue> = result_values
                .into_iter()
                .flat_map(|v| v.map_flatten_outputs(true))
                .collect();
            Ok(PerlValue::array(flattened))
        } else {
            Ok(PerlValue::array(result_values))
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
                FlowOrError::Error(PerlError::runtime(format!("par_lines: mmap: {}", e), line))
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
                local_interp.scope.set_topic(PerlValue::string(line_str));
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
                local_interp.scope.set_topic(PerlValue::string(s));
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
            let content = read_file_text_perl_compat(path)
                .map_err(|e| PerlError::runtime(format!("par_sed {}: {}", path, e), line))?;
            let new_s = re.replace_all(&content, &repl);
            if new_s != content {
                std::fs::write(path, new_s.as_bytes())
                    .map_err(|e| PerlError::runtime(format!("par_sed {}: {}", path, e), line))?;
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
        let expanded = rewrite_perl_regex_dollar_end_anchor(&expanded, flags.contains('m'));
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

    /// `(bracket, line)` for Perl's `die` / `warn` suffix `, <bracket> line N.` (`bracket` is `<>`, `<STDIN>`, `<FH>`, …).
    pub(crate) fn die_warn_io_annotation(&self) -> Option<(String, i64)> {
        if self.last_readline_handle.is_empty() {
            return (self.line_number > 0).then_some(("<>".to_string(), self.line_number));
        }
        let n = *self
            .handle_line_numbers
            .get(&self.last_readline_handle)
            .unwrap_or(&0);
        if n <= 0 {
            return None;
        }
        if !self.argv_current_file.is_empty() && self.last_readline_handle == self.argv_current_file
        {
            return Some(("<>".to_string(), n));
        }
        if self.last_readline_handle == "STDIN" {
            return Some((self.last_stdin_die_bracket.clone(), n));
        }
        Some((format!("<{}>", self.last_readline_handle), n))
    }

    /// Trailing ` at FILE line N` plus optional `, <> line $.` for `die` / `warn` (matches Perl 5).
    pub(crate) fn die_warn_at_suffix(&self, source_line: usize) -> String {
        let mut s = format!(" at {} line {}", self.file, source_line);
        if let Some((bracket, n)) = self.die_warn_io_annotation() {
            s.push_str(&format!(", {} line {}.", bracket, n));
        } else {
            s.push('.');
        }
        s
    }

    /// Process a line in -n/-p mode.
    ///
    /// `is_last_input_line` is true when this line is the last from the current stdin or `@ARGV`
    /// file so `eof` with no arguments matches Perl behavior on that line.
    pub fn process_line(
        &mut self,
        line_str: &str,
        program: &Program,
        is_last_input_line: bool,
    ) -> PerlResult<Option<String>> {
        self.line_mode_eof_pending = is_last_input_line;
        let result: PerlResult<Option<String>> = (|| {
            self.line_number += 1;
            self.scope
                .set_topic(PerlValue::string(line_str.to_string()));

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

            // `-p` implicit print matches `print $_` (appends `$\` / [`Self::ors`] — set by `-l`).
            let mut out = self.scope.get_scalar("_").to_string();
            out.push_str(&self.ors);
            Ok(Some(out))
        })();
        self.line_mode_eof_pending = false;
        result
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
    local_interp.scope.set_topic(PerlValue::string(s));
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
                'x' => {
                    let v = arg.to_int();
                    if zero_pad && w > 0 {
                        format!("{:0width$x}", v, width = w)
                    } else if left_align {
                        format!("{:<width$x}", v, width = w)
                    } else if w > 0 {
                        format!("{:width$x}", v, width = w)
                    } else {
                        format!("{:x}", v)
                    }
                }
                'X' => {
                    let v = arg.to_int();
                    if zero_pad && w > 0 {
                        format!("{:0width$X}", v, width = w)
                    } else if left_align {
                        format!("{:<width$X}", v, width = w)
                    } else if w > 0 {
                        format!("{:width$X}", v, width = w)
                    } else {
                        format!("{:X}", v)
                    }
                }
                'o' => {
                    let v = arg.to_int();
                    if zero_pad && w > 0 {
                        format!("{:0width$o}", v, width = w)
                    } else if left_align {
                        format!("{:<width$o}", v, width = w)
                    } else if w > 0 {
                        format!("{:width$o}", v, width = w)
                    } else {
                        format!("{:o}", v)
                    }
                }
                'b' => {
                    let v = arg.to_int();
                    if zero_pad && w > 0 {
                        format!("{:0width$b}", v, width = w)
                    } else if left_align {
                        format!("{:<width$b}", v, width = w)
                    } else if w > 0 {
                        format!("{:width$b}", v, width = w)
                    } else {
                        format!("{:b}", v)
                    }
                }
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

    /// `]` may be the first character in a Perl class when a later `]` closes it; `$` inside must
    /// stay literal (not rewritten to `(?:\n?\z)`).
    #[test]
    fn compile_regex_char_class_leading_close_bracket_is_literal() {
        let mut i = Interpreter::new();
        let re = i.compile_regex(r"[]\[^$.*/]", "", 1).expect("regex");
        assert!(re.is_match("$"));
        assert!(re.is_match("]"));
        assert!(!re.is_match("x"));
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
        assert!(Interpreter::is_special_scalar_name_for_set("."));
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

    #[test]
    fn scalar_flip_flop_three_dot_same_dollar_dot_second_eval_stays_active() {
        let mut i = Interpreter::new();
        i.last_readline_handle.clear();
        i.line_number = 3;
        i.prepare_flip_flop_vm_slots(1);
        assert_eq!(
            i.scalar_flip_flop_eval(3, 3, 0, true).expect("ok").to_int(),
            1
        );
        assert!(i.flip_flop_active[0]);
        assert_eq!(i.flip_flop_exclusive_left_line[0], Some(3));
        // Second evaluation on the same `$.` must not clear the range (Perl `...` defers the right test).
        assert_eq!(
            i.scalar_flip_flop_eval(3, 3, 0, true).expect("ok").to_int(),
            1
        );
        assert!(i.flip_flop_active[0]);
    }

    #[test]
    fn scalar_flip_flop_three_dot_deactivates_when_past_left_line_and_dot_matches_right() {
        let mut i = Interpreter::new();
        i.last_readline_handle.clear();
        i.line_number = 2;
        i.prepare_flip_flop_vm_slots(1);
        i.scalar_flip_flop_eval(2, 3, 0, true).expect("ok");
        assert!(i.flip_flop_active[0]);
        i.line_number = 3;
        i.scalar_flip_flop_eval(2, 3, 0, true).expect("ok");
        assert!(!i.flip_flop_active[0]);
        assert_eq!(i.flip_flop_exclusive_left_line[0], None);
    }
}
