use std::collections::{HashMap, VecDeque};
use std::io::{self, Write as IoWrite};
use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::RwLock;
use rayon::prelude::*;

use caseless::default_case_fold_str;

use crate::ast::{BinOp, Block, Expr, MatchArm, PerlTypeName, Sigil, SubSigParam};
use crate::bytecode::{BuiltinId, Chunk, Op, RuntimeSubDecl, SpliceExprEntry};
use crate::compiler::scalar_compound_op_from_byte;
use crate::error::{ErrorKind, PerlError, PerlResult};
use crate::interpreter::{
    fold_preduce_init_step, merge_preduce_init_partials, preduce_init_fold_identity, Flow,
    FlowOrError, Interpreter, WantarrayCtx,
};
use crate::perl_fs::read_file_text_perl_compat;
use crate::pmap_progress::{FanProgress, PmapProgress};
use crate::sort_fast::{sort_magic_cmp, SortBlockFast};
use crate::value::{
    perl_list_range_expand, PerlAsyncTask, PerlBarrier, PerlHeap, PerlSub, PerlValue,
    PipelineInner, PipelineOp,
};
use parking_lot::Mutex;
use std::sync::Barrier;

/// Stable reference for empty-stack [`VM::peek`] (not a temporary `&PerlValue::UNDEF`).
static PEEK_UNDEF: PerlValue = PerlValue::UNDEF;

/// Immutable snapshot of [`VM`] pools for rayon workers (cheap `Arc` clones; no `&mut VM` in closures).
struct ParallelBlockVmShared {
    ops: Arc<Vec<Op>>,
    names: Arc<Vec<String>>,
    constants: Arc<Vec<PerlValue>>,
    lines: Arc<Vec<usize>>,
    sub_entries: Vec<(u16, usize, bool)>,
    static_sub_calls: Vec<(usize, bool, u16)>,
    blocks: Vec<Block>,
    code_ref_sigs: Vec<Vec<SubSigParam>>,
    block_bytecode_ranges: Vec<Option<(usize, usize)>>,
    map_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    grep_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    regex_flip_flop_rhs_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    given_entries: Vec<(Expr, Block)>,
    given_topic_bytecode_ranges: Vec<Option<(usize, usize)>>,
    eval_timeout_entries: Vec<(Expr, Block)>,
    eval_timeout_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    algebraic_match_entries: Vec<(Expr, Vec<MatchArm>)>,
    algebraic_match_subject_bytecode_ranges: Vec<Option<(usize, usize)>>,
    par_lines_entries: Vec<(Expr, Expr, Option<Expr>)>,
    par_walk_entries: Vec<(Expr, Expr, Option<Expr>)>,
    pwatch_entries: Vec<(Expr, Expr)>,
    substr_four_arg_entries: Vec<(Expr, Expr, Option<Expr>, Expr)>,
    keys_expr_entries: Vec<Expr>,
    keys_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    map_expr_entries: Vec<Expr>,
    grep_expr_entries: Vec<Expr>,
    regex_flip_flop_rhs_expr_entries: Vec<Expr>,
    values_expr_entries: Vec<Expr>,
    values_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    delete_expr_entries: Vec<Expr>,
    exists_expr_entries: Vec<Expr>,
    push_expr_entries: Vec<(Expr, Vec<Expr>)>,
    pop_expr_entries: Vec<Expr>,
    shift_expr_entries: Vec<Expr>,
    unshift_expr_entries: Vec<(Expr, Vec<Expr>)>,
    splice_expr_entries: Vec<SpliceExprEntry>,
    lvalues: Vec<Expr>,
    format_decls: Vec<(String, Vec<String>)>,
    use_overload_entries: Vec<Vec<(String, String)>>,
    runtime_sub_decls: Arc<Vec<RuntimeSubDecl>>,
    jit_sub_invoke_threshold: u32,
    op_len_plus_one: usize,
    static_sub_closure_subs: Vec<Option<Arc<PerlSub>>>,
    sub_entry_by_name: HashMap<u16, (usize, bool)>,
}

impl ParallelBlockVmShared {
    fn from_vm(vm: &VM<'_>) -> Self {
        let n = vm.ops.len().saturating_add(1);
        Self {
            ops: Arc::clone(&vm.ops),
            names: Arc::clone(&vm.names),
            constants: Arc::clone(&vm.constants),
            lines: Arc::clone(&vm.lines),
            sub_entries: vm.sub_entries.clone(),
            static_sub_calls: vm.static_sub_calls.clone(),
            blocks: vm.blocks.clone(),
            code_ref_sigs: vm.code_ref_sigs.clone(),
            block_bytecode_ranges: vm.block_bytecode_ranges.clone(),
            map_expr_bytecode_ranges: vm.map_expr_bytecode_ranges.clone(),
            grep_expr_bytecode_ranges: vm.grep_expr_bytecode_ranges.clone(),
            regex_flip_flop_rhs_expr_bytecode_ranges: vm
                .regex_flip_flop_rhs_expr_bytecode_ranges
                .clone(),
            given_entries: vm.given_entries.clone(),
            given_topic_bytecode_ranges: vm.given_topic_bytecode_ranges.clone(),
            eval_timeout_entries: vm.eval_timeout_entries.clone(),
            eval_timeout_expr_bytecode_ranges: vm.eval_timeout_expr_bytecode_ranges.clone(),
            algebraic_match_entries: vm.algebraic_match_entries.clone(),
            algebraic_match_subject_bytecode_ranges: vm
                .algebraic_match_subject_bytecode_ranges
                .clone(),
            par_lines_entries: vm.par_lines_entries.clone(),
            par_walk_entries: vm.par_walk_entries.clone(),
            pwatch_entries: vm.pwatch_entries.clone(),
            substr_four_arg_entries: vm.substr_four_arg_entries.clone(),
            keys_expr_entries: vm.keys_expr_entries.clone(),
            keys_expr_bytecode_ranges: vm.keys_expr_bytecode_ranges.clone(),
            map_expr_entries: vm.map_expr_entries.clone(),
            grep_expr_entries: vm.grep_expr_entries.clone(),
            regex_flip_flop_rhs_expr_entries: vm.regex_flip_flop_rhs_expr_entries.clone(),
            values_expr_entries: vm.values_expr_entries.clone(),
            values_expr_bytecode_ranges: vm.values_expr_bytecode_ranges.clone(),
            delete_expr_entries: vm.delete_expr_entries.clone(),
            exists_expr_entries: vm.exists_expr_entries.clone(),
            push_expr_entries: vm.push_expr_entries.clone(),
            pop_expr_entries: vm.pop_expr_entries.clone(),
            shift_expr_entries: vm.shift_expr_entries.clone(),
            unshift_expr_entries: vm.unshift_expr_entries.clone(),
            splice_expr_entries: vm.splice_expr_entries.clone(),
            lvalues: vm.lvalues.clone(),
            format_decls: vm.format_decls.clone(),
            use_overload_entries: vm.use_overload_entries.clone(),
            runtime_sub_decls: Arc::clone(&vm.runtime_sub_decls),
            jit_sub_invoke_threshold: vm.jit_sub_invoke_threshold,
            op_len_plus_one: n,
            static_sub_closure_subs: vm.static_sub_closure_subs.clone(),
            sub_entry_by_name: vm.sub_entry_by_name.clone(),
        }
    }

    fn worker_vm<'a>(&self, interp: &'a mut Interpreter) -> VM<'a> {
        let n = self.op_len_plus_one;
        VM {
            names: Arc::clone(&self.names),
            constants: Arc::clone(&self.constants),
            ops: Arc::clone(&self.ops),
            lines: Arc::clone(&self.lines),
            sub_entries: self.sub_entries.clone(),
            static_sub_calls: self.static_sub_calls.clone(),
            blocks: self.blocks.clone(),
            code_ref_sigs: self.code_ref_sigs.clone(),
            block_bytecode_ranges: self.block_bytecode_ranges.clone(),
            map_expr_bytecode_ranges: self.map_expr_bytecode_ranges.clone(),
            grep_expr_bytecode_ranges: self.grep_expr_bytecode_ranges.clone(),
            regex_flip_flop_rhs_expr_bytecode_ranges: self
                .regex_flip_flop_rhs_expr_bytecode_ranges
                .clone(),
            given_entries: self.given_entries.clone(),
            given_topic_bytecode_ranges: self.given_topic_bytecode_ranges.clone(),
            eval_timeout_entries: self.eval_timeout_entries.clone(),
            eval_timeout_expr_bytecode_ranges: self.eval_timeout_expr_bytecode_ranges.clone(),
            algebraic_match_entries: self.algebraic_match_entries.clone(),
            algebraic_match_subject_bytecode_ranges: self
                .algebraic_match_subject_bytecode_ranges
                .clone(),
            par_lines_entries: self.par_lines_entries.clone(),
            par_walk_entries: self.par_walk_entries.clone(),
            pwatch_entries: self.pwatch_entries.clone(),
            substr_four_arg_entries: self.substr_four_arg_entries.clone(),
            keys_expr_entries: self.keys_expr_entries.clone(),
            keys_expr_bytecode_ranges: self.keys_expr_bytecode_ranges.clone(),
            map_expr_entries: self.map_expr_entries.clone(),
            grep_expr_entries: self.grep_expr_entries.clone(),
            regex_flip_flop_rhs_expr_entries: self.regex_flip_flop_rhs_expr_entries.clone(),
            values_expr_entries: self.values_expr_entries.clone(),
            values_expr_bytecode_ranges: self.values_expr_bytecode_ranges.clone(),
            delete_expr_entries: self.delete_expr_entries.clone(),
            exists_expr_entries: self.exists_expr_entries.clone(),
            push_expr_entries: self.push_expr_entries.clone(),
            pop_expr_entries: self.pop_expr_entries.clone(),
            shift_expr_entries: self.shift_expr_entries.clone(),
            unshift_expr_entries: self.unshift_expr_entries.clone(),
            splice_expr_entries: self.splice_expr_entries.clone(),
            lvalues: self.lvalues.clone(),
            format_decls: self.format_decls.clone(),
            use_overload_entries: self.use_overload_entries.clone(),
            runtime_sub_decls: Arc::clone(&self.runtime_sub_decls),
            ip: 0,
            stack: Vec::with_capacity(256),
            call_stack: Vec::with_capacity(32),
            wantarray_stack: Vec::with_capacity(8),
            interp,
            jit_enabled: false,
            sub_jit_skip_linear: vec![false; n],
            sub_jit_skip_block: vec![false; n],
            sub_entry_at_ip: {
                let mut v = vec![false; n];
                for (_, e, _) in &self.sub_entries {
                    if *e < v.len() {
                        v[*e] = true;
                    }
                }
                v
            },
            sub_entry_invoke_count: vec![0; n],
            jit_sub_invoke_threshold: self.jit_sub_invoke_threshold,
            jit_buf_slot: Vec::new(),
            jit_buf_plain: Vec::new(),
            jit_buf_arg: Vec::new(),
            jit_trampoline_out: None,
            jit_trampoline_depth: 0,
            halt: false,
            try_stack: Vec::new(),
            pending_catch_error: None,
            exit_main_dispatch: false,
            exit_main_dispatch_value: None,
            static_sub_closure_subs: self.static_sub_closure_subs.clone(),
            sub_entry_by_name: self.sub_entry_by_name.clone(),
            block_region_mode: false,
            block_region_end: 0,
            block_region_return: None,
        }
    }
}

#[inline]
fn vm_interp_result(r: Result<PerlValue, FlowOrError>, line: usize) -> PerlResult<PerlValue> {
    match r {
        Ok(v) => Ok(v),
        Err(FlowOrError::Error(e)) => Err(e),
        Err(FlowOrError::Flow(_)) => Err(PerlError::runtime(
            "unexpected control flow in tree-assisted opcode",
            line,
        )),
    }
}

/// Saved state for `try { } catch (…) { } finally { }`.
/// Jump targets live in [`Op::TryPush`] and are patched after emission; we only store the op index.
#[derive(Debug, Clone)]
pub(crate) struct TryFrame {
    pub(crate) try_push_op_idx: usize,
}

/// Saved state when entering a function call.
#[derive(Debug)]
struct CallFrame {
    return_ip: usize,
    stack_base: usize,
    scope_depth: usize,
    saved_wantarray: WantarrayCtx,
    /// [`stryke_jit_call_sub`] — no bytecode resume; result stored in [`VM::jit_trampoline_out`].
    jit_trampoline_return: bool,
    /// Synthetic frame for [`Op::BlockReturnValue`] (`map`/`grep`/`sort` block bytecode), paired with
    /// `scope_push_hook` at [`VM::run_block_region`] entry (not a sub call; no closure capture).
    block_region: bool,
    /// Wall-clock start for [`crate::profiler::Profiler::exit_sub`] (paired with `enter_sub` on `Call`).
    sub_profiler_start: Option<std::time::Instant>,
}

/// Stack-based bytecode virtual machine.
pub struct VM<'a> {
    /// Shared with parallel workers via [`Self::new_parallel_worker`] (cheap `Arc` clones).
    names: Arc<Vec<String>>,
    constants: Arc<Vec<PerlValue>>,
    ops: Arc<Vec<Op>>,
    lines: Arc<Vec<usize>>,
    sub_entries: Vec<(u16, usize, bool)>,
    /// See [`Chunk::static_sub_calls`] (`Op::CallStaticSubId`).
    static_sub_calls: Vec<(usize, bool, u16)>,
    blocks: Vec<Block>,
    code_ref_sigs: Vec<Vec<SubSigParam>>,
    /// Optional `ops[start..end]` lowering for [`Self::blocks`] (see [`Chunk::block_bytecode_ranges`]).
    block_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// Optional lowering for [`Chunk::map_expr_entries`] (see [`Chunk::map_expr_bytecode_ranges`]).
    map_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// Optional lowering for [`Chunk::grep_expr_entries`] (see [`Chunk::grep_expr_bytecode_ranges`]).
    grep_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    given_entries: Vec<(Expr, Block)>,
    given_topic_bytecode_ranges: Vec<Option<(usize, usize)>>,
    eval_timeout_entries: Vec<(Expr, Block)>,
    eval_timeout_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    algebraic_match_entries: Vec<(Expr, Vec<MatchArm>)>,
    algebraic_match_subject_bytecode_ranges: Vec<Option<(usize, usize)>>,
    par_lines_entries: Vec<(Expr, Expr, Option<Expr>)>,
    par_walk_entries: Vec<(Expr, Expr, Option<Expr>)>,
    pwatch_entries: Vec<(Expr, Expr)>,
    substr_four_arg_entries: Vec<(Expr, Expr, Option<Expr>, Expr)>,
    keys_expr_entries: Vec<Expr>,
    keys_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    map_expr_entries: Vec<Expr>,
    grep_expr_entries: Vec<Expr>,
    regex_flip_flop_rhs_expr_entries: Vec<Expr>,
    regex_flip_flop_rhs_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    values_expr_entries: Vec<Expr>,
    values_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    delete_expr_entries: Vec<Expr>,
    exists_expr_entries: Vec<Expr>,
    push_expr_entries: Vec<(Expr, Vec<Expr>)>,
    pop_expr_entries: Vec<Expr>,
    shift_expr_entries: Vec<Expr>,
    unshift_expr_entries: Vec<(Expr, Vec<Expr>)>,
    splice_expr_entries: Vec<SpliceExprEntry>,
    lvalues: Vec<Expr>,
    format_decls: Vec<(String, Vec<String>)>,
    use_overload_entries: Vec<Vec<(String, String)>>,
    runtime_sub_decls: Arc<Vec<RuntimeSubDecl>>,
    ip: usize,
    stack: Vec<PerlValue>,
    call_stack: Vec<CallFrame>,
    /// Paired with [`Op::WantarrayPush`] / [`Op::WantarrayPop`] (e.g. `splice` list vs scalar return).
    wantarray_stack: Vec<WantarrayCtx>,
    interp: &'a mut Interpreter,
    /// When `false`, [`VM::execute`] skips Cranelift JIT (linear, block, and subroutine linear) and
    /// uses only the opcode interpreter. Default `true`.
    jit_enabled: bool,
    /// `sub_jit_skip_linear[ip]` — true when linear sub-JIT cannot apply (control flow / calls).
    /// Indexed by IP for O(1) lookup instead of hashing (recursive subs like fib hit this millions of times).
    sub_jit_skip_linear: Vec<bool>,
    /// `sub_jit_skip_block[ip]` — true when block sub-JIT cannot apply.
    sub_jit_skip_block: Vec<bool>,
    /// `sub_entry_at_ip[ip]` — faster than hashing on every opcode (recursive subs dispatch millions of ops).
    sub_entry_at_ip: Vec<bool>,
    /// Invocations per sub-entry IP (tiered JIT: interpreter until count exceeds threshold).
    sub_entry_invoke_count: Vec<u32>,
    /// Minimum invocations before attempting subroutine JIT. Override with `STRYKE_JIT_SUB_INVOKES` (default 50).
    jit_sub_invoke_threshold: u32,
    /// Reused `i64` tables for sub-JIT / top-level JIT attempts (avoids `vec![0; n]` on every try).
    jit_buf_slot: Vec<i64>,
    jit_buf_plain: Vec<i64>,
    jit_buf_arg: Vec<i64>,
    /// Set when running [`VM::jit_trampoline_run_sub`]; [`Op::ReturnValue`] stores here and exits dispatch.
    jit_trampoline_out: Option<PerlValue>,
    /// Nesting depth for [`Self::jit_trampoline_run_sub`]; dispatch breaks on [`Self::jit_trampoline_out`] only when `> 0`.
    jit_trampoline_depth: u32,
    /// Set by [`Op::Halt`]; outer loop exits after handling [`Self::try_recover_from_exception`].
    halt: bool,
    /// Stack of active `try` regions (LIFO).
    try_stack: Vec<TryFrame>,
    /// Error message for the next [`Op::CatchReceive`] (set before jumping to `catch_ip`).
    pub(crate) pending_catch_error: Option<String>,
    /// [`Op::Return`] / [`Op::ReturnValue`] with no caller frame: exit the main dispatch loop (was `break`).
    exit_main_dispatch: bool,
    /// Top-level [`Op::ReturnValue`] with no frame: value for implicit return (was `last = val; break`).
    exit_main_dispatch_value: Option<PerlValue>,
    /// [`Chunk::static_sub_calls`] index → pre-resolved [`PerlSub`] for closure restore (stash key lookup once at VM build).
    static_sub_closure_subs: Vec<Option<Arc<PerlSub>>>,
    /// O(1) [`Chunk::sub_entries`] lookup (same first-wins semantics as the old linear scan).
    sub_entry_by_name: HashMap<u16, (usize, bool)>,
    /// When executing [`Chunk::block_bytecode_ranges`] via [`Self::run_block_region`].
    block_region_mode: bool,
    block_region_end: usize,
    block_region_return: Option<PerlValue>,
}

impl<'a> VM<'a> {
    pub fn new(chunk: &Chunk, interp: &'a mut Interpreter) -> Self {
        let static_sub_closure_subs: Vec<Option<Arc<PerlSub>>> = chunk
            .static_sub_calls
            .iter()
            .map(|(_, _, name_idx)| {
                let nm = chunk.names[*name_idx as usize].as_str();
                interp.subs.get(nm).cloned()
            })
            .collect();
        let mut sub_entry_by_name = HashMap::with_capacity(chunk.sub_entries.len());
        for &(n, ip, sa) in &chunk.sub_entries {
            sub_entry_by_name.entry(n).or_insert((ip, sa));
        }
        Self {
            names: Arc::new(chunk.names.clone()),
            constants: Arc::new(chunk.constants.clone()),
            ops: Arc::new(chunk.ops.clone()),
            lines: Arc::new(chunk.lines.clone()),
            sub_entries: chunk.sub_entries.clone(),
            static_sub_calls: chunk.static_sub_calls.clone(),
            blocks: chunk.blocks.clone(),
            code_ref_sigs: chunk.code_ref_sigs.clone(),
            block_bytecode_ranges: chunk.block_bytecode_ranges.clone(),
            map_expr_bytecode_ranges: chunk.map_expr_bytecode_ranges.clone(),
            grep_expr_bytecode_ranges: chunk.grep_expr_bytecode_ranges.clone(),
            regex_flip_flop_rhs_expr_bytecode_ranges: chunk
                .regex_flip_flop_rhs_expr_bytecode_ranges
                .clone(),
            given_entries: chunk.given_entries.clone(),
            given_topic_bytecode_ranges: chunk.given_topic_bytecode_ranges.clone(),
            eval_timeout_entries: chunk.eval_timeout_entries.clone(),
            eval_timeout_expr_bytecode_ranges: chunk.eval_timeout_expr_bytecode_ranges.clone(),
            algebraic_match_entries: chunk.algebraic_match_entries.clone(),
            algebraic_match_subject_bytecode_ranges: chunk
                .algebraic_match_subject_bytecode_ranges
                .clone(),
            par_lines_entries: chunk.par_lines_entries.clone(),
            par_walk_entries: chunk.par_walk_entries.clone(),
            pwatch_entries: chunk.pwatch_entries.clone(),
            substr_four_arg_entries: chunk.substr_four_arg_entries.clone(),
            keys_expr_entries: chunk.keys_expr_entries.clone(),
            keys_expr_bytecode_ranges: chunk.keys_expr_bytecode_ranges.clone(),
            map_expr_entries: chunk.map_expr_entries.clone(),
            grep_expr_entries: chunk.grep_expr_entries.clone(),
            regex_flip_flop_rhs_expr_entries: chunk.regex_flip_flop_rhs_expr_entries.clone(),
            values_expr_entries: chunk.values_expr_entries.clone(),
            values_expr_bytecode_ranges: chunk.values_expr_bytecode_ranges.clone(),
            delete_expr_entries: chunk.delete_expr_entries.clone(),
            exists_expr_entries: chunk.exists_expr_entries.clone(),
            push_expr_entries: chunk.push_expr_entries.clone(),
            pop_expr_entries: chunk.pop_expr_entries.clone(),
            shift_expr_entries: chunk.shift_expr_entries.clone(),
            unshift_expr_entries: chunk.unshift_expr_entries.clone(),
            splice_expr_entries: chunk.splice_expr_entries.clone(),
            lvalues: chunk.lvalues.clone(),
            format_decls: chunk.format_decls.clone(),
            use_overload_entries: chunk.use_overload_entries.clone(),
            runtime_sub_decls: Arc::new(chunk.runtime_sub_decls.clone()),
            ip: 0,
            stack: Vec::with_capacity(256),
            call_stack: Vec::with_capacity(32),
            wantarray_stack: Vec::with_capacity(8),
            interp,
            jit_enabled: true,
            sub_jit_skip_linear: vec![false; chunk.ops.len().saturating_add(1)],
            sub_jit_skip_block: vec![false; chunk.ops.len().saturating_add(1)],
            sub_entry_at_ip: {
                let mut v = vec![false; chunk.ops.len().saturating_add(1)];
                for (_, e, _) in &chunk.sub_entries {
                    if *e < v.len() {
                        v[*e] = true;
                    }
                }
                v
            },
            sub_entry_invoke_count: vec![0; chunk.ops.len().saturating_add(1)],
            jit_sub_invoke_threshold: std::env::var("STRYKE_JIT_SUB_INVOKES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50),
            jit_buf_slot: Vec::new(),
            jit_buf_plain: Vec::new(),
            jit_buf_arg: Vec::new(),
            jit_trampoline_out: None,
            jit_trampoline_depth: 0,
            halt: false,
            try_stack: Vec::new(),
            pending_catch_error: None,
            exit_main_dispatch: false,
            exit_main_dispatch_value: None,
            static_sub_closure_subs,
            sub_entry_by_name,
            block_region_mode: false,
            block_region_end: 0,
            block_region_return: None,
        }
    }

    /// Pop a synthetic [`CallFrame::block_region`] frame if dispatch exited before
    /// [`Op::BlockReturnValue`] (error or fallthrough), restoring stack and scope.
    fn unwind_stale_block_region_frame(&mut self) {
        if let Some(frame) = self.call_stack.pop() {
            if frame.block_region {
                self.interp.wantarray_kind = frame.saved_wantarray;
                self.stack.truncate(frame.stack_base);
                self.interp.pop_scope_to_depth(frame.scope_depth);
            } else {
                self.call_stack.push(frame);
            }
        }
    }

    /// Run `ops[start..end]` (exclusive) for a compiled `map`/`grep`/`sort` block body.
    ///
    /// Matches [`Interpreter::exec_block`]: `$_` / `$a` / `$b` are set in the caller before each
    /// iteration; then one block-local scope frame is pushed (no closure capture) and the body runs
    /// inline. [`Op::BlockReturnValue`] unwinds that frame via [`Self::unwind_stale_block_region_frame`]
    /// on error paths here.
    fn run_block_region(
        &mut self,
        start: usize,
        end: usize,
        op_count: &mut u64,
    ) -> PerlResult<PerlValue> {
        let resume_ip = self.ip;
        let saved_mode = self.block_region_mode;
        let saved_end = self.block_region_end;
        let saved_ret = self.block_region_return.take();

        let scope_depth_before = self.interp.scope.depth();
        let saved_wa = self.interp.wantarray_kind;

        self.call_stack.push(CallFrame {
            return_ip: 0,
            stack_base: self.stack.len(),
            scope_depth: scope_depth_before,
            saved_wantarray: saved_wa,
            jit_trampoline_return: false,
            block_region: true,
            sub_profiler_start: None,
        });
        self.interp.scope_push_hook();
        self.interp.wantarray_kind = WantarrayCtx::Scalar;
        self.ip = start;
        self.block_region_mode = true;
        self.block_region_end = end;
        self.block_region_return = None;

        let r = self.run_main_dispatch_loop(PerlValue::UNDEF, op_count, false);
        let out = self.block_region_return.take();

        self.block_region_return = saved_ret;
        self.block_region_mode = saved_mode;
        self.block_region_end = saved_end;
        self.ip = resume_ip;

        match r {
            Ok(_) => {
                if let Some(val) = out {
                    Ok(val)
                } else {
                    self.unwind_stale_block_region_frame();
                    Err(PerlError::runtime(
                        "block bytecode region did not finish with BlockReturnValue",
                        self.line(),
                    ))
                }
            }
            Err(e) => {
                self.unwind_stale_block_region_frame();
                Err(e)
            }
        }
    }

    #[inline]
    fn extend_map_outputs(dst: &mut Vec<PerlValue>, val: PerlValue, peel_array_ref: bool) {
        dst.extend(val.map_flatten_outputs(peel_array_ref));
    }

    fn map_with_block_common(
        &mut self,
        list: Vec<PerlValue>,
        block_idx: u16,
        peel_array_ref: bool,
        op_count: &mut u64,
    ) -> PerlResult<()> {
        if list.len() == 1 {
            if let Some(p) = list[0].as_pipeline() {
                if peel_array_ref {
                    return Err(PerlError::runtime(
                        "flat_map onto a pipeline value is not supported in this form — use a pipeline ->map stage",
                        self.line(),
                    ));
                }
                let idx = block_idx as usize;
                let sub = self.interp.anon_coderef_from_block(&self.blocks[idx]);
                let line = self.line();
                self.interp.pipeline_push(&p, PipelineOp::Map(sub), line)?;
                self.push(PerlValue::pipeline(Arc::clone(&p)));
                return Ok(());
            }
        }
        let idx = block_idx as usize;
        // map's BLOCK is list context. The shared block bytecode region is compiled with a
        // scalar-context tail (grep/sort consumers need that), so when the block's tail is
        // list-sensitive (`($_, $_*10)`, `1..$_`, `reverse …`, an array variable, …) fall
        // back to the tree walker's list-tail [`Interpreter::exec_block_with_tail`]. For
        // plain scalar tails (`$_ * 2`, `f($_)`, string ops) the bytecode region produces
        // the same value in either context, so keep using it for speed.
        let block_tail_is_list_sensitive = self
            .blocks
            .get(idx)
            .and_then(|b| b.last())
            .map(|stmt| match &stmt.kind {
                crate::ast::StmtKind::Expression(expr) => {
                    crate::compiler::expr_tail_is_list_sensitive(expr)
                }
                _ => true,
            })
            .unwrap_or(true);
        if !block_tail_is_list_sensitive {
            if let Some(&(start, end)) =
                self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
            {
                let mut result = Vec::new();
                for item in list {
                    self.interp.scope.set_topic(item);
                    let val = self.run_block_region(start, end, op_count)?;
                    Self::extend_map_outputs(&mut result, val, peel_array_ref);
                }
                self.push(PerlValue::array(result));
                return Ok(());
            }
        }
        let block = self.blocks[idx].clone();
        let mut result = Vec::new();
        for item in list {
            self.interp.scope.set_topic(item);
            match self.interp.exec_block_with_tail(&block, WantarrayCtx::List) {
                Ok(val) => Self::extend_map_outputs(&mut result, val, peel_array_ref),
                Err(FlowOrError::Error(e)) => return Err(e),
                Err(_) => {}
            }
        }
        self.push(PerlValue::array(result));
        Ok(())
    }

    fn map_with_expr_common(
        &mut self,
        list: Vec<PerlValue>,
        expr_idx: u16,
        peel_array_ref: bool,
        op_count: &mut u64,
    ) -> PerlResult<()> {
        let idx = expr_idx as usize;
        if let Some(&(start, end)) = self
            .map_expr_bytecode_ranges
            .get(idx)
            .and_then(|r| r.as_ref())
        {
            let mut result = Vec::new();
            for item in list {
                self.interp.scope.set_topic(item);
                let val = self.run_block_region(start, end, op_count)?;
                Self::extend_map_outputs(&mut result, val, peel_array_ref);
            }
            self.push(PerlValue::array(result));
        } else {
            let e = &self.map_expr_entries[idx];
            let mut result = Vec::new();
            for item in list {
                self.interp.scope.set_topic(item);
                let val = vm_interp_result(
                    self.interp.eval_expr_ctx(e, WantarrayCtx::List),
                    self.line(),
                )?;
                Self::extend_map_outputs(&mut result, val, peel_array_ref);
            }
            self.push(PerlValue::array(result));
        }
        Ok(())
    }

    /// Consecutive groups: key from block with `$_`; keys compared with [`PerlValue::str_eq`].
    fn chunk_by_with_block_common(
        &mut self,
        list: Vec<PerlValue>,
        block_idx: u16,
        op_count: &mut u64,
    ) -> PerlResult<()> {
        if list.is_empty() {
            self.push(PerlValue::array(vec![]));
            return Ok(());
        }
        let idx = block_idx as usize;
        let mut chunks: Vec<PerlValue> = Vec::new();
        let mut run: Vec<PerlValue> = Vec::new();
        let mut prev_key: Option<PerlValue> = None;

        let eval_key =
            |vm: &mut VM, item: PerlValue, op_count: &mut u64| -> PerlResult<PerlValue> {
                vm.interp.scope.set_topic(item);
                if let Some(&(start, end)) =
                    vm.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                {
                    vm.run_block_region(start, end, op_count)
                } else {
                    let block = vm.blocks[idx].clone();
                    match vm.interp.exec_block(&block) {
                        Ok(val) => Ok(val),
                        Err(FlowOrError::Error(e)) => Err(e),
                        Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
                        Err(_) => Ok(PerlValue::UNDEF),
                    }
                }
            };

        for item in list {
            let key = eval_key(self, item.clone(), op_count)?;
            match &prev_key {
                None => {
                    run.push(item);
                    prev_key = Some(key);
                }
                Some(pk) => {
                    if key.str_eq(pk) {
                        run.push(item);
                    } else {
                        chunks.push(PerlValue::array_ref(Arc::new(RwLock::new(std::mem::take(
                            &mut run,
                        )))));
                        run.push(item);
                        prev_key = Some(key);
                    }
                }
            }
        }
        if !run.is_empty() {
            chunks.push(PerlValue::array_ref(Arc::new(RwLock::new(run))));
        }
        self.push(PerlValue::array(chunks));
        Ok(())
    }

    fn chunk_by_with_expr_common(
        &mut self,
        list: Vec<PerlValue>,
        expr_idx: u16,
        op_count: &mut u64,
    ) -> PerlResult<()> {
        if list.is_empty() {
            self.push(PerlValue::array(vec![]));
            return Ok(());
        }
        let idx = expr_idx as usize;
        let mut chunks: Vec<PerlValue> = Vec::new();
        let mut run: Vec<PerlValue> = Vec::new();
        let mut prev_key: Option<PerlValue> = None;
        for item in list {
            self.interp.scope.set_topic(item.clone());
            let key = if let Some(&(start, end)) = self
                .map_expr_bytecode_ranges
                .get(idx)
                .and_then(|r| r.as_ref())
            {
                self.run_block_region(start, end, op_count)?
            } else {
                let e = &self.map_expr_entries[idx];
                vm_interp_result(
                    self.interp.eval_expr_ctx(e, WantarrayCtx::Scalar),
                    self.line(),
                )?
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
                        chunks.push(PerlValue::array_ref(Arc::new(RwLock::new(std::mem::take(
                            &mut run,
                        )))));
                        run.push(item);
                        prev_key = Some(key);
                    }
                }
            }
        }
        if !run.is_empty() {
            chunks.push(PerlValue::array_ref(Arc::new(RwLock::new(run))));
        }
        self.push(PerlValue::array(chunks));
        Ok(())
    }

    #[inline]
    fn sub_jit_skip_linear_test(&self, ip: usize) -> bool {
        self.sub_jit_skip_linear.get(ip).copied().unwrap_or(false)
    }

    #[inline]
    fn sub_jit_skip_linear_mark(&mut self, ip: usize) {
        if ip >= self.sub_jit_skip_linear.len() {
            self.sub_jit_skip_linear.resize(ip + 1, false);
        }
        self.sub_jit_skip_linear[ip] = true;
    }

    #[inline]
    fn sub_jit_skip_block_test(&self, ip: usize) -> bool {
        self.sub_jit_skip_block.get(ip).copied().unwrap_or(false)
    }

    #[inline]
    fn sub_jit_skip_block_mark(&mut self, ip: usize) {
        if ip >= self.sub_jit_skip_block.len() {
            self.sub_jit_skip_block.resize(ip + 1, false);
        }
        self.sub_jit_skip_block[ip] = true;
    }

    /// Enable or disable Cranelift JIT for this execution. Disabling skips compilation and buffer
    /// prefetch for JIT paths (pure interpreter).
    pub fn set_jit_enabled(&mut self, enabled: bool) {
        self.jit_enabled = enabled;
    }

    #[inline]
    fn push(&mut self, val: PerlValue) {
        self.stack.push(val);
    }

    #[inline]
    fn pop(&mut self) -> PerlValue {
        self.stack.pop().unwrap_or(PerlValue::UNDEF)
    }

    /// Pop `n` array-slice index specs (TOS = last spec). Each spec is a scalar index or an array
    /// of indices (list-context `..`, `qw/.../`, parenthesized list), matching
    /// [`crate::compiler::Compiler::compile_array_slice_index_expr`]. Returns flattened indices in
    /// source order (first spec’s indices first).
    fn pop_flattened_array_slice_specs(&mut self, n: usize) -> Vec<i64> {
        let mut chunks: Vec<Vec<i64>> = Vec::with_capacity(n);
        for _ in 0..n {
            let spec = self.pop();
            let mut flat = Vec::new();
            if let Some(av) = spec.as_array_vec() {
                for pv in av.iter() {
                    flat.push(pv.to_int());
                }
            } else {
                flat.push(spec.to_int());
            }
            chunks.push(flat);
        }
        chunks.reverse();
        chunks.into_iter().flatten().collect()
    }

    /// Call operands are pushed so the rightmost syntactic argument is on top. Restore
    /// left-to-right order, then flatten list-valued operands (`qw/.../`, list literals) into
    /// successive scalars — matching Perl's argument list for simple calls. Reversing after
    /// flattening would incorrectly reverse elements inside expanded lists.
    fn pop_call_operands_flattened(&mut self, argc: usize) -> Vec<PerlValue> {
        let mut slots = Vec::with_capacity(argc);
        for _ in 0..argc {
            slots.push(self.pop());
        }
        slots.reverse();
        let mut out = Vec::new();
        for v in slots {
            if let Some(items) = v.as_array_vec() {
                out.extend(items);
            } else {
                out.push(v);
            }
        }
        out
    }

    /// Like [`Self::pop_call_operands_flattened`], but each syntactic argument stays one
    /// [`PerlValue`] (`zip` / `mesh` need full lists per operand, not Perl's flattened `@_`).
    fn pop_call_operands_preserved(&mut self, argc: usize) -> Vec<PerlValue> {
        let mut slots = Vec::with_capacity(argc);
        for _ in 0..argc {
            slots.push(self.pop());
        }
        slots.reverse();
        slots
    }

    #[inline]
    fn call_preserve_operand_arrays(name: &str) -> bool {
        matches!(
            name,
            "zip"
                | "List::Util::zip"
                | "List::Util::zip_longest"
                | "List::Util::zip_shortest"
                | "List::Util::mesh"
                | "List::Util::mesh_longest"
                | "List::Util::mesh_shortest"
                | "take"
                | "head"
                | "tail"
                | "drop"
                | "List::Util::head"
                | "List::Util::tail"
        )
    }

    fn flatten_array_slice_specs_ordered_values(
        &self,
        specs: &[PerlValue],
    ) -> Result<Vec<i64>, PerlError> {
        let mut out = Vec::new();
        for spec in specs {
            if let Some(av) = spec.as_array_vec() {
                for pv in av.iter() {
                    out.push(pv.to_int());
                }
            } else {
                out.push(spec.to_int());
            }
        }
        Ok(out)
    }

    /// Hash `{…}` slice key slots in source order (each slot may expand to many string keys).
    fn flatten_hash_slice_key_slots(key_vals: &[PerlValue]) -> Vec<String> {
        let mut ks = Vec::new();
        for kv in key_vals {
            if let Some(vv) = kv.as_array_vec() {
                ks.extend(vv.iter().map(|x| x.to_string()));
            } else {
                ks.push(kv.to_string());
            }
        }
        ks
    }

    #[inline]
    fn peek(&self) -> &PerlValue {
        self.stack.last().unwrap_or(&PEEK_UNDEF)
    }

    #[inline]
    fn constant(&self, idx: u16) -> &PerlValue {
        &self.constants[idx as usize]
    }

    fn line(&self) -> usize {
        self.lines
            .get(self.ip.saturating_sub(1))
            .copied()
            .unwrap_or(0)
    }

    /// Cranelift linear JIT for a subroutine body when `ip` is a compiled sub entry (see `Chunk::sub_entries`).
    /// Returns `Ok(true)` when the sub was executed natively and the VM should continue at `return_ip`.
    fn try_jit_subroutine_linear(&mut self) -> Result<bool, PerlError> {
        let ip = self.ip;
        debug_assert!(self.sub_entry_at_ip.get(ip).copied().unwrap_or(false));
        if self.sub_jit_skip_linear_test(ip) {
            return Ok(false);
        }
        let ops: &Vec<Op> = &self.ops;
        let ops = ops as *const Vec<Op>;
        let ops = unsafe { &*ops };
        let constants: &Vec<PerlValue> = &self.constants;
        let constants = constants as *const Vec<PerlValue>;
        let constants = unsafe { &*constants };
        let names: &Vec<String> = &self.names;
        let names = names as *const Vec<String>;
        let names = unsafe { &*names };
        let Some((seg, _)) = crate::jit::sub_entry_segment(ops, ip) else {
            return Ok(false);
        };
        // `try_run_linear_sub` rejects these segments without compiling — skip expensive work before
        // resize/fill of reusable scratch buffers (`jit_buf_*`).
        if crate::jit::segment_blocks_subroutine_linear_jit(seg, &self.sub_entries) {
            self.sub_jit_skip_linear_mark(ip);
            return Ok(false);
        }
        let mut slot_len: Option<usize> = None;
        if let Some(max) = crate::jit::linear_slot_ops_max_index_seq(seg) {
            let n = max as usize + 1;
            self.jit_buf_slot.resize(n, 0);
            let mut ok = true;
            for i in 0..=max {
                let pv = self.interp.scope.get_scalar_slot(i);
                self.jit_buf_slot[i as usize] = match pv.as_integer() {
                    Some(v) => v,
                    None if pv.is_undef() && crate::jit::slot_undef_prefill_ok_seq(seg, i) => 0,
                    None => {
                        ok = false;
                        break;
                    }
                };
            }
            if ok {
                slot_len = Some(n);
            }
        }
        let mut plain_len: Option<usize> = None;
        if let Some(max) = crate::jit::linear_plain_ops_max_index_seq(seg) {
            if (max as usize) < names.len() {
                let n = max as usize + 1;
                self.jit_buf_plain.resize(n, 0);
                let mut ok = true;
                for i in 0..=max {
                    let nm = names[i as usize].as_str();
                    match self.interp.scope.get_scalar(nm).as_integer() {
                        Some(v) => self.jit_buf_plain[i as usize] = v,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    plain_len = Some(n);
                }
            }
        }
        let mut arg_len: Option<usize> = None;
        if let Some(max) = crate::jit::linear_arg_ops_max_index_seq(seg) {
            if let Some(frame) = self.call_stack.last() {
                let base = frame.stack_base;
                let n = max as usize + 1;
                self.jit_buf_arg.resize(n, 0);
                let mut ok = true;
                for i in 0..=max {
                    let pos = base + i as usize;
                    let pv = self.stack.get(pos).cloned().unwrap_or(PerlValue::UNDEF);
                    match pv.as_integer() {
                        Some(v) => self.jit_buf_arg[i as usize] = v,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    arg_len = Some(n);
                }
            }
        }
        let vm_ptr = self as *mut VM<'_> as *mut std::ffi::c_void;
        let slot_buf = slot_len.map(|n| &mut self.jit_buf_slot[..n]);
        let plain_buf = plain_len.map(|n| &mut self.jit_buf_plain[..n]);
        let arg_buf = arg_len.map(|n| &self.jit_buf_arg[..n]);
        let Some(v) = crate::jit::try_run_linear_sub(
            ops,
            ip,
            slot_buf,
            plain_buf,
            arg_buf,
            constants,
            &self.sub_entries,
            vm_ptr,
        ) else {
            return Ok(false);
        };
        if let Some(n) = slot_len {
            let buf = &self.jit_buf_slot[..n];
            for idx in crate::jit::linear_slot_ops_written_indices_seq(seg) {
                self.interp
                    .scope
                    .set_scalar_slot(idx, PerlValue::integer(buf[idx as usize]));
            }
        }
        if let Some(n) = plain_len {
            let buf = &self.jit_buf_plain[..n];
            for idx in crate::jit::linear_plain_ops_written_indices_seq(seg) {
                let name = names[idx as usize].as_str();
                self.interp
                    .scope
                    .set_scalar(name, PerlValue::integer(buf[idx as usize]))
                    .map_err(|e| e.at_line(self.line()))?;
            }
        }
        if let Some(frame) = self.call_stack.pop() {
            self.interp.wantarray_kind = frame.saved_wantarray;
            self.stack.truncate(frame.stack_base);
            self.interp.pop_scope_to_depth(frame.scope_depth);
            if frame.jit_trampoline_return {
                self.jit_trampoline_out = Some(v);
            } else {
                self.push(v);
                self.ip = frame.return_ip;
            }
        }
        Ok(true)
    }

    /// Cranelift block JIT for a subroutine with control flow (see [`crate::jit::block_jit_validate_sub`]).
    fn try_jit_subroutine_block(&mut self) -> Result<bool, PerlError> {
        let ip = self.ip;
        debug_assert!(self.sub_entry_at_ip.get(ip).copied().unwrap_or(false));
        if self.sub_jit_skip_block_test(ip) {
            return Ok(false);
        }
        let vm_ptr = self as *mut VM<'_> as *mut std::ffi::c_void;
        let ops: &Vec<Op> = &self.ops;
        let constants: &Vec<PerlValue> = &self.constants;
        let names: &Vec<String> = &self.names;
        let Some((full_body, term)) = crate::jit::sub_full_body(ops, ip) else {
            return Ok(false);
        };
        if crate::jit::sub_body_blocks_subroutine_block_jit(full_body) {
            self.sub_jit_skip_block_mark(ip);
            return Ok(false);
        }
        let Some(validated) =
            crate::jit::block_jit_validate_sub(full_body, constants, term, &self.sub_entries)
        else {
            self.sub_jit_skip_block_mark(ip);
            return Ok(false);
        };
        let block_buf_mode = validated.buffer_mode();

        let mut b_slot_len: Option<usize> = None;
        if let Some(max) = crate::jit::block_slot_ops_max_index(full_body) {
            let n = max as usize + 1;
            self.jit_buf_slot.resize(n, 0);
            let mut ok = true;
            for i in 0..=max {
                let pv = self.interp.scope.get_scalar_slot(i);
                self.jit_buf_slot[i as usize] = match block_buf_mode {
                    crate::jit::BlockJitBufferMode::I64AsPerlValueBits => pv.raw_bits() as i64,
                    crate::jit::BlockJitBufferMode::I64AsInteger => match pv.as_integer() {
                        Some(v) => v,
                        None if pv.is_undef()
                            && crate::jit::block_slot_undef_prefill_ok(full_body, i) =>
                        {
                            0
                        }
                        None => {
                            ok = false;
                            break;
                        }
                    },
                };
            }
            if ok {
                b_slot_len = Some(n);
            }
        }

        let mut b_plain_len: Option<usize> = None;
        if let Some(max) = crate::jit::block_plain_ops_max_index(full_body) {
            if (max as usize) < names.len() {
                let n = max as usize + 1;
                self.jit_buf_plain.resize(n, 0);
                let mut ok = true;
                for i in 0..=max {
                    let nm = names[i as usize].as_str();
                    let pv = self.interp.scope.get_scalar(nm);
                    self.jit_buf_plain[i as usize] = match block_buf_mode {
                        crate::jit::BlockJitBufferMode::I64AsPerlValueBits => pv.raw_bits() as i64,
                        crate::jit::BlockJitBufferMode::I64AsInteger => match pv.as_integer() {
                            Some(v) => v,
                            None => {
                                ok = false;
                                break;
                            }
                        },
                    };
                }
                if ok {
                    b_plain_len = Some(n);
                }
            }
        }

        let mut b_arg_len: Option<usize> = None;
        if let Some(max) = crate::jit::block_arg_ops_max_index(full_body) {
            if let Some(frame) = self.call_stack.last() {
                let base = frame.stack_base;
                let n = max as usize + 1;
                self.jit_buf_arg.resize(n, 0);
                let mut ok = true;
                for i in 0..=max {
                    let pos = base + i as usize;
                    let pv = self.stack.get(pos).cloned().unwrap_or(PerlValue::UNDEF);
                    self.jit_buf_arg[i as usize] = match block_buf_mode {
                        crate::jit::BlockJitBufferMode::I64AsPerlValueBits => pv.raw_bits() as i64,
                        crate::jit::BlockJitBufferMode::I64AsInteger => match pv.as_integer() {
                            Some(v) => v,
                            None => {
                                ok = false;
                                break;
                            }
                        },
                    };
                }
                if ok {
                    b_arg_len = Some(n);
                }
            }
        }

        let block_slot_buf = b_slot_len.map(|n| &mut self.jit_buf_slot[..n]);
        let block_plain_buf = b_plain_len.map(|n| &mut self.jit_buf_plain[..n]);
        let block_arg_buf = b_arg_len.map(|n| &self.jit_buf_arg[..n]);

        let Some((v, buf_mode)) = crate::jit::try_run_block_ops(
            full_body,
            block_slot_buf,
            block_plain_buf,
            block_arg_buf,
            constants,
            Some(validated),
            vm_ptr,
            &self.sub_entries,
        ) else {
            self.sub_jit_skip_block_mark(ip);
            return Ok(false);
        };

        if let Some(n) = b_slot_len {
            let buf = &self.jit_buf_slot[..n];
            for idx in crate::jit::block_slot_ops_written_indices(full_body) {
                let bits = buf[idx as usize] as u64;
                let pv = match buf_mode {
                    crate::jit::BlockJitBufferMode::I64AsPerlValueBits => {
                        PerlValue::from_raw_bits(bits)
                    }
                    crate::jit::BlockJitBufferMode::I64AsInteger => {
                        PerlValue::integer(buf[idx as usize])
                    }
                };
                self.interp.scope.set_scalar_slot(idx, pv);
            }
        }
        if let Some(n) = b_plain_len {
            let buf = &self.jit_buf_plain[..n];
            for idx in crate::jit::block_plain_ops_written_indices(full_body) {
                let name = names[idx as usize].as_str();
                let bits = buf[idx as usize] as u64;
                let pv = match buf_mode {
                    crate::jit::BlockJitBufferMode::I64AsPerlValueBits => {
                        PerlValue::from_raw_bits(bits)
                    }
                    crate::jit::BlockJitBufferMode::I64AsInteger => {
                        PerlValue::integer(buf[idx as usize])
                    }
                };
                self.interp
                    .scope
                    .set_scalar(name, pv)
                    .map_err(|e| e.at_line(self.line()))?;
            }
        }
        if let Some(frame) = self.call_stack.pop() {
            self.interp.wantarray_kind = frame.saved_wantarray;
            self.stack.truncate(frame.stack_base);
            self.interp.pop_scope_to_depth(frame.scope_depth);
            if frame.jit_trampoline_return {
                self.jit_trampoline_out = Some(v);
            } else {
                self.push(v);
                self.ip = frame.return_ip;
            }
        }
        Ok(true)
    }

    fn run_method_op(
        &mut self,
        name_idx: u16,
        argc: u8,
        wa: u8,
        super_call: bool,
    ) -> PerlResult<()> {
        let method_owned = self.names[name_idx as usize].clone();
        let argc = argc as usize;
        let want = WantarrayCtx::from_byte(wa);
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(self.pop());
        }
        args.reverse();
        let obj = self.pop();
        let method = method_owned.as_str();
        if let Some(r) = crate::pchannel::dispatch_method(&obj, method, &args, self.line()) {
            self.push(r?);
            return Ok(());
        }
        if let Some(r) = self
            .interp
            .try_native_method(&obj, method, &args, self.line())
        {
            self.push(r?);
            return Ok(());
        }
        let class = if let Some(b) = obj.as_blessed_ref() {
            b.class.clone()
        } else if let Some(s) = obj.as_str() {
            s
        } else {
            return Err(PerlError::runtime(
                "Can't call method on non-object",
                self.line(),
            ));
        };
        if method == "VERSION" && !super_call {
            if let Some(ver) = self.interp.package_version_scalar(class.as_str())? {
                self.push(ver);
                return Ok(());
            }
        }
        // UNIVERSAL methods: isa, can, DOES
        if !super_call {
            match method {
                "isa" => {
                    let target = args.first().map(|v| v.to_string()).unwrap_or_default();
                    let mro = self.interp.mro_linearize(&class);
                    let result = mro.iter().any(|c| c == &target);
                    self.push(PerlValue::integer(if result { 1 } else { 0 }));
                    return Ok(());
                }
                "can" => {
                    let target_method = args.first().map(|v| v.to_string()).unwrap_or_default();
                    let found = self
                        .interp
                        .resolve_method_full_name(&class, &target_method, false)
                        .and_then(|fq| self.interp.subs.get(&fq))
                        .is_some();
                    if found {
                        self.push(PerlValue::code_ref(std::sync::Arc::new(
                            crate::value::PerlSub {
                                name: target_method,
                                params: vec![],
                                body: vec![],
                                closure_env: None,
                                prototype: None,
                                fib_like: None,
                            },
                        )));
                    } else {
                        self.push(PerlValue::UNDEF);
                    }
                    return Ok(());
                }
                "DOES" => {
                    let target = args.first().map(|v| v.to_string()).unwrap_or_default();
                    let mro = self.interp.mro_linearize(&class);
                    let result = mro.iter().any(|c| c == &target);
                    self.push(PerlValue::integer(if result { 1 } else { 0 }));
                    return Ok(());
                }
                _ => {}
            }
        }
        let mut all_args = vec![obj];
        all_args.extend(args);
        let full_name = match self
            .interp
            .resolve_method_full_name(&class, method, super_call)
        {
            Some(f) => f,
            None => {
                return Err(PerlError::runtime(
                    format!(
                        "Can't locate method \"{}\" via inheritance (invocant \"{}\")",
                        method, class
                    ),
                    self.line(),
                ));
            }
        };
        if let Some(sub) = self.interp.subs.get(&full_name).cloned() {
            let saved_wa = self.interp.wantarray_kind;
            self.interp.wantarray_kind = want;
            self.interp.scope_push_hook();
            self.interp.scope.declare_array("_", all_args);
            if let Some(ref env) = sub.closure_env {
                self.interp.scope.restore_capture(env);
            }
            let line = self.line();
            let argv = self.interp.scope.take_sub_underscore().unwrap_or_default();
            self.interp
                .apply_sub_signature(sub.as_ref(), &argv, line)
                .map_err(|e| e.at_line(line))?;
            self.interp.scope.declare_array("_", argv);
            let result = self.interp.exec_block_no_scope(&sub.body);
            self.interp.wantarray_kind = saved_wa;
            self.interp.scope_pop_hook();
            match result {
                Ok(v) => self.push(v),
                Err(crate::interpreter::FlowOrError::Flow(crate::interpreter::Flow::Return(v))) => {
                    self.push(v)
                }
                Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                Err(_) => self.push(PerlValue::UNDEF),
            }
        } else if method == "new" && !super_call {
            if class == "Set" {
                self.push(crate::value::set_from_elements(
                    all_args.into_iter().skip(1),
                ));
            } else if let Some(def) = self.interp.struct_defs.get(&class).cloned() {
                let line = self.line();
                let mut provided = Vec::new();
                let mut i = 1;
                while i + 1 < all_args.len() {
                    let k = all_args[i].to_string();
                    let v = all_args[i + 1].clone();
                    provided.push((k, v));
                    i += 2;
                }
                let mut defaults = Vec::with_capacity(def.fields.len());
                for field in &def.fields {
                    if let Some(ref expr) = field.default {
                        let val = self.interp.eval_expr(expr).map_err(|e| match e {
                            crate::interpreter::FlowOrError::Error(stryke) => stryke,
                            _ => PerlError::runtime("default evaluation flow", line),
                        })?;
                        defaults.push(Some(val));
                    } else {
                        defaults.push(None);
                    }
                }
                let v =
                    crate::native_data::struct_new_with_defaults(&def, &provided, &defaults, line)?;
                self.push(v);
            } else {
                let mut map = IndexMap::new();
                let mut i = 1;
                while i + 1 < all_args.len() {
                    map.insert(all_args[i].to_string(), all_args[i + 1].clone());
                    i += 2;
                }
                self.push(PerlValue::blessed(Arc::new(
                    crate::value::BlessedRef::new_blessed(class, PerlValue::hash(map)),
                )));
            }
        } else if let Some(result) =
            self.interp
                .try_autoload_call(&full_name, all_args, self.line(), want, Some(&class))
        {
            match result {
                Ok(v) => self.push(v),
                Err(crate::interpreter::FlowOrError::Flow(crate::interpreter::Flow::Return(v))) => {
                    self.push(v)
                }
                Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                Err(_) => self.push(PerlValue::UNDEF),
            }
        } else {
            return Err(PerlError::runtime(
                format!(
                    "Can't locate method \"{}\" in package \"{}\"",
                    method, class
                ),
                self.line(),
            ));
        }
        Ok(())
    }

    fn run_fan_block(
        &mut self,
        block_idx: u16,
        n: usize,
        line: usize,
        progress: bool,
    ) -> PerlResult<()> {
        let block = self.blocks[block_idx as usize].clone();
        let subs = self.interp.subs.clone();
        let (scope_capture, atomic_arrays, atomic_hashes) =
            self.interp.scope.capture_with_atomics();
        let fan_progress = FanProgress::new(progress, n);
        let first_err: Arc<Mutex<Option<PerlError>>> = Arc::new(Mutex::new(None));
        (0..n).into_par_iter().for_each(|i| {
            if first_err.lock().is_some() {
                return;
            }
            fan_progress.start_worker(i);
            let mut local_interp = Interpreter::new();
            local_interp.subs = subs.clone();
            local_interp.suppress_stdout = progress;
            local_interp.scope.restore_capture(&scope_capture);
            local_interp
                .scope
                .restore_atomics(&atomic_arrays, &atomic_hashes);
            local_interp.enable_parallel_guard();
            local_interp.scope.set_topic(PerlValue::integer(i as i64));
            crate::parallel_trace::fan_worker_set_index(Some(i as i64));
            local_interp.scope_push_hook();
            match local_interp.exec_block_no_scope(&block) {
                Ok(_) => {}
                Err(e) => {
                    let stryke = match e {
                        FlowOrError::Error(stryke) => stryke,
                        FlowOrError::Flow(_) => PerlError::runtime(
                            "return/last/next/redo not supported inside fan block",
                            line,
                        ),
                    };
                    let mut g = first_err.lock();
                    if g.is_none() {
                        *g = Some(stryke);
                    }
                }
            }
            local_interp.scope_pop_hook();
            crate::parallel_trace::fan_worker_set_index(None);
            fan_progress.finish_worker(i);
        });
        fan_progress.finish();
        if let Some(e) = first_err.lock().take() {
            return Err(e);
        }
        self.push(PerlValue::UNDEF);
        Ok(())
    }

    fn run_fan_cap_block(
        &mut self,
        block_idx: u16,
        n: usize,
        line: usize,
        progress: bool,
    ) -> PerlResult<()> {
        let block = self.blocks[block_idx as usize].clone();
        let subs = self.interp.subs.clone();
        let (scope_capture, atomic_arrays, atomic_hashes) =
            self.interp.scope.capture_with_atomics();
        let fan_progress = FanProgress::new(progress, n);
        let pairs: Vec<(usize, Result<PerlValue, FlowOrError>)> = (0..n)
            .into_par_iter()
            .map(|i| {
                fan_progress.start_worker(i);
                let mut local_interp = Interpreter::new();
                local_interp.subs = subs.clone();
                local_interp.suppress_stdout = progress;
                local_interp.scope.restore_capture(&scope_capture);
                local_interp
                    .scope
                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                local_interp.enable_parallel_guard();
                local_interp.scope.set_topic(PerlValue::integer(i as i64));
                crate::parallel_trace::fan_worker_set_index(Some(i as i64));
                local_interp.scope_push_hook();
                let res = local_interp.exec_block_no_scope(&block);
                local_interp.scope_pop_hook();
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
                Err(e) => {
                    let stryke = match e {
                        FlowOrError::Error(stryke) => stryke,
                        FlowOrError::Flow(_) => PerlError::runtime(
                            "return/last/next/redo not supported inside fan_cap block",
                            line,
                        ),
                    };
                    return Err(stryke);
                }
            }
        }
        self.push(PerlValue::array(out));
        Ok(())
    }

    fn require_scalar_mutable(&self, name: &str) -> PerlResult<()> {
        if self.interp.scope.is_scalar_frozen(name) {
            return Err(PerlError::syntax(
                format!("cannot assign to frozen variable `${}`", name),
                self.line(),
            ));
        }
        Ok(())
    }

    fn require_array_mutable(&self, name: &str) -> PerlResult<()> {
        if self.interp.scope.is_array_frozen(name) {
            return Err(PerlError::syntax(
                format!("cannot modify frozen array `@{}`", name),
                self.line(),
            ));
        }
        Ok(())
    }

    fn require_hash_mutable(&self, name: &str) -> PerlResult<()> {
        if self.interp.scope.is_hash_frozen(name) {
            return Err(PerlError::syntax(
                format!("cannot modify frozen hash `%{}`", name),
                self.line(),
            ));
        }
        Ok(())
    }

    /// Run bytecode: first attempts Cranelift method JIT for eligible numeric fragments (unless
    /// [`VM::set_jit_enabled`] disabled it). For block JIT, `block_jit_validate` runs once per attempt;
    /// buffers may use `PerlValue::raw_bits` for `defined`-style control flow. Then the main opcode
    /// interpreter loop.
    pub fn execute(&mut self) -> PerlResult<PerlValue> {
        let ops_ref: &Vec<Op> = &self.ops;
        let ops = ops_ref as *const Vec<Op>;
        // SAFETY: ops doesn't change during execution; pointer avoids borrow on self
        let ops = unsafe { &*ops };
        let names_ref: &Vec<String> = &self.names;
        let names = names_ref as *const Vec<String>;
        // SAFETY: names doesn't change during execution; pointer avoids borrow on self
        let names = unsafe { &*names };
        let constants_ref: &Vec<PerlValue> = &self.constants;
        let constants = constants_ref as *const Vec<PerlValue>;
        // SAFETY: constants doesn't change during execution; pointer avoids borrow on self
        let constants = unsafe { &*constants };
        let mut last = PerlValue::UNDEF;
        // Safety limit: [`run_main_dispatch_loop`] counts ops (1B cap).
        let mut op_count: u64 = 0;

        // Match tree-walker `exec_statement_inner`: deliver `%SIG` and set `$^C` latch (Unix).
        crate::perl_signal::poll(self.interp)?;
        if self.jit_enabled {
            let mut top_slot_len: Option<usize> = None;
            if let Some(max) = crate::jit::linear_slot_ops_max_index(ops) {
                let n = max as usize + 1;
                self.jit_buf_slot.resize(n, 0);
                let mut ok = true;
                for i in 0..=max {
                    let pv = self.interp.scope.get_scalar_slot(i);
                    self.jit_buf_slot[i as usize] = match pv.as_integer() {
                        Some(v) => v,
                        None if pv.is_undef() && crate::jit::slot_undef_prefill_ok(ops, i) => 0,
                        None => {
                            ok = false;
                            break;
                        }
                    };
                }
                if ok {
                    top_slot_len = Some(n);
                }
            }

            let mut top_plain_len: Option<usize> = None;
            if let Some(max) = crate::jit::linear_plain_ops_max_index(ops) {
                if (max as usize) < names.len() {
                    let n = max as usize + 1;
                    self.jit_buf_plain.resize(n, 0);
                    let mut ok = true;
                    for i in 0..=max {
                        let nm = names[i as usize].as_str();
                        match self.interp.scope.get_scalar(nm).as_integer() {
                            Some(v) => self.jit_buf_plain[i as usize] = v,
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        top_plain_len = Some(n);
                    }
                }
            }

            let mut top_arg_len: Option<usize> = None;
            if let Some(max) = crate::jit::linear_arg_ops_max_index(ops) {
                if let Some(frame) = self.call_stack.last() {
                    let base = frame.stack_base;
                    let n = max as usize + 1;
                    self.jit_buf_arg.resize(n, 0);
                    let mut ok = true;
                    for i in 0..=max {
                        let pos = base + i as usize;
                        let pv = self.stack.get(pos).cloned().unwrap_or(PerlValue::UNDEF);
                        match pv.as_integer() {
                            Some(v) => self.jit_buf_arg[i as usize] = v,
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        top_arg_len = Some(n);
                    }
                }
            }

            let slot_buf = top_slot_len.map(|n| &mut self.jit_buf_slot[..n]);
            let plain_buf = top_plain_len.map(|n| &mut self.jit_buf_plain[..n]);
            let arg_buf = top_arg_len.map(|n| &self.jit_buf_arg[..n]);

            if let Some(v) =
                crate::jit::try_run_linear_ops(ops, slot_buf, plain_buf, arg_buf, constants)
            {
                if let Some(n) = top_slot_len {
                    let buf = &self.jit_buf_slot[..n];
                    for idx in crate::jit::linear_slot_ops_written_indices(ops) {
                        self.interp
                            .scope
                            .set_scalar_slot(idx, PerlValue::integer(buf[idx as usize]));
                    }
                }
                if let Some(n) = top_plain_len {
                    let buf = &self.jit_buf_plain[..n];
                    for idx in crate::jit::linear_plain_ops_written_indices(ops) {
                        let name = names[idx as usize].as_str();
                        self.interp
                            .scope
                            .set_scalar(name, PerlValue::integer(buf[idx as usize]))?;
                    }
                }
                return Ok(v);
            }

            // ── Block JIT: try to compile sequences with control flow (loops, conditionals). ──
            if let Some(validated) =
                crate::jit::block_jit_validate(ops, constants, &self.sub_entries)
            {
                let block_buf_mode = validated.buffer_mode();

                let mut top_b_slot_len: Option<usize> = None;
                if let Some(max) = crate::jit::block_slot_ops_max_index(ops) {
                    let n = max as usize + 1;
                    self.jit_buf_slot.resize(n, 0);
                    let mut ok = true;
                    for i in 0..=max {
                        let pv = self.interp.scope.get_scalar_slot(i);
                        self.jit_buf_slot[i as usize] = match block_buf_mode {
                            crate::jit::BlockJitBufferMode::I64AsPerlValueBits => {
                                pv.raw_bits() as i64
                            }
                            crate::jit::BlockJitBufferMode::I64AsInteger => match pv.as_integer() {
                                Some(v) => v,
                                None if pv.is_undef()
                                    && crate::jit::block_slot_undef_prefill_ok(ops, i) =>
                                {
                                    0
                                }
                                None => {
                                    ok = false;
                                    break;
                                }
                            },
                        };
                    }
                    if ok {
                        top_b_slot_len = Some(n);
                    }
                }

                let mut top_b_plain_len: Option<usize> = None;
                if let Some(max) = crate::jit::block_plain_ops_max_index(ops) {
                    if (max as usize) < names.len() {
                        let n = max as usize + 1;
                        self.jit_buf_plain.resize(n, 0);
                        let mut ok = true;
                        for i in 0..=max {
                            let nm = names[i as usize].as_str();
                            let pv = self.interp.scope.get_scalar(nm);
                            self.jit_buf_plain[i as usize] = match block_buf_mode {
                                crate::jit::BlockJitBufferMode::I64AsPerlValueBits => {
                                    pv.raw_bits() as i64
                                }
                                crate::jit::BlockJitBufferMode::I64AsInteger => {
                                    match pv.as_integer() {
                                        Some(v) => v,
                                        None => {
                                            ok = false;
                                            break;
                                        }
                                    }
                                }
                            };
                        }
                        if ok {
                            top_b_plain_len = Some(n);
                        }
                    }
                }

                let mut top_b_arg_len: Option<usize> = None;
                if let Some(max) = crate::jit::block_arg_ops_max_index(ops) {
                    if let Some(frame) = self.call_stack.last() {
                        let base = frame.stack_base;
                        let n = max as usize + 1;
                        self.jit_buf_arg.resize(n, 0);
                        let mut ok = true;
                        for i in 0..=max {
                            let pos = base + i as usize;
                            let pv = self.stack.get(pos).cloned().unwrap_or(PerlValue::UNDEF);
                            self.jit_buf_arg[i as usize] = match block_buf_mode {
                                crate::jit::BlockJitBufferMode::I64AsPerlValueBits => {
                                    pv.raw_bits() as i64
                                }
                                crate::jit::BlockJitBufferMode::I64AsInteger => {
                                    match pv.as_integer() {
                                        Some(v) => v,
                                        None => {
                                            ok = false;
                                            break;
                                        }
                                    }
                                }
                            };
                        }
                        if ok {
                            top_b_arg_len = Some(n);
                        }
                    }
                }

                let vm_ptr = self as *mut VM<'_> as *mut std::ffi::c_void;
                let block_slot_buf = top_b_slot_len.map(|n| &mut self.jit_buf_slot[..n]);
                let block_plain_buf = top_b_plain_len.map(|n| &mut self.jit_buf_plain[..n]);
                let block_arg_buf = top_b_arg_len.map(|n| &self.jit_buf_arg[..n]);

                if let Some((v, buf_mode)) = crate::jit::try_run_block_ops(
                    ops,
                    block_slot_buf,
                    block_plain_buf,
                    block_arg_buf,
                    constants,
                    Some(validated),
                    vm_ptr,
                    &self.sub_entries,
                ) {
                    if let Some(n) = top_b_slot_len {
                        let buf = &self.jit_buf_slot[..n];
                        for idx in crate::jit::block_slot_ops_written_indices(ops) {
                            let bits = buf[idx as usize] as u64;
                            let pv = match buf_mode {
                                crate::jit::BlockJitBufferMode::I64AsPerlValueBits => {
                                    PerlValue::from_raw_bits(bits)
                                }
                                crate::jit::BlockJitBufferMode::I64AsInteger => {
                                    PerlValue::integer(buf[idx as usize])
                                }
                            };
                            self.interp.scope.set_scalar_slot(idx, pv);
                        }
                    }
                    if let Some(n) = top_b_plain_len {
                        let buf = &self.jit_buf_plain[..n];
                        for idx in crate::jit::block_plain_ops_written_indices(ops) {
                            let name = names[idx as usize].as_str();
                            let bits = buf[idx as usize] as u64;
                            let pv = match buf_mode {
                                crate::jit::BlockJitBufferMode::I64AsPerlValueBits => {
                                    PerlValue::from_raw_bits(bits)
                                }
                                crate::jit::BlockJitBufferMode::I64AsInteger => {
                                    PerlValue::integer(buf[idx as usize])
                                }
                            };
                            self.interp.scope.set_scalar(name, pv)?;
                        }
                    }
                    return Ok(v);
                }
            }
        }

        last = self.run_main_dispatch_loop(last, &mut op_count, true)?;

        Ok(last)
    }

    /// `die` / runtime errors inside `try` jump to `catch_ip` unless the error is [`ErrorKind::Exit`].
    fn try_recover_from_exception(&mut self, e: &PerlError) -> PerlResult<bool> {
        if matches!(e.kind, ErrorKind::Exit(_)) {
            return Ok(false);
        }
        let Some(frame) = self.try_stack.last() else {
            return Ok(false);
        };
        let Op::TryPush { catch_ip, .. } = &self.ops[frame.try_push_op_idx] else {
            return Ok(false);
        };
        self.pending_catch_error = Some(e.to_string());
        self.ip = *catch_ip;
        Ok(true)
    }

    /// Stash lookup only (qualified key from compiler); avoids `resolve_sub_by_name`'s package fallback on hot calls.
    #[inline]
    fn sub_for_closure_restore(&self, name: &str) -> Option<Arc<PerlSub>> {
        self.interp.subs.get(name).cloned()
    }

    fn vm_dispatch_user_call(
        &mut self,
        name_idx: u16,
        entry_opt: Option<(usize, bool)>,
        argc_u8: u8,
        wa_byte: u8,
        // Pre-resolved sub for `Op::CallStaticSubId` (stash lookup once in `VM::new`).
        closure_sub_hint: Option<Arc<PerlSub>>,
    ) -> PerlResult<()> {
        let name_owned = self.names[name_idx as usize].clone();
        let name = name_owned.as_str();
        let argc = argc_u8 as usize;
        let want = WantarrayCtx::from_byte(wa_byte);

        if let Some((entry_ip, stack_args)) = entry_opt {
            let saved_wa = self.interp.wantarray_kind;
            let sub_prof_t0 = self.interp.profiler.is_some().then(std::time::Instant::now);
            if let Some(p) = &mut self.interp.profiler {
                p.enter_sub(name);
            }

            // Fib-shaped recursive-add fast path: if the target sub is tagged with a
            // `fib_like` pattern (detected at sub-registration time in the compiler and
            // cached in `static_sub_closure_subs`), skip frame setup entirely and
            // evaluate the closed-form-ish iterative version. `bench_fib` collapses from
            // ~2.7M recursive VM calls to a single `while` loop.
            let fib_sub: Option<Arc<PerlSub>> = closure_sub_hint
                .clone()
                .or_else(|| self.sub_for_closure_restore(name));
            if let Some(ref sub_arc) = fib_sub {
                if let Some(pat) = sub_arc.fib_like.as_ref() {
                    // stack_args path pushes exactly `argc` ints; non-stack_args pops them
                    // off the stack into @_. Only the argc==1 / integer case qualifies.
                    if argc == 1 {
                        let top_idx = self.stack.len().saturating_sub(1);
                        if let Some(n0) = self.stack.get(top_idx).and_then(|v| v.as_integer()) {
                            let result = crate::fib_like_tail::eval_fib_like_recursive_add(n0, pat);
                            // Drop the arg, push the result, keep wantarray as the caller had it.
                            self.stack.truncate(top_idx);
                            self.push(PerlValue::integer(result));
                            if let (Some(p), Some(t0)) = (&mut self.interp.profiler, sub_prof_t0) {
                                p.exit_sub(t0.elapsed());
                            }
                            self.interp.wantarray_kind = saved_wa;
                            return Ok(());
                        }
                    }
                }
            }

            if stack_args {
                let eff_argc = if argc == 0 {
                    self.push(self.interp.scope.get_scalar("_").clone());
                    1
                } else {
                    argc
                };
                let stack_base = self.stack.len() - eff_argc;
                self.call_stack.push(CallFrame {
                    return_ip: self.ip,
                    stack_base,
                    scope_depth: self.interp.scope.depth(),
                    saved_wantarray: saved_wa,
                    jit_trampoline_return: false,
                    block_region: false,
                    sub_profiler_start: sub_prof_t0,
                });
                self.interp.wantarray_kind = want;
                self.interp.scope_push_hook();
                let closure_sub = closure_sub_hint.or_else(|| self.sub_for_closure_restore(name));
                if let Some(ref sub) = closure_sub {
                    if let Some(ref env) = sub.closure_env {
                        self.interp.scope.restore_capture(env);
                    }
                    self.interp.current_sub_stack.push(sub.clone());
                }
                self.ip = entry_ip;
            } else {
                let args = if Self::call_preserve_operand_arrays(name) {
                    self.pop_call_operands_preserved(argc)
                } else {
                    self.pop_call_operands_flattened(argc)
                };
                let args = self.interp.with_topic_default_args(args);
                self.call_stack.push(CallFrame {
                    return_ip: self.ip,
                    stack_base: self.stack.len(),
                    scope_depth: self.interp.scope.depth(),
                    saved_wantarray: saved_wa,
                    jit_trampoline_return: false,
                    block_region: false,
                    sub_profiler_start: sub_prof_t0,
                });
                self.interp.wantarray_kind = want;
                self.interp.scope_push_hook();
                self.interp.scope.declare_array("_", args);
                let closure_sub = closure_sub_hint.or_else(|| self.sub_for_closure_restore(name));
                if let Some(ref sub) = closure_sub {
                    if let Some(ref env) = sub.closure_env {
                        self.interp.scope.restore_capture(env);
                    }
                    let line = self.line();
                    let argv = self.interp.scope.take_sub_underscore().unwrap_or_default();
                    self.interp
                        .apply_sub_signature(sub.as_ref(), &argv, line)
                        .map_err(|e| e.at_line(line))?;
                    self.interp.scope.declare_array("_", argv.clone());
                    self.interp.scope.set_closure_args(&argv);
                    self.interp.current_sub_stack.push(sub.clone());
                }
                self.ip = entry_ip;
            }
        } else {
            let args = if Self::call_preserve_operand_arrays(name) {
                self.pop_call_operands_preserved(argc)
            } else {
                self.pop_call_operands_flattened(argc)
            };

            let saved_wa_call = self.interp.wantarray_kind;
            self.interp.wantarray_kind = want;
            if let Some(r) = crate::builtins::try_builtin(self.interp, name, &args, self.line()) {
                self.interp.wantarray_kind = saved_wa_call;
                self.push(r?);
            } else {
                self.interp.wantarray_kind = saved_wa_call;
                if let Some(sub) = self.interp.resolve_sub_by_name(name) {
                    let t0 = self.interp.profiler.is_some().then(std::time::Instant::now);
                    if let Some(p) = &mut self.interp.profiler {
                        p.enter_sub(name);
                    }
                    let args = self.interp.with_topic_default_args(args);
                    let saved_wa = self.interp.wantarray_kind;
                    self.interp.wantarray_kind = want;
                    self.interp.scope_push_hook();
                    self.interp.scope.declare_array("_", args);
                    if let Some(ref env) = sub.closure_env {
                        self.interp.scope.restore_capture(env);
                    }
                    let argv = self.interp.scope.take_sub_underscore().unwrap_or_default();
                    let line = self.line();
                    self.interp
                        .apply_sub_signature(&sub, &argv, line)
                        .map_err(|e| e.at_line(line))?;
                    let result = if let Some(r) =
                        crate::list_util::native_dispatch(self.interp, &sub, &argv, want)
                    {
                        r
                    } else {
                        self.interp.scope.declare_array("_", argv.clone());
                        self.interp.scope.set_closure_args(&argv);
                        self.interp
                            .exec_block_no_scope_with_tail(&sub.body, WantarrayCtx::List)
                    };
                    self.interp.wantarray_kind = saved_wa;
                    self.interp.scope_pop_hook();
                    match result {
                        Ok(v) => self.push(v),
                        Err(crate::interpreter::FlowOrError::Flow(
                            crate::interpreter::Flow::Return(v),
                        )) => self.push(v),
                        Err(crate::interpreter::FlowOrError::Error(e)) => {
                            if let (Some(p), Some(t0)) = (&mut self.interp.profiler, t0) {
                                p.exit_sub(t0.elapsed());
                            }
                            return Err(e);
                        }
                        Err(_) => self.push(PerlValue::UNDEF),
                    }
                    if let (Some(p), Some(t0)) = (&mut self.interp.profiler, t0) {
                        p.exit_sub(t0.elapsed());
                    }
                } else if !name.contains("::")
                    && matches!(
                        name,
                        "uniq"
                            | "distinct"
                            | "uniqstr"
                            | "uniqint"
                            | "uniqnum"
                            | "shuffle"
                            | "sample"
                            | "chunked"
                            | "windowed"
                            | "zip"
                            | "zip_shortest"
                            | "zip_longest"
                            | "mesh"
                            | "mesh_shortest"
                            | "mesh_longest"
                            | "any"
                            | "all"
                            | "none"
                            | "notall"
                            | "first"
                            | "reduce"
                            | "reductions"
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
                            | "pairs"
                            | "unpairs"
                            | "pairkeys"
                            | "pairvalues"
                            | "pairgrep"
                            | "pairmap"
                            | "pairfirst"
                    )
                {
                    let t0 = self.interp.profiler.is_some().then(std::time::Instant::now);
                    if let Some(p) = &mut self.interp.profiler {
                        p.enter_sub(name);
                    }
                    let saved_wa = self.interp.wantarray_kind;
                    self.interp.wantarray_kind = want;
                    let out = self
                        .interp
                        .call_bare_list_util(name, args, self.line(), want);
                    self.interp.wantarray_kind = saved_wa;
                    match out {
                        Ok(v) => self.push(v),
                        Err(crate::interpreter::FlowOrError::Flow(
                            crate::interpreter::Flow::Return(v),
                        )) => self.push(v),
                        Err(crate::interpreter::FlowOrError::Error(e)) => {
                            if let (Some(p), Some(t0)) = (&mut self.interp.profiler, t0) {
                                p.exit_sub(t0.elapsed());
                            }
                            return Err(e);
                        }
                        Err(_) => self.push(PerlValue::UNDEF),
                    }
                    if let (Some(p), Some(t0)) = (&mut self.interp.profiler, t0) {
                        p.exit_sub(t0.elapsed());
                    }
                } else if let Some(result) = self.interp.try_autoload_call(
                    name,
                    self.interp.with_topic_default_args(args.clone()),
                    self.line(),
                    want,
                    None,
                ) {
                    let t0 = self.interp.profiler.is_some().then(std::time::Instant::now);
                    if let Some(p) = &mut self.interp.profiler {
                        p.enter_sub(name);
                    }
                    match result {
                        Ok(v) => self.push(v),
                        Err(crate::interpreter::FlowOrError::Flow(
                            crate::interpreter::Flow::Return(v),
                        )) => self.push(v),
                        Err(crate::interpreter::FlowOrError::Error(e)) => {
                            if let (Some(p), Some(t0)) = (&mut self.interp.profiler, t0) {
                                p.exit_sub(t0.elapsed());
                            }
                            return Err(e);
                        }
                        Err(_) => self.push(PerlValue::UNDEF),
                    }
                    if let (Some(p), Some(t0)) = (&mut self.interp.profiler, t0) {
                        p.exit_sub(t0.elapsed());
                    }
                } else if let Some(def) = self.interp.struct_defs.get(name).cloned() {
                    // Struct constructor: Point(x => 1, y => 2) or Point(1, 2)
                    let result = self.interp.struct_construct(&def, args, self.line());
                    match result {
                        Ok(v) => self.push(v),
                        Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                        _ => self.push(PerlValue::UNDEF),
                    }
                } else if let Some(def) = self.interp.class_defs.get(name).cloned() {
                    // Class constructor: Dog(name => "Rex") or Dog("Rex", 5)
                    let result = self.interp.class_construct(&def, args, self.line());
                    match result {
                        Ok(v) => self.push(v),
                        Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                        _ => self.push(PerlValue::UNDEF),
                    }
                } else if let Some((prefix, suffix)) = name.rsplit_once("::") {
                    // Enum variant constructor: Color::Red or Maybe::Some(value)
                    if let Some(def) = self.interp.enum_defs.get(prefix).cloned() {
                        let result = self.interp.enum_construct(&def, suffix, args, self.line());
                        match result {
                            Ok(v) => self.push(v),
                            Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                            _ => self.push(PerlValue::UNDEF),
                        }
                    // Static class method: Math::add(...)
                    } else if let Some(def) = self.interp.class_defs.get(prefix).cloned() {
                        if let Some(m) = def.method(suffix) {
                            if m.is_static {
                                if let Some(ref body) = m.body {
                                    let params = m.params.clone();
                                    match self.interp.call_static_class_method(
                                        body,
                                        &params,
                                        args.clone(),
                                        self.line(),
                                    ) {
                                        Ok(v) => self.push(v),
                                        Err(crate::interpreter::FlowOrError::Error(e)) => {
                                            return Err(e)
                                        }
                                        Err(crate::interpreter::FlowOrError::Flow(
                                            crate::interpreter::Flow::Return(v),
                                        )) => self.push(v),
                                        _ => self.push(PerlValue::UNDEF),
                                    }
                                } else {
                                    self.push(PerlValue::UNDEF);
                                }
                            } else {
                                return Err(PerlError::runtime(
                                    format!("method `{}` is not static", suffix),
                                    self.line(),
                                ));
                            }
                        } else if def.static_fields.iter().any(|sf| sf.name == suffix) {
                            // Static field access: getter (0 args) or setter (1 arg)
                            let key = format!("{}::{}", prefix, suffix);
                            match args.len() {
                                0 => {
                                    let val = self.interp.scope.get_scalar(&key);
                                    self.push(val);
                                }
                                1 => {
                                    let _ = self.interp.scope.set_scalar(&key, args[0].clone());
                                    self.push(args[0].clone());
                                }
                                _ => {
                                    return Err(PerlError::runtime(
                                        format!(
                                            "static field `{}::{}` takes 0 or 1 arguments",
                                            prefix, suffix
                                        ),
                                        self.line(),
                                    ));
                                }
                            }
                        } else {
                            return Err(PerlError::runtime(
                                self.interp.undefined_subroutine_call_message(name),
                                self.line(),
                            ));
                        }
                    } else {
                        return Err(PerlError::runtime(
                            self.interp.undefined_subroutine_call_message(name),
                            self.line(),
                        ));
                    }
                } else {
                    return Err(PerlError::runtime(
                        self.interp.undefined_subroutine_call_message(name),
                        self.line(),
                    ));
                }
            }
        }
        Ok(())
    }

    #[inline]
    fn push_binop_with_overload<F>(
        &mut self,
        op: BinOp,
        a: PerlValue,
        b: PerlValue,
        default: F,
    ) -> PerlResult<()>
    where
        F: FnOnce(&PerlValue, &PerlValue) -> PerlResult<PerlValue>,
    {
        let line = self.line();
        if let Some(exec_res) = self.interp.try_overload_binop(op, &a, &b, line) {
            self.push(vm_interp_result(exec_res, line)?);
        } else {
            self.push(default(&a, &b)?);
        }
        Ok(())
    }

    pub(crate) fn concat_stack_values(
        &mut self,
        a: PerlValue,
        b: PerlValue,
    ) -> PerlResult<PerlValue> {
        let line = self.line();
        if let Some(exec_res) = self.interp.try_overload_binop(BinOp::Concat, &a, &b, line) {
            vm_interp_result(exec_res, line)
        } else {
            let sa = match self.interp.stringify_value(a, line) {
                Ok(s) => s,
                Err(FlowOrError::Error(e)) => return Err(e),
                Err(FlowOrError::Flow(_)) => {
                    return Err(PerlError::runtime("concat: unexpected control flow", line));
                }
            };
            let sb = match self.interp.stringify_value(b, line) {
                Ok(s) => s,
                Err(FlowOrError::Error(e)) => return Err(e),
                Err(FlowOrError::Flow(_)) => {
                    return Err(PerlError::runtime("concat: unexpected control flow", line));
                }
            };
            let mut s = sa;
            s.push_str(&sb);
            Ok(PerlValue::string(s))
        }
    }

    fn run_main_dispatch_loop(
        &mut self,
        mut last: PerlValue,
        op_count: &mut u64,
        init_dispatch: bool,
    ) -> PerlResult<PerlValue> {
        if init_dispatch {
            self.halt = false;
            self.exit_main_dispatch = false;
            self.exit_main_dispatch_value = None;
        }
        let ops_ref: &Vec<Op> = &self.ops;
        let ops = ops_ref as *const Vec<Op>;
        let ops = unsafe { &*ops };
        let names_ref: &Vec<String> = &self.names;
        let names = names_ref as *const Vec<String>;
        let names = unsafe { &*names };
        let constants_ref: &Vec<PerlValue> = &self.constants;
        let constants = constants_ref as *const Vec<PerlValue>;
        let constants = unsafe { &*constants };
        let len = ops.len();
        const MAX_OPS: u64 = 1_000_000_000;
        loop {
            if self.jit_trampoline_depth > 0 && self.jit_trampoline_out.is_some() {
                break;
            }
            if self.block_region_return.is_some() {
                break;
            }
            if self.block_region_mode && self.ip >= self.block_region_end {
                return Err(PerlError::runtime(
                    "block bytecode region fell through without BlockReturnValue",
                    self.line(),
                ));
            }
            if self.ip >= len {
                break;
            }

            if !self.block_region_mode
                && self.jit_enabled
                && self.sub_entry_at_ip.get(self.ip).copied().unwrap_or(false)
            {
                let sub_ip = self.ip;
                if sub_ip >= self.sub_entry_invoke_count.len() {
                    self.sub_entry_invoke_count.resize(sub_ip + 1, 0);
                }
                let c = &mut self.sub_entry_invoke_count[sub_ip];
                if *c <= self.jit_sub_invoke_threshold {
                    *c = c.saturating_add(1);
                }
                let should_try_jit = *c > self.jit_sub_invoke_threshold
                    && (!self.sub_jit_skip_linear_test(sub_ip)
                        || !self.sub_jit_skip_block_test(sub_ip));
                if should_try_jit {
                    if !self.sub_jit_skip_linear_test(sub_ip) && self.try_jit_subroutine_linear()? {
                        continue;
                    }
                    if !self.sub_jit_skip_block_test(sub_ip) && self.try_jit_subroutine_block()? {
                        continue;
                    }
                }
            }

            *op_count += 1;
            // `%SIG` delivery and the execution cap: same cadence as the old per-op poll (signals
            // remain responsive; hot loops avoid a syscall/atomic path every opcode).
            if (*op_count & 0x3FF) == 0 {
                crate::perl_signal::poll(self.interp)?;
                if *op_count > MAX_OPS {
                    return Err(PerlError::runtime(
                        "VM execution limit exceeded (possible infinite loop)",
                        self.line(),
                    ));
                }
            }

            let ip_before = self.ip;
            let line = self.lines.get(ip_before).copied().unwrap_or(0);
            let op = &ops[self.ip];
            self.ip += 1;

            // Debugger hook: check if we should stop at this line
            if let Some(ref mut dbg) = self.interp.debugger {
                if dbg.should_stop(line) {
                    let call_stack = self.interp.debug_call_stack.clone();
                    match dbg.prompt(line, &self.interp.scope, &call_stack) {
                        crate::debugger::DebugAction::Quit => {
                            return Err(PerlError::runtime("debugger: quit", line));
                        }
                        crate::debugger::DebugAction::Continue => {}
                        crate::debugger::DebugAction::Prompt => {}
                    }
                }
            }

            let op_prof_t0 = self.interp.profiler.is_some().then(std::time::Instant::now);
            // Closure: `?` / `return Err` inside `match op` must not return from
            // `run_main_dispatch_loop` — they must become `__op_res` so `try_recover_from_exception`
            // can run before propagating.
            let __op_res: PerlResult<()> = (|| -> PerlResult<()> {
                match op {
                    Op::Nop => Ok(()),
                    // ── Constants ──
                    Op::LoadInt(n) => {
                        self.push(PerlValue::integer(*n));
                        Ok(())
                    }
                    Op::LoadFloat(f) => {
                        self.push(PerlValue::float(*f));
                        Ok(())
                    }
                    Op::LoadConst(idx) => {
                        self.push(self.constant(*idx).clone());
                        Ok(())
                    }
                    Op::LoadUndef => {
                        self.push(PerlValue::UNDEF);
                        Ok(())
                    }
                    Op::RuntimeErrorConst(idx) => {
                        let msg = self.constant(*idx).to_string();
                        let line = self.line();
                        Err(crate::error::PerlError::runtime(msg, line))
                    }
                    Op::BarewordRvalue(name_idx) => {
                        let name = names[*name_idx as usize].clone();
                        let line = self.line();
                        let out = vm_interp_result(
                            self.interp.resolve_bareword_rvalue(
                                &name,
                                crate::interpreter::WantarrayCtx::Scalar,
                                line,
                            ),
                            line,
                        )?;
                        self.push(out);
                        Ok(())
                    }

                    // ── Stack ──
                    Op::Pop => {
                        let v = self.pop();
                        // Drain iterators used as void statements so side effects fire.
                        if v.is_iterator() {
                            let iter = v.into_iterator();
                            while iter.next_item().is_some() {}
                        }
                        Ok(())
                    }
                    Op::Dup => {
                        let v = self.peek().dup_stack();
                        self.push(v);
                        Ok(())
                    }
                    Op::Dup2 => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(a.dup_stack());
                        self.push(b.dup_stack());
                        self.push(a);
                        self.push(b);
                        Ok(())
                    }
                    Op::Swap => {
                        let top = self.pop();
                        let below = self.pop();
                        self.push(top);
                        self.push(below);
                        Ok(())
                    }
                    Op::Rot => {
                        let c = self.pop();
                        let b = self.pop();
                        let a = self.pop();
                        self.push(b);
                        self.push(c);
                        self.push(a);
                        Ok(())
                    }
                    Op::ValueScalarContext => {
                        let v = self.pop();
                        self.push(v.scalar_context());
                        Ok(())
                    }

                    // ── Scalars ──
                    Op::GetScalar(idx) => {
                        let n = names[*idx as usize].as_str();
                        let val = self.interp.get_special_var(n);
                        self.push(val);
                        Ok(())
                    }
                    Op::GetScalarPlain(idx) => {
                        let n = names[*idx as usize].as_str();
                        let val = self.interp.scope.get_scalar(n);
                        self.push(val);
                        Ok(())
                    }
                    Op::SetScalar(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        self.interp.maybe_invalidate_regex_capture_memo(n);
                        self.interp
                            .set_special_var(n, &val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::SetScalarPlain(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        self.interp.maybe_invalidate_regex_capture_memo(n);
                        self.interp
                            .scope
                            .set_scalar(n, val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::SetScalarKeep(idx) => {
                        let val = self.peek().dup_stack();
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        self.interp.maybe_invalidate_regex_capture_memo(n);
                        self.interp
                            .set_special_var(n, &val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::SetScalarKeepPlain(idx) => {
                        let val = self.peek().dup_stack();
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        self.interp.maybe_invalidate_regex_capture_memo(n);
                        self.interp
                            .scope
                            .set_scalar(n, val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::DeclareScalar(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.interp
                            .scope
                            .declare_scalar_frozen(n, val, false, None)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::DeclareScalarFrozen(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.interp
                            .scope
                            .declare_scalar_frozen(n, val, true, None)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::DeclareScalarTyped(idx, tyb) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        let ty = PerlTypeName::from_byte(*tyb).ok_or_else(|| {
                            PerlError::runtime(
                                format!("invalid typed scalar type byte {}", tyb),
                                self.line(),
                            )
                        })?;
                        self.interp
                            .scope
                            .declare_scalar_frozen(n, val, false, Some(ty))
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::DeclareScalarTypedFrozen(idx, tyb) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        let ty = PerlTypeName::from_byte(*tyb).ok_or_else(|| {
                            PerlError::runtime(
                                format!("invalid typed scalar type byte {}", tyb),
                                self.line(),
                            )
                        })?;
                        self.interp
                            .scope
                            .declare_scalar_frozen(n, val, true, Some(ty))
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }

                    // ── Arrays ──
                    Op::GetArray(idx) => {
                        let n = names[*idx as usize].as_str();
                        let arr = self.interp.scope.get_array(n);
                        self.push(PerlValue::array(arr));
                        Ok(())
                    }
                    Op::SetArray(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.require_array_mutable(n)?;
                        self.interp
                            .scope
                            .set_array(n, val.to_list())
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::DeclareArray(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.interp.scope.declare_array(n, val.to_list());
                        Ok(())
                    }
                    Op::DeclareArrayFrozen(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.interp
                            .scope
                            .declare_array_frozen(n, val.to_list(), true);
                        Ok(())
                    }
                    Op::GetArrayElem(idx) => {
                        let index = self.pop().to_int();
                        let n = names[*idx as usize].as_str();
                        let val = self.interp.scope.get_array_element(n, index);
                        self.push(val);
                        Ok(())
                    }
                    Op::ExistsArrayElem(idx) => {
                        let index = self.pop().to_int();
                        let n = names[*idx as usize].as_str();
                        let yes = self.interp.scope.exists_array_element(n, index);
                        self.push(PerlValue::integer(if yes { 1 } else { 0 }));
                        Ok(())
                    }
                    Op::DeleteArrayElem(idx) => {
                        let index = self.pop().to_int();
                        let n = names[*idx as usize].as_str();
                        self.require_array_mutable(n)?;
                        let v = self
                            .interp
                            .scope
                            .delete_array_element(n, index)
                            .map_err(|e| e.at_line(self.line()))?;
                        self.push(v);
                        Ok(())
                    }
                    Op::SetArrayElem(idx) => {
                        let index = self.pop().to_int();
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.require_array_mutable(n)?;
                        self.interp
                            .scope
                            .set_array_element(n, index, val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::SetArrayElemKeep(idx) => {
                        let index = self.pop().to_int();
                        let val = self.pop();
                        let val_keep = val.clone();
                        let n = names[*idx as usize].as_str();
                        self.require_array_mutable(n)?;
                        let line = self.line();
                        self.interp
                            .scope
                            .set_array_element(n, index, val)
                            .map_err(|e| e.at_line(line))?;
                        self.push(val_keep);
                        Ok(())
                    }
                    Op::PushArray(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.require_array_mutable(n)?;
                        let line = self.line();
                        if let Some(items) = val.as_array_vec() {
                            for item in items {
                                self.interp
                                    .scope
                                    .push_to_array(n, item)
                                    .map_err(|e| e.at_line(line))?;
                            }
                        } else {
                            self.interp
                                .scope
                                .push_to_array(n, val)
                                .map_err(|e| e.at_line(line))?;
                        }
                        Ok(())
                    }
                    Op::PopArray(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.require_array_mutable(n)?;
                        let line = self.line();
                        let val = self
                            .interp
                            .scope
                            .pop_from_array(n)
                            .map_err(|e| e.at_line(line))?;
                        self.push(val);
                        Ok(())
                    }
                    Op::ShiftArray(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.require_array_mutable(n)?;
                        let line = self.line();
                        let val = self
                            .interp
                            .scope
                            .shift_from_array(n)
                            .map_err(|e| e.at_line(line))?;
                        self.push(val);
                        Ok(())
                    }
                    Op::PushArrayDeref => {
                        let val = self.pop();
                        let r = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp
                                .push_array_deref_value(r.clone(), val, line)
                                .map(|_| PerlValue::UNDEF),
                            line,
                        )?;
                        self.push(r);
                        Ok(())
                    }
                    Op::ArrayDerefLen => {
                        let r = self.pop();
                        let line = self.line();
                        let n = match self.interp.array_deref_len(r, line) {
                            Ok(n) => n,
                            Err(FlowOrError::Error(e)) => return Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                return Err(PerlError::runtime(
                                    "unexpected flow in tree-assisted opcode",
                                    line,
                                ));
                            }
                        };
                        self.push(PerlValue::integer(n));
                        Ok(())
                    }
                    Op::PopArrayDeref => {
                        let r = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(self.interp.pop_array_deref(r, line), line)?;
                        self.push(v);
                        Ok(())
                    }
                    Op::ShiftArrayDeref => {
                        let r = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(self.interp.shift_array_deref(r, line), line)?;
                        self.push(v);
                        Ok(())
                    }
                    Op::UnshiftArrayDeref(n_extra) => {
                        let n = *n_extra as usize;
                        let mut vals: Vec<PerlValue> = Vec::with_capacity(n);
                        for _ in 0..n {
                            vals.push(self.pop());
                        }
                        vals.reverse();
                        let r = self.pop();
                        let line = self.line();
                        let len = match self.interp.unshift_array_deref_multi(r, vals, line) {
                            Ok(n) => n,
                            Err(FlowOrError::Error(e)) => return Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                return Err(PerlError::runtime(
                                    "unexpected flow in tree-assisted opcode",
                                    line,
                                ));
                            }
                        };
                        self.push(PerlValue::integer(len));
                        Ok(())
                    }
                    Op::SpliceArrayDeref(n_rep) => {
                        let n = *n_rep as usize;
                        let mut rep_vals: Vec<PerlValue> = Vec::with_capacity(n);
                        for _ in 0..n {
                            rep_vals.push(self.pop());
                        }
                        rep_vals.reverse();
                        let length_val = self.pop();
                        let offset_val = self.pop();
                        let aref = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(
                            self.interp
                                .splice_array_deref(aref, offset_val, length_val, rep_vals, line),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::ArrayLen(idx) => {
                        let len = self.interp.scope.array_len(&self.names[*idx as usize]);
                        self.push(PerlValue::integer(len as i64));
                        Ok(())
                    }
                    Op::ArraySlicePart(idx) => {
                        let spec = self.pop();
                        let n = names[*idx as usize].as_str();
                        let mut out = Vec::new();
                        if let Some(indices) = spec.as_array_vec() {
                            for pv in indices {
                                out.push(self.interp.scope.get_array_element(n, pv.to_int()));
                            }
                        } else {
                            out.push(self.interp.scope.get_array_element(n, spec.to_int()));
                        }
                        self.push(PerlValue::array(out));
                        Ok(())
                    }
                    Op::ArrayConcatTwo => {
                        let b = self.pop();
                        let a = self.pop();
                        let mut av = a.as_array_vec().unwrap_or_else(|| vec![a]);
                        let bv = b.as_array_vec().unwrap_or_else(|| vec![b]);
                        av.extend(bv);
                        self.push(PerlValue::array(av));
                        Ok(())
                    }

                    // ── Hashes ──
                    Op::GetHash(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.interp.touch_env_hash(n);
                        let h = self.interp.scope.get_hash(n);
                        self.push(PerlValue::hash(h));
                        Ok(())
                    }
                    Op::SetHash(idx) => {
                        let val = self.pop();
                        let items = val.to_list();
                        let mut map = IndexMap::new();
                        let mut i = 0;
                        while i + 1 < items.len() {
                            map.insert(items[i].to_string(), items[i + 1].clone());
                            i += 2;
                        }
                        let n = names[*idx as usize].as_str();
                        self.require_hash_mutable(n)?;
                        self.interp
                            .scope
                            .set_hash(n, map)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::DeclareHash(idx) => {
                        let val = self.pop();
                        let items = val.to_list();
                        let mut map = IndexMap::new();
                        let mut i = 0;
                        while i + 1 < items.len() {
                            map.insert(items[i].to_string(), items[i + 1].clone());
                            i += 2;
                        }
                        let n = names[*idx as usize].as_str();
                        self.interp.scope.declare_hash(n, map);
                        Ok(())
                    }
                    Op::DeclareHashFrozen(idx) => {
                        let val = self.pop();
                        let items = val.to_list();
                        let mut map = IndexMap::new();
                        let mut i = 0;
                        while i + 1 < items.len() {
                            map.insert(items[i].to_string(), items[i + 1].clone());
                            i += 2;
                        }
                        let n = names[*idx as usize].as_str();
                        self.interp.scope.declare_hash_frozen(n, map, true);
                        Ok(())
                    }
                    Op::LocalDeclareScalar(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        // `local $X` on a special var (`$/`, `$\`, `$,`, `$"`, …) — see
                        // tree-interpreter's `StmtKind::Local` handler. Save prior value to
                        // the interpreter's `special_var_restore_frames` so `scope_pop_hook`
                        // restores the backing field on block exit.
                        if Interpreter::is_special_scalar_name_for_set(n) {
                            let old = self.interp.get_special_var(n);
                            if let Some(frame) = self.interp.special_var_restore_frames.last_mut() {
                                frame.push((n.to_string(), old));
                            }
                            let line = self.line();
                            self.interp
                                .set_special_var(n, &val)
                                .map_err(|e| e.at_line(line))?;
                        }
                        self.interp
                            .scope
                            .local_set_scalar(n, val.clone())
                            .map_err(|e| e.at_line(self.line()))?;
                        self.push(val);
                        Ok(())
                    }
                    Op::LocalDeclareArray(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.interp
                            .scope
                            .local_set_array(n, val.to_list())
                            .map_err(|e| e.at_line(self.line()))?;
                        self.push(val);
                        Ok(())
                    }
                    Op::LocalDeclareHash(idx) => {
                        let val = self.pop();
                        let items = val.to_list();
                        let mut map = IndexMap::new();
                        let mut i = 0;
                        while i + 1 < items.len() {
                            map.insert(items[i].to_string(), items[i + 1].clone());
                            i += 2;
                        }
                        let n = names[*idx as usize].as_str();
                        self.interp.touch_env_hash(n);
                        self.interp
                            .scope
                            .local_set_hash(n, map)
                            .map_err(|e| e.at_line(self.line()))?;
                        self.push(val);
                        Ok(())
                    }
                    Op::LocalDeclareHashElement(idx) => {
                        let key = self.pop().to_string();
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.interp.touch_env_hash(n);
                        self.interp
                            .scope
                            .local_set_hash_element(n, key.as_str(), val.clone())
                            .map_err(|e| e.at_line(self.line()))?;
                        self.push(val);
                        Ok(())
                    }
                    Op::LocalDeclareArrayElement(idx) => {
                        let index = self.pop().to_int();
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.require_array_mutable(n)?;
                        self.interp
                            .scope
                            .local_set_array_element(n, index, val.clone())
                            .map_err(|e| e.at_line(self.line()))?;
                        self.push(val);
                        Ok(())
                    }
                    Op::LocalDeclareTypeglob(lhs_i, rhs_opt) => {
                        let lhs = names[*lhs_i as usize].as_str();
                        let rhs = rhs_opt.map(|i| names[i as usize].as_str());
                        let line = self.line();
                        self.interp
                            .local_declare_typeglob(lhs, rhs, line)
                            .map_err(|e| e.at_line(line))?;
                        Ok(())
                    }
                    Op::LocalDeclareTypeglobDynamic(rhs_opt) => {
                        let lhs = self.pop().to_string();
                        let rhs = rhs_opt.map(|i| names[i as usize].as_str());
                        let line = self.line();
                        self.interp
                            .local_declare_typeglob(lhs.as_str(), rhs, line)
                            .map_err(|e| e.at_line(line))?;
                        Ok(())
                    }
                    Op::GetHashElem(idx) => {
                        let key = self.pop().to_string();
                        let n = names[*idx as usize].as_str();
                        self.interp.touch_env_hash(n);
                        let val = self.interp.scope.get_hash_element(n, &key);
                        self.push(val);
                        Ok(())
                    }
                    Op::SetHashElem(idx) => {
                        let key = self.pop().to_string();
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.require_hash_mutable(n)?;
                        self.interp.touch_env_hash(n);
                        self.interp
                            .scope
                            .set_hash_element(n, &key, val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::SetHashElemKeep(idx) => {
                        let key = self.pop().to_string();
                        let val = self.pop();
                        let val_keep = val.clone();
                        let n = names[*idx as usize].as_str();
                        self.require_hash_mutable(n)?;
                        self.interp.touch_env_hash(n);
                        let line = self.line();
                        self.interp
                            .scope
                            .set_hash_element(n, &key, val)
                            .map_err(|e| e.at_line(line))?;
                        self.push(val_keep);
                        Ok(())
                    }
                    Op::DeleteHashElem(idx) => {
                        let key = self.pop().to_string();
                        let n = names[*idx as usize].as_str();
                        self.require_hash_mutable(n)?;
                        self.interp.touch_env_hash(n);
                        if let Some(obj) = self.interp.tied_hashes.get(n).cloned() {
                            let class = obj
                                .as_blessed_ref()
                                .map(|b| b.class.clone())
                                .unwrap_or_default();
                            let full = format!("{}::DELETE", class);
                            if let Some(sub) = self.interp.subs.get(&full).cloned() {
                                let line = self.line();
                                let v = vm_interp_result(
                                    self.interp.call_sub(
                                        &sub,
                                        vec![obj, PerlValue::string(key)],
                                        WantarrayCtx::Scalar,
                                        line,
                                    ),
                                    line,
                                )?;
                                self.push(v);
                                return Ok(());
                            }
                        }
                        let val = self
                            .interp
                            .scope
                            .delete_hash_element(n, &key)
                            .map_err(|e| e.at_line(self.line()))?;
                        self.push(val);
                        Ok(())
                    }
                    Op::ExistsHashElem(idx) => {
                        let key = self.pop().to_string();
                        let n = names[*idx as usize].as_str();
                        self.interp.touch_env_hash(n);
                        if let Some(obj) = self.interp.tied_hashes.get(n).cloned() {
                            let class = obj
                                .as_blessed_ref()
                                .map(|b| b.class.clone())
                                .unwrap_or_default();
                            let full = format!("{}::EXISTS", class);
                            if let Some(sub) = self.interp.subs.get(&full).cloned() {
                                let line = self.line();
                                let v = vm_interp_result(
                                    self.interp.call_sub(
                                        &sub,
                                        vec![obj, PerlValue::string(key)],
                                        WantarrayCtx::Scalar,
                                        line,
                                    ),
                                    line,
                                )?;
                                self.push(v);
                                return Ok(());
                            }
                        }
                        let exists = self.interp.scope.exists_hash_element(n, &key);
                        self.push(PerlValue::integer(if exists { 1 } else { 0 }));
                        Ok(())
                    }
                    Op::ExistsArrowHashElem => {
                        let key = self.pop().to_string();
                        let container = self.pop();
                        let line = self.line();
                        let yes = vm_interp_result(
                            self.interp
                                .exists_arrow_hash_element(container, &key, line)
                                .map(|b| PerlValue::integer(if b { 1 } else { 0 }))
                                .map_err(FlowOrError::Error),
                            line,
                        )?;
                        self.push(yes);
                        Ok(())
                    }
                    Op::DeleteArrowHashElem => {
                        let key = self.pop().to_string();
                        let container = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(
                            self.interp
                                .delete_arrow_hash_element(container, &key, line)
                                .map_err(FlowOrError::Error),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::ExistsArrowArrayElem => {
                        let idx = self.pop().to_int();
                        let container = self.pop();
                        let line = self.line();
                        let yes = vm_interp_result(
                            self.interp
                                .exists_arrow_array_element(container, idx, line)
                                .map(|b| PerlValue::integer(if b { 1 } else { 0 }))
                                .map_err(FlowOrError::Error),
                            line,
                        )?;
                        self.push(yes);
                        Ok(())
                    }
                    Op::DeleteArrowArrayElem => {
                        let idx = self.pop().to_int();
                        let container = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(
                            self.interp
                                .delete_arrow_array_element(container, idx, line)
                                .map_err(FlowOrError::Error),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::HashKeys(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.interp.touch_env_hash(n);
                        let h = self.interp.scope.get_hash(n);
                        let keys: Vec<PerlValue> =
                            h.keys().map(|k| PerlValue::string(k.clone())).collect();
                        self.push(PerlValue::array(keys));
                        Ok(())
                    }
                    Op::HashKeysScalar(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.interp.touch_env_hash(n);
                        let h = self.interp.scope.get_hash(n);
                        self.push(PerlValue::integer(h.len() as i64));
                        Ok(())
                    }
                    Op::HashValues(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.interp.touch_env_hash(n);
                        let h = self.interp.scope.get_hash(n);
                        let vals: Vec<PerlValue> = h.values().cloned().collect();
                        self.push(PerlValue::array(vals));
                        Ok(())
                    }
                    Op::HashValuesScalar(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.interp.touch_env_hash(n);
                        let h = self.interp.scope.get_hash(n);
                        self.push(PerlValue::integer(h.len() as i64));
                        Ok(())
                    }
                    Op::KeysFromValue => {
                        let val = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(Interpreter::keys_from_value(val, line), line)?;
                        self.push(v);
                        Ok(())
                    }
                    Op::KeysFromValueScalar => {
                        let val = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(Interpreter::keys_from_value(val, line), line)?;
                        let n = v.as_array_vec().map(|a| a.len()).unwrap_or(0) as i64;
                        self.push(PerlValue::integer(n));
                        Ok(())
                    }
                    Op::ValuesFromValue => {
                        let val = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(Interpreter::values_from_value(val, line), line)?;
                        self.push(v);
                        Ok(())
                    }
                    Op::ValuesFromValueScalar => {
                        let val = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(Interpreter::values_from_value(val, line), line)?;
                        let n = v.as_array_vec().map(|a| a.len()).unwrap_or(0) as i64;
                        self.push(PerlValue::integer(n));
                        Ok(())
                    }

                    // ── Arithmetic (integer fast paths) ──
                    Op::Add => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::Add, a, b, |a, b| {
                            Ok(
                                if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                    PerlValue::integer(x.wrapping_add(y))
                                } else {
                                    PerlValue::float(a.to_number() + b.to_number())
                                },
                            )
                        })
                    }
                    Op::Sub => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::Sub, a, b, |a, b| {
                            Ok(
                                if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                    PerlValue::integer(x.wrapping_sub(y))
                                } else {
                                    PerlValue::float(a.to_number() - b.to_number())
                                },
                            )
                        })
                    }
                    Op::Mul => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::Mul, a, b, |a, b| {
                            Ok(
                                if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                    PerlValue::integer(x.wrapping_mul(y))
                                } else {
                                    PerlValue::float(a.to_number() * b.to_number())
                                },
                            )
                        })
                    }
                    Op::Div => {
                        let b = self.pop();
                        let a = self.pop();
                        let line = self.line();
                        self.push_binop_with_overload(BinOp::Div, a, b, |a, b| {
                            if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                if y == 0 {
                                    return Err(PerlError::runtime(
                                        "Illegal division by zero",
                                        line,
                                    ));
                                }
                                Ok(if x % y == 0 {
                                    PerlValue::integer(x / y)
                                } else {
                                    PerlValue::float(x as f64 / y as f64)
                                })
                            } else {
                                let d = b.to_number();
                                if d == 0.0 {
                                    return Err(PerlError::runtime(
                                        "Illegal division by zero",
                                        line,
                                    ));
                                }
                                Ok(PerlValue::float(a.to_number() / d))
                            }
                        })
                    }
                    Op::Mod => {
                        let b = self.pop();
                        let a = self.pop();
                        let line = self.line();
                        self.push_binop_with_overload(BinOp::Mod, a, b, |a, b| {
                            let b = b.to_int();
                            let a = a.to_int();
                            if b == 0 {
                                return Err(PerlError::runtime("Illegal modulus zero", line));
                            }
                            Ok(PerlValue::integer(a % b))
                        })
                    }
                    Op::Pow => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::Pow, a, b, |a, b| {
                            Ok(
                                if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                    if let Some(r) = (y >= 0)
                                        .then(|| u32::try_from(y).ok())
                                        .flatten()
                                        .and_then(|yu| x.checked_pow(yu))
                                    {
                                        PerlValue::integer(r)
                                    } else {
                                        PerlValue::float(a.to_number().powf(b.to_number()))
                                    }
                                } else {
                                    PerlValue::float(a.to_number().powf(b.to_number()))
                                },
                            )
                        })
                    }
                    Op::Negate => {
                        let a = self.pop();
                        let line = self.line();
                        if let Some(exec_res) =
                            self.interp.try_overload_unary_dispatch("neg", &a, line)
                        {
                            self.push(vm_interp_result(exec_res, line)?);
                        } else {
                            self.push(if let Some(n) = a.as_integer() {
                                PerlValue::integer(-n)
                            } else {
                                PerlValue::float(-a.to_number())
                            });
                        }
                        Ok(())
                    }
                    Op::Inc => {
                        let a = self.pop();
                        self.push(if let Some(n) = a.as_integer() {
                            PerlValue::integer(n.wrapping_add(1))
                        } else {
                            PerlValue::float(a.to_number() + 1.0)
                        });
                        Ok(())
                    }
                    Op::Dec => {
                        let a = self.pop();
                        self.push(if let Some(n) = a.as_integer() {
                            PerlValue::integer(n.wrapping_sub(1))
                        } else {
                            PerlValue::float(a.to_number() - 1.0)
                        });
                        Ok(())
                    }

                    // ── String ──
                    Op::Concat => {
                        let b = self.pop();
                        let a = self.pop();
                        let out = self.concat_stack_values(a, b)?;
                        self.push(out);
                        Ok(())
                    }
                    Op::ArrayStringifyListSep => {
                        let raw = self.pop();
                        let v = self.interp.peel_array_ref_for_list_join(raw);
                        let sep = self.interp.list_separator.clone();
                        let list = v.to_list();
                        let joined = list
                            .iter()
                            .map(|x| x.to_string())
                            .collect::<Vec<_>>()
                            .join(&sep);
                        self.push(PerlValue::string(joined));
                        Ok(())
                    }
                    Op::StringRepeat => {
                        let n = self.pop().to_int().max(0) as usize;
                        let val = self.pop();
                        self.push(PerlValue::string(val.to_string().repeat(n)));
                        Ok(())
                    }
                    Op::ProcessCaseEscapes => {
                        let val = self.pop();
                        let s = val.to_string();
                        let processed = Interpreter::process_case_escapes(&s);
                        self.push(PerlValue::string(processed));
                        Ok(())
                    }

                    // ── Numeric comparison ──
                    Op::NumEq => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::NumEq, a.clone(), b.clone(), |a, b| {
                            // Struct equality: compare all fields
                            if let (Some(sa), Some(sb)) = (a.as_struct_inst(), b.as_struct_inst()) {
                                if sa.def.name != sb.def.name {
                                    return Ok(PerlValue::integer(0));
                                }
                                let av = sa.get_values();
                                let bv = sb.get_values();
                                let eq = av.len() == bv.len()
                                    && av.iter().zip(bv.iter()).all(|(x, y)| x.struct_field_eq(y));
                                Ok(PerlValue::integer(if eq { 1 } else { 0 }))
                            } else {
                                Ok(int_cmp(a, b, |x, y| x == y, |x, y| x == y))
                            }
                        })
                    }
                    Op::NumNe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::NumNe, a, b, |a, b| {
                            Ok(int_cmp(a, b, |x, y| x != y, |x, y| x != y))
                        })
                    }
                    Op::NumLt => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::NumLt, a, b, |a, b| {
                            Ok(int_cmp(a, b, |x, y| x < y, |x, y| x < y))
                        })
                    }
                    Op::NumGt => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::NumGt, a, b, |a, b| {
                            Ok(int_cmp(a, b, |x, y| x > y, |x, y| x > y))
                        })
                    }
                    Op::NumLe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::NumLe, a, b, |a, b| {
                            Ok(int_cmp(a, b, |x, y| x <= y, |x, y| x <= y))
                        })
                    }
                    Op::NumGe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::NumGe, a, b, |a, b| {
                            Ok(int_cmp(a, b, |x, y| x >= y, |x, y| x >= y))
                        })
                    }
                    Op::Spaceship => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::Spaceship, a, b, |a, b| {
                            Ok(
                                if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                    PerlValue::integer(if x < y {
                                        -1
                                    } else if x > y {
                                        1
                                    } else {
                                        0
                                    })
                                } else {
                                    let x = a.to_number();
                                    let y = b.to_number();
                                    PerlValue::integer(if x < y {
                                        -1
                                    } else if x > y {
                                        1
                                    } else {
                                        0
                                    })
                                },
                            )
                        })
                    }

                    // ── String comparison ──
                    Op::StrEq => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::StrEq, a, b, |a, b| {
                            Ok(PerlValue::integer(if a.str_eq(b) { 1 } else { 0 }))
                        })
                    }
                    Op::StrNe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::StrNe, a, b, |a, b| {
                            Ok(PerlValue::integer(if !a.str_eq(b) { 1 } else { 0 }))
                        })
                    }
                    Op::StrLt => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::StrLt, a, b, |a, b| {
                            Ok(PerlValue::integer(
                                if a.str_cmp(b) == std::cmp::Ordering::Less {
                                    1
                                } else {
                                    0
                                },
                            ))
                        })
                    }
                    Op::StrGt => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::StrGt, a, b, |a, b| {
                            Ok(PerlValue::integer(
                                if a.str_cmp(b) == std::cmp::Ordering::Greater {
                                    1
                                } else {
                                    0
                                },
                            ))
                        })
                    }
                    Op::StrLe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::StrLe, a, b, |a, b| {
                            let o = a.str_cmp(b);
                            Ok(PerlValue::integer(
                                if matches!(o, std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                                {
                                    1
                                } else {
                                    0
                                },
                            ))
                        })
                    }
                    Op::StrGe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::StrGe, a, b, |a, b| {
                            let o = a.str_cmp(b);
                            Ok(PerlValue::integer(
                                if matches!(
                                    o,
                                    std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
                                ) {
                                    1
                                } else {
                                    0
                                },
                            ))
                        })
                    }
                    Op::StrCmp => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push_binop_with_overload(BinOp::StrCmp, a, b, |a, b| {
                            let cmp = a.str_cmp(b);
                            Ok(PerlValue::integer(match cmp {
                                std::cmp::Ordering::Less => -1,
                                std::cmp::Ordering::Greater => 1,
                                std::cmp::Ordering::Equal => 0,
                            }))
                        })
                    }

                    // ── Logical / Bitwise ──
                    Op::LogNot => {
                        let a = self.pop();
                        let line = self.line();
                        if let Some(exec_res) =
                            self.interp.try_overload_unary_dispatch("bool", &a, line)
                        {
                            let pv = vm_interp_result(exec_res, line)?;
                            self.push(PerlValue::integer(if pv.is_true() { 0 } else { 1 }));
                        } else {
                            self.push(PerlValue::integer(if a.is_true() { 0 } else { 1 }));
                        }
                        Ok(())
                    }
                    Op::BitAnd => {
                        let rv = self.pop();
                        let lv = self.pop();
                        if let Some(s) = crate::value::set_intersection(&lv, &rv) {
                            self.push(s);
                        } else {
                            self.push(PerlValue::integer(lv.to_int() & rv.to_int()));
                        }
                        Ok(())
                    }
                    Op::BitOr => {
                        let rv = self.pop();
                        let lv = self.pop();
                        if let Some(s) = crate::value::set_union(&lv, &rv) {
                            self.push(s);
                        } else {
                            self.push(PerlValue::integer(lv.to_int() | rv.to_int()));
                        }
                        Ok(())
                    }
                    Op::BitXor => {
                        let b = self.pop().to_int();
                        let a = self.pop().to_int();
                        self.push(PerlValue::integer(a ^ b));
                        Ok(())
                    }
                    Op::BitNot => {
                        let a = self.pop().to_int();
                        self.push(PerlValue::integer(!a));
                        Ok(())
                    }
                    Op::Shl => {
                        let b = self.pop().to_int();
                        let a = self.pop().to_int();
                        self.push(PerlValue::integer(a << b));
                        Ok(())
                    }
                    Op::Shr => {
                        let b = self.pop().to_int();
                        let a = self.pop().to_int();
                        self.push(PerlValue::integer(a >> b));
                        Ok(())
                    }

                    // ── Control flow ──
                    Op::Jump(target) => {
                        self.ip = *target;
                        Ok(())
                    }
                    Op::JumpIfTrue(target) => {
                        let val = self.pop();
                        if val.is_true() {
                            self.ip = *target;
                        }
                        Ok(())
                    }
                    Op::JumpIfFalse(target) => {
                        let val = self.pop();
                        if !val.is_true() {
                            self.ip = *target;
                        }
                        Ok(())
                    }
                    Op::JumpIfFalseKeep(target) => {
                        if !self.peek().is_true() {
                            self.ip = *target;
                        } else {
                            self.pop();
                        }
                        Ok(())
                    }
                    Op::JumpIfTrueKeep(target) => {
                        if self.peek().is_true() {
                            self.ip = *target;
                        } else {
                            self.pop();
                        }
                        Ok(())
                    }
                    Op::JumpIfDefinedKeep(target) => {
                        if !self.peek().is_undef() {
                            self.ip = *target;
                        } else {
                            self.pop();
                        }
                        Ok(())
                    }

                    // ── Increment / Decrement ──
                    Op::PreInc(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        let en = self.interp.english_scalar_name(n);
                        let new_val = self
                            .interp
                            .scope
                            .atomic_mutate(en, |v| PerlValue::integer(v.to_int() + 1));
                        self.push(new_val);
                        Ok(())
                    }
                    Op::PreDec(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        let en = self.interp.english_scalar_name(n);
                        let new_val = self
                            .interp
                            .scope
                            .atomic_mutate(en, |v| PerlValue::integer(v.to_int() - 1));
                        self.push(new_val);
                        Ok(())
                    }
                    Op::PostInc(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        let en = self.interp.english_scalar_name(n);
                        if self.ip < len && matches!(ops[self.ip], Op::Pop) {
                            let _ = self
                                .interp
                                .scope
                                .atomic_mutate_post(en, |v| PerlValue::integer(v.to_int() + 1));
                            self.ip += 1;
                        } else {
                            let old = self
                                .interp
                                .scope
                                .atomic_mutate_post(en, |v| PerlValue::integer(v.to_int() + 1));
                            self.push(old);
                        }
                        Ok(())
                    }
                    Op::PostDec(idx) => {
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        let en = self.interp.english_scalar_name(n);
                        if self.ip < len && matches!(ops[self.ip], Op::Pop) {
                            let _ = self
                                .interp
                                .scope
                                .atomic_mutate_post(en, |v| PerlValue::integer(v.to_int() - 1));
                            self.ip += 1;
                        } else {
                            let old = self
                                .interp
                                .scope
                                .atomic_mutate_post(en, |v| PerlValue::integer(v.to_int() - 1));
                            self.push(old);
                        }
                        Ok(())
                    }
                    Op::PreIncSlot(slot) => {
                        let val = self.interp.scope.get_scalar_slot(*slot).to_int() + 1;
                        let new_val = PerlValue::integer(val);
                        self.interp.scope.set_scalar_slot(*slot, new_val.clone());
                        self.push(new_val);
                        Ok(())
                    }
                    Op::PreIncSlotVoid(slot) => {
                        let val = self.interp.scope.get_scalar_slot(*slot).to_int() + 1;
                        self.interp
                            .scope
                            .set_scalar_slot(*slot, PerlValue::integer(val));
                        Ok(())
                    }
                    Op::PreDecSlot(slot) => {
                        let val = self.interp.scope.get_scalar_slot(*slot).to_int() - 1;
                        let new_val = PerlValue::integer(val);
                        self.interp.scope.set_scalar_slot(*slot, new_val.clone());
                        self.push(new_val);
                        Ok(())
                    }
                    Op::PostIncSlot(slot) => {
                        // Fuse PostIncSlot+Pop: if next op discards the old value, skip stack work.
                        if self.ip < len && matches!(ops[self.ip], Op::Pop) {
                            let val = self.interp.scope.get_scalar_slot(*slot).to_int() + 1;
                            self.interp
                                .scope
                                .set_scalar_slot(*slot, PerlValue::integer(val));
                            self.ip += 1; // skip Pop
                        } else {
                            let old = self.interp.scope.get_scalar_slot(*slot);
                            let new_val = PerlValue::integer(old.to_int() + 1);
                            self.interp.scope.set_scalar_slot(*slot, new_val);
                            self.push(old);
                        }
                        Ok(())
                    }
                    Op::PostDecSlot(slot) => {
                        if self.ip < len && matches!(ops[self.ip], Op::Pop) {
                            let val = self.interp.scope.get_scalar_slot(*slot).to_int() - 1;
                            self.interp
                                .scope
                                .set_scalar_slot(*slot, PerlValue::integer(val));
                            self.ip += 1;
                        } else {
                            let old = self.interp.scope.get_scalar_slot(*slot);
                            let new_val = PerlValue::integer(old.to_int() - 1);
                            self.interp.scope.set_scalar_slot(*slot, new_val);
                            self.push(old);
                        }
                        Ok(())
                    }

                    // ── Functions ──
                    Op::Call(name_idx, argc, wa) => {
                        let entry_opt = self.find_sub_entry(*name_idx);
                        self.vm_dispatch_user_call(*name_idx, entry_opt, *argc, *wa, None)?;
                        Ok(())
                    }
                    Op::CallStaticSubId(sid, name_idx, argc, wa) => {
                        let t = self.static_sub_calls.get(*sid as usize).ok_or_else(|| {
                            PerlError::runtime("VM: invalid CallStaticSubId", self.line())
                        })?;
                        debug_assert_eq!(t.2, *name_idx);
                        let closure_sub = self
                            .static_sub_closure_subs
                            .get(*sid as usize)
                            .and_then(|x| x.clone());
                        self.vm_dispatch_user_call(
                            *name_idx,
                            Some((t.0, t.1)),
                            *argc,
                            *wa,
                            closure_sub,
                        )?;
                        Ok(())
                    }
                    Op::Return => {
                        if let Some(frame) = self.call_stack.pop() {
                            if frame.block_region {
                                return Err(PerlError::runtime(
                                    "Return in map/grep/sort block bytecode (use tree interpreter)",
                                    self.line(),
                                ));
                            }
                            if let Some(t0) = frame.sub_profiler_start {
                                if let Some(p) = &mut self.interp.profiler {
                                    p.exit_sub(t0.elapsed());
                                }
                            }
                            self.interp.wantarray_kind = frame.saved_wantarray;
                            self.stack.truncate(frame.stack_base);
                            self.interp.pop_scope_to_depth(frame.scope_depth);
                            self.interp.current_sub_stack.pop();
                            if frame.jit_trampoline_return {
                                self.jit_trampoline_out = Some(PerlValue::UNDEF);
                            } else {
                                self.push(PerlValue::UNDEF);
                                self.ip = frame.return_ip;
                            }
                        } else {
                            self.exit_main_dispatch = true;
                        }
                        Ok(())
                    }
                    Op::ReturnValue => {
                        let val = self.pop();
                        if let Some(frame) = self.call_stack.pop() {
                            if frame.block_region {
                                return Err(PerlError::runtime(
                                    "Return in map/grep/sort block bytecode (use tree interpreter)",
                                    self.line(),
                                ));
                            }
                            if let Some(t0) = frame.sub_profiler_start {
                                if let Some(p) = &mut self.interp.profiler {
                                    p.exit_sub(t0.elapsed());
                                }
                            }
                            self.interp.wantarray_kind = frame.saved_wantarray;
                            self.stack.truncate(frame.stack_base);
                            self.interp.pop_scope_to_depth(frame.scope_depth);
                            self.interp.current_sub_stack.pop();
                            if frame.jit_trampoline_return {
                                self.jit_trampoline_out = Some(val);
                            } else {
                                self.push(val);
                                self.ip = frame.return_ip;
                            }
                        } else {
                            self.exit_main_dispatch_value = Some(val);
                            self.exit_main_dispatch = true;
                        }
                        Ok(())
                    }
                    Op::BlockReturnValue => {
                        let val = self.pop();
                        if let Some(frame) = self.call_stack.pop() {
                            if !frame.block_region {
                                return Err(PerlError::runtime(
                                    "BlockReturnValue without map/grep/sort block frame",
                                    self.line(),
                                ));
                            }
                            self.interp.wantarray_kind = frame.saved_wantarray;
                            self.stack.truncate(frame.stack_base);
                            self.interp.pop_scope_to_depth(frame.scope_depth);
                            self.block_region_return = Some(val);
                            Ok(())
                        } else {
                            Err(PerlError::runtime(
                                "BlockReturnValue with empty call stack",
                                self.line(),
                            ))
                        }
                    }
                    Op::BindSubClosure(name_idx) => {
                        let n = names[*name_idx as usize].as_str();
                        self.interp.rebind_sub_closure(n);
                        Ok(())
                    }

                    // ── Scope ──
                    Op::PushFrame => {
                        self.interp.scope_push_hook();
                        Ok(())
                    }
                    Op::PopFrame => {
                        self.interp.scope_pop_hook();
                        Ok(())
                    }
                    // ── I/O ──
                    Op::Print(handle_idx, argc) => {
                        let argc = *argc as usize;
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc {
                            args.push(self.pop());
                        }
                        args.reverse();
                        let mut output = String::new();
                        if args.is_empty() {
                            let topic = self.interp.scope.get_scalar("_").clone();
                            let s = match self.interp.stringify_value(topic, self.line()) {
                                Ok(s) => s,
                                Err(FlowOrError::Error(e)) => return Err(e),
                                Err(FlowOrError::Flow(_)) => {
                                    return Err(PerlError::runtime(
                                        "print: unexpected control flow",
                                        self.line(),
                                    ));
                                }
                            };
                            output.push_str(&s);
                        } else {
                            for (i, arg) in args.iter().enumerate() {
                                if i > 0 && !self.interp.ofs.is_empty() {
                                    output.push_str(&self.interp.ofs);
                                }
                                for item in arg.to_list() {
                                    let s = match self.interp.stringify_value(item, self.line()) {
                                        Ok(s) => s,
                                        Err(FlowOrError::Error(e)) => return Err(e),
                                        Err(FlowOrError::Flow(_)) => {
                                            return Err(PerlError::runtime(
                                                "print: unexpected control flow",
                                                self.line(),
                                            ));
                                        }
                                    };
                                    output.push_str(&s);
                                }
                            }
                        }
                        output.push_str(&self.interp.ors);
                        let handle_name = match handle_idx {
                            Some(idx) => self.interp.resolve_io_handle_name(
                                self.names
                                    .get(*idx as usize)
                                    .map_or("STDOUT", |s| s.as_str()),
                            ),
                            None => self
                                .interp
                                .resolve_io_handle_name(self.interp.default_print_handle.as_str()),
                        };
                        self.interp.write_formatted_print(
                            handle_name.as_str(),
                            &output,
                            self.line(),
                        )?;
                        self.push(PerlValue::integer(1));
                        Ok(())
                    }
                    Op::Say(handle_idx, argc) => {
                        if (self.interp.feature_bits & crate::interpreter::FEAT_SAY) == 0 {
                            return Err(PerlError::runtime(
                            "say() is disabled (enable with use feature 'say' or use feature ':5.10')",
                            self.line(),
                        ));
                        }
                        let argc = *argc as usize;
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc {
                            args.push(self.pop());
                        }
                        args.reverse();
                        let mut output = String::new();
                        if args.is_empty() {
                            let topic = self.interp.scope.get_scalar("_").clone();
                            let s = match self.interp.stringify_value(topic, self.line()) {
                                Ok(s) => s,
                                Err(FlowOrError::Error(e)) => return Err(e),
                                Err(FlowOrError::Flow(_)) => {
                                    return Err(PerlError::runtime(
                                        "say: unexpected control flow",
                                        self.line(),
                                    ));
                                }
                            };
                            output.push_str(&s);
                        } else {
                            for (i, arg) in args.iter().enumerate() {
                                if i > 0 && !self.interp.ofs.is_empty() {
                                    output.push_str(&self.interp.ofs);
                                }
                                for item in arg.to_list() {
                                    let s = match self.interp.stringify_value(item, self.line()) {
                                        Ok(s) => s,
                                        Err(FlowOrError::Error(e)) => return Err(e),
                                        Err(FlowOrError::Flow(_)) => {
                                            return Err(PerlError::runtime(
                                                "say: unexpected control flow",
                                                self.line(),
                                            ));
                                        }
                                    };
                                    output.push_str(&s);
                                }
                            }
                        }
                        output.push('\n');
                        output.push_str(&self.interp.ors);
                        let handle_name = match handle_idx {
                            Some(idx) => self.interp.resolve_io_handle_name(
                                self.names
                                    .get(*idx as usize)
                                    .map_or("STDOUT", |s| s.as_str()),
                            ),
                            None => self
                                .interp
                                .resolve_io_handle_name(self.interp.default_print_handle.as_str()),
                        };
                        self.interp.write_formatted_print(
                            handle_name.as_str(),
                            &output,
                            self.line(),
                        )?;
                        self.push(PerlValue::integer(1));
                        Ok(())
                    }

                    // ── Built-in dispatch ──
                    Op::CallBuiltin(id, argc) => {
                        let argc = *argc as usize;
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc {
                            args.push(self.pop());
                        }
                        args.reverse();
                        let result = self.exec_builtin(*id, args)?;
                        self.push(result);
                        Ok(())
                    }
                    Op::WantarrayPush(wa) => {
                        self.wantarray_stack.push(self.interp.wantarray_kind);
                        self.interp.wantarray_kind = WantarrayCtx::from_byte(*wa);
                        Ok(())
                    }
                    Op::WantarrayPop => {
                        self.interp.wantarray_kind =
                            self.wantarray_stack.pop().unwrap_or(WantarrayCtx::Scalar);
                        Ok(())
                    }

                    // ── List / Range ──
                    Op::MakeArray(n) => {
                        let n = *n as usize;
                        // Pops are last-to-first on the stack; reverse to source (left-to-right) order,
                        // then flatten nested arrays in place (Perl list literal semantics).
                        let mut stack_vals = Vec::with_capacity(n);
                        for _ in 0..n {
                            stack_vals.push(self.pop());
                        }
                        stack_vals.reverse();
                        let mut arr = Vec::new();
                        for v in stack_vals {
                            if let Some(items) = v.as_array_vec() {
                                arr.extend(items);
                            } else {
                                arr.push(v);
                            }
                        }
                        self.push(PerlValue::array(arr));
                        Ok(())
                    }
                    Op::HashSliceDeref(n) => {
                        let n = *n as usize;
                        let mut key_vals = Vec::with_capacity(n);
                        for _ in 0..n {
                            key_vals.push(self.pop());
                        }
                        key_vals.reverse();
                        let container = self.pop();
                        let line = self.line();
                        let out = vm_interp_result(
                            self.interp
                                .hash_slice_deref_values(&container, &key_vals, line),
                            line,
                        )?;
                        self.push(out);
                        Ok(())
                    }
                    Op::ArrowArraySlice(n) => {
                        let n = *n as usize;
                        let idxs = self.pop_flattened_array_slice_specs(n);
                        let r = self.pop();
                        let line = self.line();
                        let out = vm_interp_result(
                            self.interp.arrow_array_slice_values(r, &idxs, line),
                            line,
                        )?;
                        self.push(out);
                        Ok(())
                    }
                    Op::SetHashSliceDeref(n) => {
                        let n = *n as usize;
                        let mut key_vals = Vec::with_capacity(n);
                        for _ in 0..n {
                            key_vals.push(self.pop());
                        }
                        key_vals.reverse();
                        let container = self.pop();
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp
                                .assign_hash_slice_deref(container, key_vals, val, line),
                            line,
                        )?;
                        Ok(())
                    }
                    Op::SetHashSlice(hash_idx, n) => {
                        let n = *n as usize;
                        let mut key_vals = Vec::with_capacity(n);
                        for _ in 0..n {
                            key_vals.push(self.pop());
                        }
                        key_vals.reverse();
                        let name = names[*hash_idx as usize].as_str();
                        self.require_hash_mutable(name)?;
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp
                                .assign_named_hash_slice(name, key_vals, val, line),
                            line,
                        )?;
                        Ok(())
                    }
                    Op::HashSliceDerefCompound(op_byte, n) => {
                        let n = *n as usize;
                        let mut key_vals = Vec::with_capacity(n);
                        for _ in 0..n {
                            key_vals.push(self.pop());
                        }
                        key_vals.reverse();
                        let container = self.pop();
                        let rhs = self.pop();
                        let line = self.line();
                        let op = crate::compiler::scalar_compound_op_from_byte(*op_byte)
                            .ok_or_else(|| {
                                crate::error::PerlError::runtime(
                                    "VM: HashSliceDerefCompound: bad op byte",
                                    line,
                                )
                            })?;
                        let new_val = vm_interp_result(
                            self.interp.compound_assign_hash_slice_deref(
                                container, key_vals, op, rhs, line,
                            ),
                            line,
                        )?;
                        self.push(new_val);
                        Ok(())
                    }
                    Op::HashSliceDerefIncDec(kind, n) => {
                        let n = *n as usize;
                        let mut key_vals = Vec::with_capacity(n);
                        for _ in 0..n {
                            key_vals.push(self.pop());
                        }
                        key_vals.reverse();
                        let container = self.pop();
                        let line = self.line();
                        let out = vm_interp_result(
                            self.interp
                                .hash_slice_deref_inc_dec(container, key_vals, *kind, line),
                            line,
                        )?;
                        self.push(out);
                        Ok(())
                    }
                    Op::NamedHashSliceCompound(op_byte, hash_idx, n) => {
                        let n = *n as usize;
                        let mut key_vals = Vec::with_capacity(n);
                        for _ in 0..n {
                            key_vals.push(self.pop());
                        }
                        key_vals.reverse();
                        let name = names[*hash_idx as usize].as_str();
                        self.require_hash_mutable(name)?;
                        let rhs = self.pop();
                        let line = self.line();
                        let op = crate::compiler::scalar_compound_op_from_byte(*op_byte)
                            .ok_or_else(|| {
                                crate::error::PerlError::runtime(
                                    "VM: NamedHashSliceCompound: bad op byte",
                                    line,
                                )
                            })?;
                        let new_val = vm_interp_result(
                            self.interp
                                .compound_assign_named_hash_slice(name, key_vals, op, rhs, line),
                            line,
                        )?;
                        self.push(new_val);
                        Ok(())
                    }
                    Op::NamedHashSliceIncDec(kind, hash_idx, n) => {
                        let n = *n as usize;
                        let mut key_vals = Vec::with_capacity(n);
                        for _ in 0..n {
                            key_vals.push(self.pop());
                        }
                        key_vals.reverse();
                        let name = names[*hash_idx as usize].as_str();
                        self.require_hash_mutable(name)?;
                        let line = self.line();
                        let out = vm_interp_result(
                            self.interp
                                .named_hash_slice_inc_dec(name, key_vals, *kind, line),
                            line,
                        )?;
                        self.push(out);
                        Ok(())
                    }
                    Op::NamedHashSlicePeekLast(hash_idx, n) => {
                        let n = *n as usize;
                        let line = self.line();
                        let name = names[*hash_idx as usize].as_str();
                        self.require_hash_mutable(name)?;
                        let len = self.stack.len();
                        if len < n {
                            return Err(PerlError::runtime(
                                "VM: NamedHashSlicePeekLast: stack underflow",
                                line,
                            ));
                        }
                        let base = len - n;
                        let key_vals: Vec<PerlValue> = self.stack[base..base + n].to_vec();
                        let ks = Self::flatten_hash_slice_key_slots(&key_vals);
                        let last_k = ks.last().ok_or_else(|| {
                            PerlError::runtime("VM: NamedHashSlicePeekLast: empty key list", line)
                        })?;
                        self.interp.touch_env_hash(name);
                        let cur = self.interp.scope.get_hash_element(name, last_k.as_str());
                        self.push(cur);
                        Ok(())
                    }
                    Op::NamedHashSliceDropKeysKeepCur(n) => {
                        let n = *n as usize;
                        let cur = self.pop();
                        for _ in 0..n {
                            self.pop();
                        }
                        self.push(cur);
                        Ok(())
                    }
                    Op::SetNamedHashSliceLastKeep(hash_idx, n) => {
                        let n = *n as usize;
                        let line = self.line();
                        let name = names[*hash_idx as usize].as_str();
                        self.require_hash_mutable(name)?;
                        let mut key_vals_rev = Vec::with_capacity(n);
                        for _ in 0..n {
                            key_vals_rev.push(self.pop());
                        }
                        key_vals_rev.reverse();
                        let mut val = self.pop();
                        if let Some(av) = val.as_array_vec() {
                            val = av.last().cloned().unwrap_or(PerlValue::UNDEF);
                        }
                        let ks = Self::flatten_hash_slice_key_slots(&key_vals_rev);
                        let last_k = ks.last().ok_or_else(|| {
                            PerlError::runtime(
                                "VM: SetNamedHashSliceLastKeep: empty key list",
                                line,
                            )
                        })?;
                        let val_keep = val.clone();
                        self.interp.touch_env_hash(name);
                        vm_interp_result(
                            self.interp
                                .scope
                                .set_hash_element(name, last_k.as_str(), val)
                                .map(|()| PerlValue::UNDEF)
                                .map_err(|e| FlowOrError::Error(e.at_line(line))),
                            line,
                        )?;
                        self.push(val_keep);
                        Ok(())
                    }
                    Op::HashSliceDerefPeekLast(n) => {
                        let n = *n as usize;
                        let line = self.line();
                        let len = self.stack.len();
                        if len < n + 1 {
                            return Err(PerlError::runtime(
                                "VM: HashSliceDerefPeekLast: stack underflow",
                                line,
                            ));
                        }
                        let base = len - n - 1;
                        let container = self.stack[base].clone();
                        let key_vals: Vec<PerlValue> = self.stack[base + 1..base + 1 + n].to_vec();
                        let list = vm_interp_result(
                            self.interp
                                .hash_slice_deref_values(&container, &key_vals, line),
                            line,
                        )?;
                        let cur = list.to_list().last().cloned().unwrap_or(PerlValue::UNDEF);
                        self.push(cur);
                        Ok(())
                    }
                    Op::HashSliceDerefRollValUnderKeys(n) => {
                        let n = *n as usize;
                        let val = self.pop();
                        let mut keys_rev = Vec::with_capacity(n);
                        for _ in 0..n {
                            keys_rev.push(self.pop());
                        }
                        let container = self.pop();
                        keys_rev.reverse();
                        self.push(val);
                        self.push(container);
                        for k in keys_rev {
                            self.push(k);
                        }
                        Ok(())
                    }
                    Op::HashSliceDerefSetLastKeep(n) => {
                        let n = *n as usize;
                        let line = self.line();
                        let mut key_vals_rev = Vec::with_capacity(n);
                        for _ in 0..n {
                            key_vals_rev.push(self.pop());
                        }
                        key_vals_rev.reverse();
                        let container = self.pop();
                        let mut val = self.pop();
                        if let Some(av) = val.as_array_vec() {
                            val = av.last().cloned().unwrap_or(PerlValue::UNDEF);
                        }
                        let ks = Self::flatten_hash_slice_key_slots(&key_vals_rev);
                        let last_k = ks.last().ok_or_else(|| {
                            PerlError::runtime(
                                "VM: HashSliceDerefSetLastKeep: empty key list",
                                line,
                            )
                        })?;
                        let val_keep = val.clone();
                        vm_interp_result(
                            self.interp.assign_hash_slice_one_key(
                                container,
                                last_k.as_str(),
                                val,
                                line,
                            ),
                            line,
                        )?;
                        self.push(val_keep);
                        Ok(())
                    }
                    Op::HashSliceDerefDropKeysKeepCur(n) => {
                        let n = *n as usize;
                        let cur = self.pop();
                        for _ in 0..n {
                            self.pop();
                        }
                        let _container = self.pop();
                        self.push(cur);
                        Ok(())
                    }
                    Op::SetArrowArraySlice(n) => {
                        let n = *n as usize;
                        let idxs = self.pop_flattened_array_slice_specs(n);
                        let aref = self.pop();
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_arrow_array_slice(aref, idxs, val, line),
                            line,
                        )?;
                        Ok(())
                    }
                    Op::ArrowArraySliceCompound(op_byte, n) => {
                        let n = *n as usize;
                        let idxs = self.pop_flattened_array_slice_specs(n);
                        let aref = self.pop();
                        let rhs = self.pop();
                        let line = self.line();
                        let op = crate::compiler::scalar_compound_op_from_byte(*op_byte)
                            .ok_or_else(|| {
                                crate::error::PerlError::runtime(
                                    "VM: ArrowArraySliceCompound: bad op byte",
                                    line,
                                )
                            })?;
                        let new_val = vm_interp_result(
                            self.interp
                                .compound_assign_arrow_array_slice(aref, idxs, op, rhs, line),
                            line,
                        )?;
                        self.push(new_val);
                        Ok(())
                    }
                    Op::ArrowArraySliceIncDec(kind, n) => {
                        let n = *n as usize;
                        let idxs = self.pop_flattened_array_slice_specs(n);
                        let aref = self.pop();
                        let line = self.line();
                        let out = vm_interp_result(
                            self.interp
                                .arrow_array_slice_inc_dec(aref, idxs, *kind, line),
                            line,
                        )?;
                        self.push(out);
                        Ok(())
                    }
                    Op::ArrowArraySlicePeekLast(n) => {
                        let n = *n as usize;
                        let line = self.line();
                        let len = self.stack.len();
                        if len < n + 1 {
                            return Err(PerlError::runtime(
                                "VM: ArrowArraySlicePeekLast: stack underflow",
                                line,
                            ));
                        }
                        let base = len - n - 1;
                        let aref = self.stack[base].clone();
                        let idxs =
                            self.flatten_array_slice_specs_ordered_values(&self.stack[base + 1..])?;
                        let last = *idxs.last().ok_or_else(|| {
                            PerlError::runtime(
                                "VM: ArrowArraySlicePeekLast: empty index list",
                                line,
                            )
                        })?;
                        let cur = vm_interp_result(
                            self.interp.read_arrow_array_element(aref, last, line),
                            line,
                        )?;
                        self.push(cur);
                        Ok(())
                    }
                    Op::ArrowArraySliceDropKeysKeepCur(n) => {
                        let n = *n as usize;
                        let cur = self.pop();
                        let _idxs = self.pop_flattened_array_slice_specs(n);
                        let _aref = self.pop();
                        self.push(cur);
                        Ok(())
                    }
                    Op::ArrowArraySliceRollValUnderSpecs(n) => {
                        let n = *n as usize;
                        let val = self.pop();
                        let mut specs_rev = Vec::with_capacity(n);
                        for _ in 0..n {
                            specs_rev.push(self.pop());
                        }
                        let aref = self.pop();
                        self.push(val);
                        self.push(aref);
                        for s in specs_rev.into_iter().rev() {
                            self.push(s);
                        }
                        Ok(())
                    }
                    Op::SetArrowArraySliceLastKeep(n) => {
                        let n = *n as usize;
                        let line = self.line();
                        let idxs = self.pop_flattened_array_slice_specs(n);
                        let aref = self.pop();
                        let mut val = self.pop();
                        // RHS is compiled in list context (`(3,4)` → one array value); Perl assigns
                        // only the **last** list element to the last slice index (`||=` / `&&=` / `//=`).
                        if let Some(av) = val.as_array_vec() {
                            val = av.last().cloned().unwrap_or(PerlValue::UNDEF);
                        }
                        let last = *idxs.last().ok_or_else(|| {
                            PerlError::runtime(
                                "VM: SetArrowArraySliceLastKeep: empty index list",
                                line,
                            )
                        })?;
                        let val_keep = val.clone();
                        vm_interp_result(
                            self.interp.assign_arrow_array_deref(aref, last, val, line),
                            line,
                        )?;
                        self.push(val_keep);
                        Ok(())
                    }
                    Op::NamedArraySliceIncDec(kind, arr_idx, n) => {
                        let n = *n as usize;
                        let idxs = self.pop_flattened_array_slice_specs(n);
                        let name = names[*arr_idx as usize].as_str();
                        self.require_array_mutable(name)?;
                        let line = self.line();
                        let out = vm_interp_result(
                            self.interp
                                .named_array_slice_inc_dec(name, idxs, *kind, line),
                            line,
                        )?;
                        self.push(out);
                        Ok(())
                    }
                    Op::NamedArraySliceCompound(op_byte, arr_idx, n) => {
                        let n = *n as usize;
                        let idxs = self.pop_flattened_array_slice_specs(n);
                        let name = names[*arr_idx as usize].as_str();
                        self.require_array_mutable(name)?;
                        let rhs = self.pop();
                        let line = self.line();
                        let op = crate::compiler::scalar_compound_op_from_byte(*op_byte)
                            .ok_or_else(|| {
                                crate::error::PerlError::runtime(
                                    "VM: NamedArraySliceCompound: bad op byte",
                                    line,
                                )
                            })?;
                        let new_val = vm_interp_result(
                            self.interp
                                .compound_assign_named_array_slice(name, idxs, op, rhs, line),
                            line,
                        )?;
                        self.push(new_val);
                        Ok(())
                    }
                    Op::NamedArraySlicePeekLast(arr_idx, n) => {
                        let n = *n as usize;
                        let line = self.line();
                        let name = names[*arr_idx as usize].as_str();
                        self.require_array_mutable(name)?;
                        let len = self.stack.len();
                        if len < n {
                            return Err(PerlError::runtime(
                                "VM: NamedArraySlicePeekLast: stack underflow",
                                line,
                            ));
                        }
                        let base = len - n;
                        let idxs =
                            self.flatten_array_slice_specs_ordered_values(&self.stack[base..])?;
                        let last = *idxs.last().ok_or_else(|| {
                            PerlError::runtime(
                                "VM: NamedArraySlicePeekLast: empty index list",
                                line,
                            )
                        })?;
                        let cur = self.interp.scope.get_array_element(name, last);
                        self.push(cur);
                        Ok(())
                    }
                    Op::NamedArraySliceDropKeysKeepCur(n) => {
                        let n = *n as usize;
                        let cur = self.pop();
                        let _idxs = self.pop_flattened_array_slice_specs(n);
                        self.push(cur);
                        Ok(())
                    }
                    Op::NamedArraySliceRollValUnderSpecs(n) => {
                        let n = *n as usize;
                        let val = self.pop();
                        let mut specs_rev = Vec::with_capacity(n);
                        for _ in 0..n {
                            specs_rev.push(self.pop());
                        }
                        self.push(val);
                        for s in specs_rev.into_iter().rev() {
                            self.push(s);
                        }
                        Ok(())
                    }
                    Op::SetNamedArraySliceLastKeep(arr_idx, n) => {
                        let n = *n as usize;
                        let line = self.line();
                        let idxs = self.pop_flattened_array_slice_specs(n);
                        let name = names[*arr_idx as usize].as_str();
                        self.require_array_mutable(name)?;
                        let mut val = self.pop();
                        if let Some(av) = val.as_array_vec() {
                            val = av.last().cloned().unwrap_or(PerlValue::UNDEF);
                        }
                        let last = *idxs.last().ok_or_else(|| {
                            PerlError::runtime(
                                "VM: SetNamedArraySliceLastKeep: empty index list",
                                line,
                            )
                        })?;
                        let val_keep = val.clone();
                        vm_interp_result(
                            self.interp
                                .scope
                                .set_array_element(name, last, val)
                                .map(|()| PerlValue::UNDEF)
                                .map_err(|e| FlowOrError::Error(e.at_line(line))),
                            line,
                        )?;
                        self.push(val_keep);
                        Ok(())
                    }
                    Op::SetNamedArraySlice(arr_idx, n) => {
                        let n = *n as usize;
                        let idxs = self.pop_flattened_array_slice_specs(n);
                        let name = names[*arr_idx as usize].as_str();
                        self.require_array_mutable(name)?;
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_named_array_slice(name, idxs, val, line),
                            line,
                        )?;
                        Ok(())
                    }
                    Op::MakeHash(n) => {
                        let n = *n as usize;
                        let mut items = Vec::with_capacity(n);
                        for _ in 0..n {
                            items.push(self.pop());
                        }
                        items.reverse();
                        let mut map = IndexMap::new();
                        let mut i = 0;
                        while i + 1 < items.len() {
                            map.insert(items[i].to_string(), items[i + 1].clone());
                            i += 2;
                        }
                        self.push(PerlValue::hash(map));
                        Ok(())
                    }
                    Op::Range => {
                        let to = self.pop();
                        let from = self.pop();
                        let arr = perl_list_range_expand(from, to);
                        self.push(PerlValue::array(arr));
                        Ok(())
                    }
                    Op::ScalarFlipFlop(slot, exclusive) => {
                        let to = self.pop().to_int();
                        let from = self.pop().to_int();
                        let line = self.line();
                        let v = vm_interp_result(
                            self.interp
                                .scalar_flip_flop_eval(from, to, *slot as usize, *exclusive != 0)
                                .map_err(Into::into),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::RegexFlipFlop(slot, exclusive, lp, lf, rp, rf) => {
                        let line = self.line();
                        let left_pat = constants[*lp as usize].as_str_or_empty();
                        let left_flags = constants[*lf as usize].as_str_or_empty();
                        let right_pat = constants[*rp as usize].as_str_or_empty();
                        let right_flags = constants[*rf as usize].as_str_or_empty();
                        let v = vm_interp_result(
                            self.interp
                                .regex_flip_flop_eval(
                                    left_pat.as_str(),
                                    left_flags.as_str(),
                                    right_pat.as_str(),
                                    right_flags.as_str(),
                                    *slot as usize,
                                    *exclusive != 0,
                                    line,
                                )
                                .map_err(Into::into),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::RegexEofFlipFlop(slot, exclusive, lp, lf) => {
                        let line = self.line();
                        let left_pat = constants[*lp as usize].as_str_or_empty();
                        let left_flags = constants[*lf as usize].as_str_or_empty();
                        let v = vm_interp_result(
                            self.interp
                                .regex_eof_flip_flop_eval(
                                    left_pat.as_str(),
                                    left_flags.as_str(),
                                    *slot as usize,
                                    *exclusive != 0,
                                    line,
                                )
                                .map_err(Into::into),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::RegexFlipFlopExprRhs(slot, exclusive, lp, lf, rhs_idx) => {
                        let idx = *rhs_idx as usize;
                        let line = self.line();
                        let right_m = if let Some(&(start, end)) = self
                            .regex_flip_flop_rhs_expr_bytecode_ranges
                            .get(idx)
                            .and_then(|r| r.as_ref())
                        {
                            let val = self.run_block_region(start, end, op_count)?;
                            val.is_true()
                        } else {
                            let e = &self.regex_flip_flop_rhs_expr_entries[idx];
                            match self.interp.eval_boolean_rvalue_condition(e) {
                                Ok(b) => b,
                                Err(FlowOrError::Error(err)) => return Err(err),
                                Err(FlowOrError::Flow(_)) => {
                                    return Err(PerlError::runtime(
                                        "unexpected flow in regex flip-flop RHS",
                                        line,
                                    ))
                                }
                            }
                        };
                        let left_pat = constants[*lp as usize].as_str_or_empty();
                        let left_flags = constants[*lf as usize].as_str_or_empty();
                        let v = vm_interp_result(
                            self.interp
                                .regex_flip_flop_eval_dynamic_right(
                                    left_pat.as_str(),
                                    left_flags.as_str(),
                                    *slot as usize,
                                    *exclusive != 0,
                                    line,
                                    right_m,
                                )
                                .map_err(Into::into),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::RegexFlipFlopDotLineRhs(slot, exclusive, lp, lf, line_cidx) => {
                        let line = self.line();
                        let rhs_line = constants[*line_cidx as usize].to_int();
                        let left_pat = constants[*lp as usize].as_str_or_empty();
                        let left_flags = constants[*lf as usize].as_str_or_empty();
                        let v = vm_interp_result(
                            self.interp
                                .regex_flip_flop_eval_dot_line_rhs(
                                    left_pat.as_str(),
                                    left_flags.as_str(),
                                    *slot as usize,
                                    *exclusive != 0,
                                    line,
                                    rhs_line,
                                )
                                .map_err(Into::into),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }

                    // ── Regex ──
                    Op::RegexMatch(pat_idx, flags_idx, scalar_g, pos_key_idx) => {
                        let val = self.pop();
                        let pattern = constants[*pat_idx as usize].as_str_or_empty();
                        let flags = constants[*flags_idx as usize].as_str_or_empty();
                        let line = self.line();
                        if val.is_iterator() {
                            let source = crate::map_stream::into_pull_iter(val);
                            let re = match self.interp.compile_regex(&pattern, &flags, line) {
                                Ok(r) => r,
                                Err(FlowOrError::Error(e)) => return Err(e),
                                Err(FlowOrError::Flow(_)) => {
                                    return Err(PerlError::runtime(
                                        "unexpected flow in regex compile",
                                        line,
                                    ));
                                }
                            };
                            let global = flags.contains('g');
                            if global {
                                self.push(PerlValue::iterator(std::sync::Arc::new(
                                    crate::map_stream::MatchGlobalStreamIterator::new(source, re),
                                )));
                            } else {
                                self.push(PerlValue::iterator(std::sync::Arc::new(
                                    crate::map_stream::MatchStreamIterator::new(source, re),
                                )));
                            }
                            return Ok(());
                        }
                        let string = val.into_string();
                        let pos_key_owned = if *pos_key_idx == u16::MAX {
                            None
                        } else {
                            Some(constants[*pos_key_idx as usize].as_str_or_empty())
                        };
                        let pos_key: &str = pos_key_owned.as_deref().unwrap_or("_");
                        match self
                            .interp
                            .regex_match_execute(string, &pattern, &flags, *scalar_g, pos_key, line)
                        {
                            Ok(v) => {
                                self.push(v);
                                Ok(())
                            }
                            Err(FlowOrError::Error(e)) => Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                Err(PerlError::runtime("unexpected flow in regex match", line))
                            }
                        }
                    }
                    Op::RegexSubst(pat_idx, repl_idx, flags_idx, lvalue_idx) => {
                        let val = self.pop();
                        let pattern = constants[*pat_idx as usize].as_str_or_empty();
                        let replacement = constants[*repl_idx as usize].as_str_or_empty();
                        let flags = constants[*flags_idx as usize].as_str_or_empty();
                        let line = self.line();
                        if val.is_iterator() {
                            let source = crate::map_stream::into_pull_iter(val);
                            let re = match self.interp.compile_regex(&pattern, &flags, line) {
                                Ok(r) => r,
                                Err(FlowOrError::Error(e)) => return Err(e),
                                Err(FlowOrError::Flow(_)) => {
                                    return Err(PerlError::runtime(
                                        "unexpected flow in regex compile",
                                        line,
                                    ));
                                }
                            };
                            let global = flags.contains('g');
                            self.push(PerlValue::iterator(std::sync::Arc::new(
                                crate::map_stream::SubstStreamIterator::new(
                                    source,
                                    re,
                                    crate::interpreter::normalize_replacement_backrefs(
                                        &replacement,
                                    ),
                                    global,
                                ),
                            )));
                            return Ok(());
                        }
                        let string = val.into_string();
                        let target = &self.lvalues[*lvalue_idx as usize];
                        match self.interp.regex_subst_execute(
                            string,
                            &pattern,
                            &replacement,
                            &flags,
                            target,
                            line,
                        ) {
                            Ok(v) => {
                                self.push(v);
                                Ok(())
                            }
                            Err(FlowOrError::Error(e)) => Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                Err(PerlError::runtime("unexpected flow in s///", line))
                            }
                        }
                    }
                    Op::RegexTransliterate(from_idx, to_idx, flags_idx, lvalue_idx) => {
                        let val = self.pop();
                        let from = constants[*from_idx as usize].as_str_or_empty();
                        let to = constants[*to_idx as usize].as_str_or_empty();
                        let flags = constants[*flags_idx as usize].as_str_or_empty();
                        let line = self.line();
                        if val.is_iterator() {
                            let source = crate::map_stream::into_pull_iter(val);
                            self.push(PerlValue::iterator(std::sync::Arc::new(
                                crate::map_stream::TransliterateStreamIterator::new(
                                    source, &from, &to, &flags,
                                ),
                            )));
                            return Ok(());
                        }
                        let string = val.into_string();
                        let target = &self.lvalues[*lvalue_idx as usize];
                        match self
                            .interp
                            .regex_transliterate_execute(string, &from, &to, &flags, target, line)
                        {
                            Ok(v) => {
                                self.push(v);
                                Ok(())
                            }
                            Err(FlowOrError::Error(e)) => Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                Err(PerlError::runtime("unexpected flow in tr///", line))
                            }
                        }
                    }
                    Op::RegexMatchDyn(negate) => {
                        let rhs = self.pop();
                        let s = self.pop().into_string();
                        let line = self.line();
                        let exec = if let Some((pat, fl)) = rhs.regex_src_and_flags() {
                            self.interp
                                .regex_match_execute(s, &pat, &fl, false, "_", line)
                        } else {
                            let pattern = rhs.into_string();
                            self.interp
                                .regex_match_execute(s, &pattern, "", false, "_", line)
                        };
                        match exec {
                            Ok(v) => {
                                let matched = v.is_true();
                                let out = if *negate { !matched } else { matched };
                                self.push(PerlValue::integer(if out { 1 } else { 0 }));
                            }
                            Err(FlowOrError::Error(e)) => return Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                return Err(PerlError::runtime("unexpected flow in =~", line));
                            }
                        }
                        Ok(())
                    }
                    Op::RegexBoolToScalar => {
                        let v = self.pop();
                        self.push(if v.is_true() {
                            PerlValue::integer(1)
                        } else {
                            PerlValue::string(String::new())
                        });
                        Ok(())
                    }
                    Op::SetRegexPos => {
                        let key = self.pop().to_string();
                        let val = self.pop();
                        if val.is_undef() {
                            self.interp.regex_pos.insert(key, None);
                        } else {
                            let u = val.to_int().max(0) as usize;
                            self.interp.regex_pos.insert(key, Some(u));
                        }
                        Ok(())
                    }
                    Op::LoadRegex(pat_idx, flags_idx) => {
                        let pattern = constants[*pat_idx as usize].as_str_or_empty();
                        let flags = constants[*flags_idx as usize].as_str_or_empty();
                        let line = self.line();
                        let pattern_owned = pattern.clone();
                        let re = match self.interp.compile_regex(&pattern, &flags, line) {
                            Ok(r) => r,
                            Err(FlowOrError::Error(e)) => return Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                return Err(PerlError::runtime(
                                    "unexpected flow in qr// compile",
                                    line,
                                ));
                            }
                        };
                        self.push(PerlValue::regex(re, pattern_owned, flags.to_string()));
                        Ok(())
                    }
                    Op::ConcatAppend(idx) => {
                        let rhs = self.pop();
                        let n = names[*idx as usize].as_str();
                        let line = self.line();
                        let result = self
                            .interp
                            .scope
                            .scalar_concat_inplace(n, &rhs)
                            .map_err(|e| e.at_line(line))?;
                        self.push(result);
                        Ok(())
                    }
                    Op::ConcatAppendSlot(slot) => {
                        let rhs = self.pop();
                        let result = self.interp.scope.scalar_slot_concat_inplace(*slot, &rhs);
                        self.push(result);
                        Ok(())
                    }
                    Op::ConcatAppendSlotVoid(slot) => {
                        let rhs = self.pop();
                        self.interp.scope.scalar_slot_concat_inplace(*slot, &rhs);
                        Ok(())
                    }
                    Op::SlotLtIntJumpIfFalse(slot, limit, target) => {
                        let val = self.interp.scope.get_scalar_slot(*slot);
                        let lt = if let Some(i) = val.as_integer() {
                            i < *limit as i64
                        } else {
                            val.to_number() < *limit as f64
                        };
                        if !lt {
                            self.ip = *target;
                        }
                        Ok(())
                    }
                    Op::SlotIncLtIntJumpBack(slot, limit, body_target) => {
                        // Fused trailing `++$slot; goto top_test` for the bench_loop shape:
                        // matches `PreIncSlotVoid` + `Jump` + top `SlotLtIntJumpIfFalse` exactly so
                        // coercion, wrap-around, and integer-only write semantics line up byte-for-byte
                        // with the un-fused form. Every iteration past the first skips the top check
                        // and the unconditional jump entirely.
                        let next_i = self
                            .interp
                            .scope
                            .get_scalar_slot(*slot)
                            .to_int()
                            .wrapping_add(1);
                        self.interp
                            .scope
                            .set_scalar_slot(*slot, PerlValue::integer(next_i));
                        if next_i < *limit as i64 {
                            self.ip = *body_target;
                        }
                        Ok(())
                    }
                    Op::AccumSumLoop(sum_slot, i_slot, limit) => {
                        // Runs the entire counted `while $i < limit { $sum += $i; $i += 1 }` loop in
                        // native Rust. The peephole only fires when the body is exactly this one
                        // accumulate statement, so every side effect is captured by the final
                        // `$sum` and `$i` writes; there is nothing else to do per iteration.
                        let mut sum = self.interp.scope.get_scalar_slot(*sum_slot).to_int();
                        let mut i = self.interp.scope.get_scalar_slot(*i_slot).to_int();
                        let limit = *limit as i64;
                        while i < limit {
                            sum = sum.wrapping_add(i);
                            i = i.wrapping_add(1);
                        }
                        self.interp
                            .scope
                            .set_scalar_slot(*sum_slot, PerlValue::integer(sum));
                        self.interp
                            .scope
                            .set_scalar_slot(*i_slot, PerlValue::integer(i));
                        Ok(())
                    }
                    Op::AddHashElemPlainKeyToSlot(sum_slot, k_name_idx, h_name_idx) => {
                        // `$sum += $h{$k}` — single-dispatch slot += hash[name-scalar] with no
                        // VM stack traffic. The key scalar is read via plain (name-based) access
                        // because the compiler's `for my $k (keys %h)` lowering currently backs
                        // `$k` with a frame scalar, not a slot.
                        let k_name = names[*k_name_idx as usize].as_str();
                        let h_name = names[*h_name_idx as usize].as_str();
                        self.interp.touch_env_hash(h_name);
                        let key = self.interp.scope.get_scalar(k_name).to_string();
                        let elem = self.interp.scope.get_hash_element(h_name, &key);
                        let cur = self.interp.scope.get_scalar_slot(*sum_slot);
                        let new_v =
                            if let (Some(a), Some(b)) = (cur.as_integer(), elem.as_integer()) {
                                PerlValue::integer(a.wrapping_add(b))
                            } else {
                                PerlValue::float(cur.to_number() + elem.to_number())
                            };
                        self.interp.scope.set_scalar_slot(*sum_slot, new_v);
                        Ok(())
                    }
                    Op::AddHashElemSlotKeyToSlot(sum_slot, k_slot, h_name_idx) => {
                        // `$sum += $h{$k}` — slot counter, slot key, slot sum. Zero name lookups
                        // for `$sum` and `$k`; one frame-walk for `%h` (same as the non-slot form).
                        let h_name = names[*h_name_idx as usize].as_str();
                        self.interp.touch_env_hash(h_name);
                        let key_val = self.interp.scope.get_scalar_slot(*k_slot);
                        let key = key_val.to_string();
                        let elem = self.interp.scope.get_hash_element(h_name, &key);
                        let cur = self.interp.scope.get_scalar_slot(*sum_slot);
                        let new_v =
                            if let (Some(a), Some(b)) = (cur.as_integer(), elem.as_integer()) {
                                PerlValue::integer(a.wrapping_add(b))
                            } else {
                                PerlValue::float(cur.to_number() + elem.to_number())
                            };
                        self.interp.scope.set_scalar_slot(*sum_slot, new_v);
                        Ok(())
                    }
                    Op::SumHashValuesToSlot(sum_slot, h_name_idx) => {
                        // `for my $k (keys %h) { $sum += $h{$k} }` fused to a single op that walks
                        // `hash.values()` in a tight native loop. No key stringification, no stack
                        // traffic, no per-iter dispatch. The foreach body reduced to
                        // `AddHashElemSlotKeyToSlot`, so this fusion is correct regardless of `$k`
                        // slot assignment — we never read `$k`.
                        let h_name = names[*h_name_idx as usize].as_str();
                        self.interp.touch_env_hash(h_name);
                        let cur = self.interp.scope.get_scalar_slot(*sum_slot);
                        let mut int_acc: i64 = cur.as_integer().unwrap_or(0);
                        let mut float_acc: f64 = 0.0;
                        let mut is_int = cur.as_integer().is_some();
                        if !is_int {
                            float_acc = cur.to_number();
                        }
                        // Walk the hash via the scope's borrow path without cloning the whole
                        // IndexMap. `for_each_hash_value` takes a visitor so the lock (if any) is
                        // held once rather than per-element.
                        self.interp.scope.for_each_hash_value(h_name, |v| {
                            if is_int {
                                if let Some(x) = v.as_integer() {
                                    int_acc = int_acc.wrapping_add(x);
                                    return;
                                }
                                float_acc = int_acc as f64;
                                is_int = false;
                            }
                            float_acc += v.to_number();
                        });
                        let new_v = if is_int {
                            PerlValue::integer(int_acc)
                        } else {
                            PerlValue::float(float_acc)
                        };
                        self.interp.scope.set_scalar_slot(*sum_slot, new_v);
                        Ok(())
                    }
                    Op::SetHashIntTimesLoop(h_name_idx, i_slot, k, limit) => {
                        // Runs the counted `while $i < limit { $h{$i} = $i * k; $i += 1 }` loop
                        // natively: the hash is `reserve()`d once, keys are stringified via
                        // `itoa` (no `format!` allocation), and values are inserted in a tight
                        // Rust loop. `$i` is left at `limit` on exit, matching the un-fused shape.
                        let i_cur = self.interp.scope.get_scalar_slot(*i_slot).to_int();
                        let lim = *limit as i64;
                        if i_cur < lim {
                            let n = names[*h_name_idx as usize].as_str();
                            self.require_hash_mutable(n)?;
                            self.interp.touch_env_hash(n);
                            let line = self.line();
                            self.interp
                                .scope
                                .set_hash_int_times_range(n, i_cur, lim, *k as i64)
                                .map_err(|e| e.at_line(line))?;
                        }
                        self.interp
                            .scope
                            .set_scalar_slot(*i_slot, PerlValue::integer(lim));
                        Ok(())
                    }
                    Op::PushIntRangeToArrayLoop(arr_name_idx, i_slot, limit) => {
                        // Runs the entire counted `while $i < limit { push @arr, $i; $i += 1 }`
                        // loop in native Rust. The array's `Vec<PerlValue>` is reserved once and
                        // `push(PerlValue::integer(i))` runs in a tight Rust loop — no per-iter
                        // op dispatch, no `require_array_mutable` check per iter.
                        let i_cur = self.interp.scope.get_scalar_slot(*i_slot).to_int();
                        let lim = *limit as i64;
                        if i_cur < lim {
                            let n = names[*arr_name_idx as usize].as_str();
                            self.require_array_mutable(n)?;
                            let line = self.line();
                            self.interp
                                .scope
                                .push_int_range_to_array(n, i_cur, lim)
                                .map_err(|e| e.at_line(line))?;
                        }
                        self.interp
                            .scope
                            .set_scalar_slot(*i_slot, PerlValue::integer(lim));
                        Ok(())
                    }
                    Op::ConcatConstSlotLoop(const_idx, s_slot, i_slot, limit) => {
                        // Runs the entire counted `while $i < limit { $s .= CONST; $i += 1 }` loop
                        // in native Rust. We stringify the constant once, reserve `(limit-i_cur) *
                        // const.len()` up front so the owning `String` reallocs at most twice, then
                        // `push_str` in a tight loop (see `try_concat_repeat_inplace`). Falls back
                        // to the per-iteration slow path when the slot is not the sole owner of a
                        // heap `String` — `.=` semantics match the un-fused shape byte-for-byte.
                        let i_cur = self.interp.scope.get_scalar_slot(*i_slot).to_int();
                        let lim = *limit as i64;
                        if i_cur < lim {
                            let n_iters = (lim - i_cur) as usize;
                            let rhs = constants[*const_idx as usize].as_str_or_empty();
                            if !self
                                .interp
                                .scope
                                .scalar_slot_concat_repeat_inplace(*s_slot, &rhs, n_iters)
                            {
                                self.interp
                                    .scope
                                    .scalar_slot_concat_repeat_slow(*s_slot, &rhs, n_iters);
                            }
                        }
                        self.interp
                            .scope
                            .set_scalar_slot(*i_slot, PerlValue::integer(lim));
                        Ok(())
                    }
                    Op::AddAssignSlotSlot(dst, src) => {
                        let a = self.interp.scope.get_scalar_slot(*dst);
                        let b = self.interp.scope.get_scalar_slot(*src);
                        let result = if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                            PerlValue::integer(x.wrapping_add(y))
                        } else {
                            PerlValue::float(a.to_number() + b.to_number())
                        };
                        self.interp.scope.set_scalar_slot(*dst, result.clone());
                        self.push(result);
                        Ok(())
                    }
                    Op::AddAssignSlotSlotVoid(dst, src) => {
                        let a = self.interp.scope.get_scalar_slot(*dst);
                        let b = self.interp.scope.get_scalar_slot(*src);
                        let result = if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                            PerlValue::integer(x.wrapping_add(y))
                        } else {
                            PerlValue::float(a.to_number() + b.to_number())
                        };
                        self.interp.scope.set_scalar_slot(*dst, result);
                        Ok(())
                    }
                    Op::SubAssignSlotSlot(dst, src) => {
                        let a = self.interp.scope.get_scalar_slot(*dst);
                        let b = self.interp.scope.get_scalar_slot(*src);
                        let result = if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                            PerlValue::integer(x.wrapping_sub(y))
                        } else {
                            PerlValue::float(a.to_number() - b.to_number())
                        };
                        self.interp.scope.set_scalar_slot(*dst, result.clone());
                        self.push(result);
                        Ok(())
                    }
                    Op::MulAssignSlotSlot(dst, src) => {
                        let a = self.interp.scope.get_scalar_slot(*dst);
                        let b = self.interp.scope.get_scalar_slot(*src);
                        let result = if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                            PerlValue::integer(x.wrapping_mul(y))
                        } else {
                            PerlValue::float(a.to_number() * b.to_number())
                        };
                        self.interp.scope.set_scalar_slot(*dst, result.clone());
                        self.push(result);
                        Ok(())
                    }

                    // ── Frame-local scalar slots (O(1), no string lookup) ──
                    Op::GetScalarSlot(slot) => {
                        let val = self.interp.scope.get_scalar_slot(*slot);
                        self.push(val);
                        Ok(())
                    }
                    Op::SetScalarSlot(slot) => {
                        let val = self.pop();
                        self.interp
                            .scope
                            .set_scalar_slot_checked(*slot, val, None)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::SetScalarSlotKeep(slot) => {
                        let val = self.peek().dup_stack();
                        self.interp
                            .scope
                            .set_scalar_slot_checked(*slot, val, None)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::DeclareScalarSlot(slot, name_idx) => {
                        let val = self.pop();
                        let name_opt = if *name_idx == u16::MAX {
                            None
                        } else {
                            Some(names[*name_idx as usize].as_str())
                        };
                        self.interp.scope.declare_scalar_slot(*slot, val, name_opt);
                        Ok(())
                    }
                    Op::GetArg(idx) => {
                        // Read argument from caller's stack region without @_ allocation.
                        let val = if let Some(frame) = self.call_stack.last() {
                            let arg_pos = frame.stack_base + *idx as usize;
                            self.stack.get(arg_pos).cloned().unwrap_or(PerlValue::UNDEF)
                        } else {
                            PerlValue::UNDEF
                        };
                        self.push(val);
                        Ok(())
                    }

                    Op::ChompInPlace(lvalue_idx) => {
                        let val = self.pop();
                        let target = &self.lvalues[*lvalue_idx as usize];
                        let line = self.line();
                        match self.interp.chomp_inplace_execute(val, target) {
                            Ok(v) => self.push(v),
                            Err(FlowOrError::Error(e)) => return Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                return Err(PerlError::runtime("unexpected flow in chomp", line));
                            }
                        }
                        Ok(())
                    }
                    Op::ChopInPlace(lvalue_idx) => {
                        let val = self.pop();
                        let target = &self.lvalues[*lvalue_idx as usize];
                        let line = self.line();
                        match self.interp.chop_inplace_execute(val, target) {
                            Ok(v) => self.push(v),
                            Err(FlowOrError::Error(e)) => return Err(e),
                            Err(FlowOrError::Flow(_)) => {
                                return Err(PerlError::runtime("unexpected flow in chop", line));
                            }
                        }
                        Ok(())
                    }
                    Op::SubstrFourArg(idx) => {
                        let (string_e, offset_e, length_e, rep_e) =
                            &self.substr_four_arg_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_substr_expr(
                                string_e,
                                offset_e,
                                length_e.as_ref(),
                                Some(rep_e),
                                self.line(),
                            ),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::KeysExpr(idx) => {
                        let i = *idx as usize;
                        let line = self.line();
                        let v = if let Some(&(start, end)) = self
                            .keys_expr_bytecode_ranges
                            .get(i)
                            .and_then(|r| r.as_ref())
                        {
                            let val = self.run_block_region(start, end, op_count)?;
                            vm_interp_result(Interpreter::keys_from_value(val, line), line)?
                        } else {
                            let e = &self.keys_expr_entries[i];
                            vm_interp_result(self.interp.eval_keys_expr(e, line), line)?
                        };
                        self.push(v);
                        Ok(())
                    }
                    Op::KeysExprScalar(idx) => {
                        let i = *idx as usize;
                        let line = self.line();
                        let v = if let Some(&(start, end)) = self
                            .keys_expr_bytecode_ranges
                            .get(i)
                            .and_then(|r| r.as_ref())
                        {
                            let val = self.run_block_region(start, end, op_count)?;
                            vm_interp_result(Interpreter::keys_from_value(val, line), line)?
                        } else {
                            let e = &self.keys_expr_entries[i];
                            vm_interp_result(self.interp.eval_keys_expr(e, line), line)?
                        };
                        let n = v.as_array_vec().map(|a| a.len()).unwrap_or(0) as i64;
                        self.push(PerlValue::integer(n));
                        Ok(())
                    }
                    Op::ValuesExpr(idx) => {
                        let i = *idx as usize;
                        let line = self.line();
                        let v = if let Some(&(start, end)) = self
                            .values_expr_bytecode_ranges
                            .get(i)
                            .and_then(|r| r.as_ref())
                        {
                            let val = self.run_block_region(start, end, op_count)?;
                            vm_interp_result(Interpreter::values_from_value(val, line), line)?
                        } else {
                            let e = &self.values_expr_entries[i];
                            vm_interp_result(self.interp.eval_values_expr(e, line), line)?
                        };
                        self.push(v);
                        Ok(())
                    }
                    Op::ValuesExprScalar(idx) => {
                        let i = *idx as usize;
                        let line = self.line();
                        let v = if let Some(&(start, end)) = self
                            .values_expr_bytecode_ranges
                            .get(i)
                            .and_then(|r| r.as_ref())
                        {
                            let val = self.run_block_region(start, end, op_count)?;
                            vm_interp_result(Interpreter::values_from_value(val, line), line)?
                        } else {
                            let e = &self.values_expr_entries[i];
                            vm_interp_result(self.interp.eval_values_expr(e, line), line)?
                        };
                        let n = v.as_array_vec().map(|a| a.len()).unwrap_or(0) as i64;
                        self.push(PerlValue::integer(n));
                        Ok(())
                    }
                    Op::DeleteExpr(idx) => {
                        let e = &self.delete_expr_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_delete_operand(e, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::ExistsExpr(idx) => {
                        let e = &self.exists_expr_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_exists_operand(e, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::PushExpr(idx) => {
                        let (array, values) = &self.push_expr_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp
                                .eval_push_expr(array, values.as_slice(), self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::PopExpr(idx) => {
                        let e = &self.pop_expr_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_pop_expr(e, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::ShiftExpr(idx) => {
                        let e = &self.shift_expr_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_shift_expr(e, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::UnshiftExpr(idx) => {
                        let (array, values) = &self.unshift_expr_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp
                                .eval_unshift_expr(array, values.as_slice(), self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::SpliceExpr(idx) => {
                        let (array, offset, length, replacement) =
                            &self.splice_expr_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_splice_expr(
                                array,
                                offset.as_ref(),
                                length.as_ref(),
                                replacement.as_slice(),
                                self.interp.wantarray_kind,
                                self.line(),
                            ),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }

                    // ── References ──
                    Op::MakeScalarRef => {
                        let val = self.pop();
                        self.push(PerlValue::scalar_ref(Arc::new(RwLock::new(val))));
                        Ok(())
                    }
                    Op::MakeScalarBindingRef(name_idx) => {
                        let name = names[*name_idx as usize].clone();
                        self.push(PerlValue::scalar_binding_ref(name));
                        Ok(())
                    }
                    Op::MakeArrayBindingRef(name_idx) => {
                        let name = names[*name_idx as usize].clone();
                        self.push(PerlValue::array_binding_ref(name));
                        Ok(())
                    }
                    Op::MakeHashBindingRef(name_idx) => {
                        let name = names[*name_idx as usize].clone();
                        self.push(PerlValue::hash_binding_ref(name));
                        Ok(())
                    }
                    Op::MakeArrayRefAlias => {
                        let v = self.pop();
                        let line = self.line();
                        let out =
                            vm_interp_result(self.interp.make_array_ref_alias(v, line), line)?;
                        self.push(out);
                        Ok(())
                    }
                    Op::MakeHashRefAlias => {
                        let v = self.pop();
                        let line = self.line();
                        let out = vm_interp_result(self.interp.make_hash_ref_alias(v, line), line)?;
                        self.push(out);
                        Ok(())
                    }
                    Op::MakeArrayRef => {
                        let val = self.pop();
                        let arr = if let Some(a) = val.as_array_vec() {
                            a
                        } else {
                            vec![val]
                        };
                        self.push(PerlValue::array_ref(Arc::new(RwLock::new(arr))));
                        Ok(())
                    }
                    Op::MakeHashRef => {
                        let val = self.pop();
                        let map = if let Some(h) = val.as_hash_map() {
                            h
                        } else {
                            let items = val.to_list();
                            let mut m = IndexMap::new();
                            let mut i = 0;
                            while i + 1 < items.len() {
                                m.insert(items[i].to_string(), items[i + 1].clone());
                                i += 2;
                            }
                            m
                        };
                        self.push(PerlValue::hash_ref(Arc::new(RwLock::new(map))));
                        Ok(())
                    }
                    Op::MakeCodeRef(block_idx, sig_idx) => {
                        let block = self.blocks[*block_idx as usize].clone();
                        let params = self.code_ref_sigs[*sig_idx as usize].clone();
                        let captured = self.interp.scope.capture();
                        self.push(PerlValue::code_ref(Arc::new(crate::value::PerlSub {
                            name: "__ANON__".to_string(),
                            params,
                            body: block,
                            closure_env: Some(captured),
                            prototype: None,
                            fib_like: None,
                        })));
                        Ok(())
                    }
                    Op::LoadNamedSubRef(name_idx) => {
                        let name = names[*name_idx as usize].as_str();
                        let line = self.line();
                        let sub = self.interp.resolve_sub_by_name(name).ok_or_else(|| {
                            PerlError::runtime(
                                self.interp.undefined_subroutine_resolve_message(name),
                                line,
                            )
                        })?;
                        self.push(PerlValue::code_ref(sub));
                        Ok(())
                    }
                    Op::LoadDynamicSubRef => {
                        let name = self.pop().to_string();
                        let line = self.line();
                        let sub = self.interp.resolve_sub_by_name(&name).ok_or_else(|| {
                            PerlError::runtime(
                                self.interp.undefined_subroutine_resolve_message(&name),
                                line,
                            )
                        })?;
                        self.push(PerlValue::code_ref(sub));
                        Ok(())
                    }
                    Op::LoadDynamicTypeglob => {
                        let name = self.pop().to_string();
                        let n = self.interp.resolve_io_handle_name(&name);
                        self.push(PerlValue::string(n));
                        Ok(())
                    }
                    Op::CopyTypeglobSlots(lhs_i, rhs_i) => {
                        let lhs = self.names[*lhs_i as usize].as_str();
                        let rhs = self.names[*rhs_i as usize].as_str();
                        let line = self.line();
                        self.interp
                            .copy_typeglob_slots(lhs, rhs, line)
                            .map_err(|e| e.at_line(line))?;
                        Ok(())
                    }
                    Op::TypeglobAssignFromValue(name_idx) => {
                        let val = self.pop();
                        let name = self.names[*name_idx as usize].as_str();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_typeglob_value(name, val.clone(), line),
                            line,
                        )?;
                        self.push(val);
                        Ok(())
                    }
                    Op::TypeglobAssignFromValueDynamic => {
                        let val = self.pop();
                        let name = self.pop().to_string();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_typeglob_value(&name, val.clone(), line),
                            line,
                        )?;
                        self.push(val);
                        Ok(())
                    }
                    Op::CopyTypeglobSlotsDynamicLhs(rhs_i) => {
                        let lhs = self.pop().to_string();
                        let rhs = self.names[*rhs_i as usize].as_str();
                        let line = self.line();
                        self.interp
                            .copy_typeglob_slots(&lhs, rhs, line)
                            .map_err(|e| e.at_line(line))?;
                        Ok(())
                    }
                    Op::SymbolicDeref(kind_byte) => {
                        let v = self.pop();
                        let kind = match *kind_byte {
                            0 => Sigil::Scalar,
                            1 => Sigil::Array,
                            2 => Sigil::Hash,
                            3 => Sigil::Typeglob,
                            _ => {
                                return Err(PerlError::runtime(
                                    "VM: bad SymbolicDeref kind byte",
                                    self.line(),
                                ));
                            }
                        };
                        let line = self.line();
                        let out =
                            vm_interp_result(self.interp.symbolic_deref(v, kind, line), line)?;
                        self.push(out);
                        Ok(())
                    }

                    // ── Arrow dereference ──
                    Op::ArrowArray => {
                        let idx = self.pop().to_int();
                        let r = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(
                            self.interp.read_arrow_array_element(r, idx, line),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::ArrowHash => {
                        let key = self.pop().to_string();
                        let r = self.pop();
                        let line = self.line();
                        let v = vm_interp_result(
                            self.interp.read_arrow_hash_element(r, key.as_str(), line),
                            line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::SetArrowHash => {
                        let key = self.pop().to_string();
                        let r = self.pop();
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_arrow_hash_deref(r, key, val, line),
                            line,
                        )?;
                        Ok(())
                    }
                    Op::SetArrowArray => {
                        let idx = self.pop().to_int();
                        let r = self.pop();
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_arrow_array_deref(r, idx, val, line),
                            line,
                        )?;
                        Ok(())
                    }
                    Op::SetArrowArrayKeep => {
                        let idx = self.pop().to_int();
                        let r = self.pop();
                        let val = self.pop();
                        let val_keep = val.clone();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_arrow_array_deref(r, idx, val, line),
                            line,
                        )?;
                        self.push(val_keep);
                        Ok(())
                    }
                    Op::SetArrowHashKeep => {
                        let key = self.pop().to_string();
                        let r = self.pop();
                        let val = self.pop();
                        let val_keep = val.clone();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_arrow_hash_deref(r, key, val, line),
                            line,
                        )?;
                        self.push(val_keep);
                        Ok(())
                    }
                    Op::ArrowArrayPostfix(b) => {
                        let idx = self.pop().to_int();
                        let r = self.pop();
                        let line = self.line();
                        let old = vm_interp_result(
                            self.interp.arrow_array_postfix(r, idx, *b == 1, line),
                            line,
                        )?;
                        self.push(old);
                        Ok(())
                    }
                    Op::ArrowHashPostfix(b) => {
                        let key = self.pop().to_string();
                        let r = self.pop();
                        let line = self.line();
                        let old = vm_interp_result(
                            self.interp.arrow_hash_postfix(r, key, *b == 1, line),
                            line,
                        )?;
                        self.push(old);
                        Ok(())
                    }
                    Op::SetSymbolicScalarRef => {
                        let r = self.pop();
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(self.interp.assign_scalar_ref_deref(r, val, line), line)?;
                        Ok(())
                    }
                    Op::SetSymbolicScalarRefKeep => {
                        let r = self.pop();
                        let val = self.pop();
                        let val_keep = val.clone();
                        let line = self.line();
                        vm_interp_result(self.interp.assign_scalar_ref_deref(r, val, line), line)?;
                        self.push(val_keep);
                        Ok(())
                    }
                    Op::SetSymbolicArrayRef => {
                        let r = self.pop();
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_symbolic_array_ref_deref(r, val, line),
                            line,
                        )?;
                        Ok(())
                    }
                    Op::SetSymbolicHashRef => {
                        let r = self.pop();
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_symbolic_hash_ref_deref(r, val, line),
                            line,
                        )?;
                        Ok(())
                    }
                    Op::SetSymbolicTypeglobRef => {
                        let r = self.pop();
                        let val = self.pop();
                        let line = self.line();
                        vm_interp_result(
                            self.interp.assign_symbolic_typeglob_ref_deref(r, val, line),
                            line,
                        )?;
                        Ok(())
                    }
                    Op::SymbolicScalarRefPostfix(b) => {
                        let r = self.pop();
                        let line = self.line();
                        let old = vm_interp_result(
                            self.interp.symbolic_scalar_ref_postfix(r, *b == 1, line),
                            line,
                        )?;
                        self.push(old);
                        Ok(())
                    }
                    Op::ArrowCall(wa) => {
                        let want = WantarrayCtx::from_byte(*wa);
                        let args_val = self.pop();
                        let r = self.pop();
                        let args = args_val.to_list();
                        if let Some(sub) = r.as_code_ref() {
                            // Higher-order function wrappers (comp, partial, memoize, etc.)
                            // have empty bodies + magic closure_env keys. Dispatch them via
                            // the interpreter's try_hof_dispatch before falling through to
                            // the normal body execution path.
                            if let Some(hof_result) =
                                self.interp.try_hof_dispatch(&sub, &args, want, self.line())
                            {
                                let v = vm_interp_result(hof_result, self.line())?;
                                self.push(v);
                                return Ok(());
                            }
                            self.interp.current_sub_stack.push(sub.clone());
                            let saved_wa = self.interp.wantarray_kind;
                            self.interp.wantarray_kind = want;
                            self.interp.scope_push_hook();
                            self.interp.scope.declare_array("_", args.clone());
                            if let Some(ref env) = sub.closure_env {
                                self.interp.scope.restore_capture(env);
                            }
                            let line = self.line();
                            let argv = self.interp.scope.take_sub_underscore().unwrap_or_default();
                            self.interp
                                .apply_sub_signature(sub.as_ref(), &argv, line)
                                .map_err(|e| e.at_line(line))?;
                            self.interp.scope.declare_array("_", argv.clone());
                            // Set $_0, $_1, $_2, ... for all args, and $_ to first arg
                            self.interp.scope.set_closure_args(&argv);
                            let result = self.interp.exec_block_no_scope(&sub.body);
                            self.interp.wantarray_kind = saved_wa;
                            self.interp.scope_pop_hook();
                            self.interp.current_sub_stack.pop();
                            match result {
                                Ok(v) => self.push(v),
                                Err(crate::interpreter::FlowOrError::Flow(
                                    crate::interpreter::Flow::Return(v),
                                )) => self.push(v),
                                Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                                Err(_) => self.push(PerlValue::UNDEF),
                            }
                        } else {
                            return Err(PerlError::runtime("Not a code reference", self.line()));
                        }
                        Ok(())
                    }
                    Op::IndirectCall(argc, wa, pass_flag) => {
                        let want = WantarrayCtx::from_byte(*wa);
                        let line = self.line();
                        let arg_vals = if *pass_flag != 0 {
                            self.interp.scope.get_array("_")
                        } else {
                            let n = *argc as usize;
                            let mut args = Vec::with_capacity(n);
                            for _ in 0..n {
                                args.push(self.pop());
                            }
                            args.reverse();
                            args
                        };
                        let target = self.pop();
                        // HOF wrapper fast path (comp, partial, memoize, etc.)
                        if let Some(sub) = target.as_code_ref() {
                            if let Some(hof_result) =
                                self.interp.try_hof_dispatch(&sub, &arg_vals, want, line)
                            {
                                let v = vm_interp_result(hof_result, line)?;
                                self.push(v);
                                return Ok(());
                            }
                        }
                        let r = self
                            .interp
                            .dispatch_indirect_call(target, arg_vals, want, line);
                        let v = vm_interp_result(r, line)?;
                        self.push(v);
                        Ok(())
                    }

                    // ── Method call ──
                    Op::MethodCall(name_idx, argc, wa) => {
                        self.run_method_op(*name_idx, *argc, *wa, false)?;
                        Ok(())
                    }
                    Op::MethodCallSuper(name_idx, argc, wa) => {
                        self.run_method_op(*name_idx, *argc, *wa, true)?;
                        Ok(())
                    }

                    // ── File test ──
                    Op::FileTestOp(test) => {
                        let path = self.pop().to_string();
                        let op = *test as char;
                        // -M, -A, -C return fractional days (float)
                        if matches!(op, 'M' | 'A' | 'C') {
                            #[cfg(unix)]
                            {
                                let v = match crate::perl_fs::filetest_age_days(&path, op) {
                                    Some(days) => PerlValue::float(days),
                                    None => PerlValue::UNDEF,
                                };
                                self.push(v);
                                return Ok(());
                            }
                            #[cfg(not(unix))]
                            {
                                self.push(PerlValue::UNDEF);
                                return Ok(());
                            }
                        }
                        // -s returns file size (integer)
                        if op == 's' {
                            let v = match std::fs::metadata(&path) {
                                Ok(m) => PerlValue::integer(m.len() as i64),
                                Err(_) => PerlValue::UNDEF,
                            };
                            self.push(v);
                            return Ok(());
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
                        self.push(PerlValue::integer(if result { 1 } else { 0 }));
                        Ok(())
                    }

                    // ── Map/Grep/Sort with blocks (opcodes when lowered; else tree-walker) ──
                    Op::MapIntMul(k) => {
                        let list = self.pop().to_list();
                        if list.len() == 1 {
                            if let Some(p) = list[0].as_pipeline() {
                                let line = self.line();
                                let sub = Interpreter::pipeline_int_mul_sub(*k);
                                self.interp.pipeline_push(&p, PipelineOp::Map(sub), line)?;
                                self.push(PerlValue::pipeline(Arc::clone(&p)));
                                return Ok(());
                            }
                        }
                        let mut result = Vec::with_capacity(list.len());
                        for item in list {
                            let n = item.to_int();
                            result.push(PerlValue::integer(n.wrapping_mul(*k)));
                        }
                        self.push(PerlValue::array(result));
                        Ok(())
                    }
                    Op::GrepIntModEq(m, r) => {
                        let list = self.pop().to_list();
                        let mut result = Vec::new();
                        for item in list {
                            let n = item.to_int();
                            if n % m == *r {
                                result.push(item);
                            }
                        }
                        self.push(PerlValue::array(result));
                        Ok(())
                    }
                    Op::MapWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        self.map_with_block_common(list, *block_idx, false, op_count)
                    }
                    Op::FlatMapWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        self.map_with_block_common(list, *block_idx, true, op_count)
                    }
                    Op::MapWithExpr(expr_idx) => {
                        let list = self.pop().to_list();
                        self.map_with_expr_common(list, *expr_idx, false, op_count)
                    }
                    Op::FlatMapWithExpr(expr_idx) => {
                        let list = self.pop().to_list();
                        self.map_with_expr_common(list, *expr_idx, true, op_count)
                    }
                    Op::MapsWithBlock(block_idx) => {
                        let val = self.pop();
                        let block = self.blocks[*block_idx as usize].clone();
                        let out =
                            self.interp
                                .map_stream_block_output(val, &block, false, self.line())?;
                        self.push(out);
                        Ok(())
                    }
                    Op::MapsFlatMapWithBlock(block_idx) => {
                        let val = self.pop();
                        let block = self.blocks[*block_idx as usize].clone();
                        let out =
                            self.interp
                                .map_stream_block_output(val, &block, true, self.line())?;
                        self.push(out);
                        Ok(())
                    }
                    Op::MapsWithExpr(expr_idx) => {
                        let val = self.pop();
                        let idx = *expr_idx as usize;
                        let expr = self.map_expr_entries[idx].clone();
                        let out =
                            self.interp
                                .map_stream_expr_output(val, &expr, false, self.line())?;
                        self.push(out);
                        Ok(())
                    }
                    Op::MapsFlatMapWithExpr(expr_idx) => {
                        let val = self.pop();
                        let idx = *expr_idx as usize;
                        let expr = self.map_expr_entries[idx].clone();
                        let out =
                            self.interp
                                .map_stream_expr_output(val, &expr, true, self.line())?;
                        self.push(out);
                        Ok(())
                    }
                    Op::FilterWithBlock(block_idx) => {
                        let val = self.pop();
                        let block = self.blocks[*block_idx as usize].clone();
                        let out =
                            self.interp
                                .filter_stream_block_output(val, &block, self.line())?;
                        self.push(out);
                        Ok(())
                    }
                    Op::FilterWithExpr(expr_idx) => {
                        let val = self.pop();
                        let idx = *expr_idx as usize;
                        let expr = self.grep_expr_entries[idx].clone();
                        let out = self
                            .interp
                            .filter_stream_expr_output(val, &expr, self.line())?;
                        self.push(out);
                        Ok(())
                    }
                    Op::ChunkByWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        self.chunk_by_with_block_common(list, *block_idx, op_count)
                    }
                    Op::ChunkByWithExpr(expr_idx) => {
                        let list = self.pop().to_list();
                        self.chunk_by_with_expr_common(list, *expr_idx, op_count)
                    }
                    Op::GrepWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        if list.len() == 1 {
                            if let Some(p) = list[0].as_pipeline() {
                                let idx = *block_idx as usize;
                                let sub = self.interp.anon_coderef_from_block(&self.blocks[idx]);
                                let line = self.line();
                                self.interp
                                    .pipeline_push(&p, PipelineOp::Filter(sub), line)?;
                                self.push(PerlValue::pipeline(Arc::clone(&p)));
                                return Ok(());
                            }
                        }
                        let idx = *block_idx as usize;
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let mut result = Vec::new();
                            for item in list {
                                self.interp.scope.set_topic(item.clone());
                                let val = self.run_block_region(start, end, op_count)?;
                                // Bare regex → match against $_ (Perl: /pat/ in grep is $_ =~ /pat/)
                                let keep = if let Some(re) = val.as_regex() {
                                    re.is_match(&item.to_string())
                                } else {
                                    val.is_true()
                                };
                                if keep {
                                    result.push(item);
                                }
                            }
                            self.push(PerlValue::array(result));
                            Ok(())
                        } else {
                            let block = self.blocks[idx].clone();
                            let mut result = Vec::new();
                            for item in list {
                                self.interp.scope.set_topic(item.clone());
                                match self.interp.exec_block(&block) {
                                    Ok(val) => {
                                        let keep = if let Some(re) = val.as_regex() {
                                            re.is_match(&item.to_string())
                                        } else {
                                            val.is_true()
                                        };
                                        if keep {
                                            result.push(item);
                                        }
                                    }
                                    Err(crate::interpreter::FlowOrError::Error(e)) => {
                                        return Err(e)
                                    }
                                    Err(_) => {}
                                }
                            }
                            self.push(PerlValue::array(result));
                            Ok(())
                        }
                    }
                    Op::ForEachWithBlock(block_idx) => {
                        let val = self.pop();
                        let idx = *block_idx as usize;
                        // Lazy iterator: consume one-at-a-time without materializing.
                        if val.is_iterator() {
                            let iter = val.into_iterator();
                            let mut count = 0i64;
                            if let Some(&(start, end)) =
                                self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                            {
                                while let Some(item) = iter.next_item() {
                                    count += 1;
                                    self.interp.scope.set_topic(item);
                                    self.run_block_region(start, end, op_count)?;
                                }
                            } else {
                                let block = self.blocks[idx].clone();
                                while let Some(item) = iter.next_item() {
                                    count += 1;
                                    self.interp.scope.set_topic(item);
                                    match self.interp.exec_block(&block) {
                                        Ok(_) => {}
                                        Err(crate::interpreter::FlowOrError::Error(e)) => {
                                            return Err(e)
                                        }
                                        Err(_) => {}
                                    }
                                }
                            }
                            self.push(PerlValue::integer(count));
                            return Ok(());
                        }
                        let list = val.to_list();
                        let count = list.len() as i64;
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            for item in list {
                                self.interp.scope.set_topic(item);
                                self.run_block_region(start, end, op_count)?;
                            }
                        } else {
                            let block = self.blocks[idx].clone();
                            for item in list {
                                self.interp.scope.set_topic(item);
                                match self.interp.exec_block(&block) {
                                    Ok(_) => {}
                                    Err(crate::interpreter::FlowOrError::Error(e)) => {
                                        return Err(e)
                                    }
                                    Err(_) => {}
                                }
                            }
                        }
                        self.push(PerlValue::integer(count));
                        Ok(())
                    }
                    Op::GrepWithExpr(expr_idx) => {
                        let list = self.pop().to_list();
                        let idx = *expr_idx as usize;
                        if let Some(&(start, end)) = self
                            .grep_expr_bytecode_ranges
                            .get(idx)
                            .and_then(|r| r.as_ref())
                        {
                            let mut result = Vec::new();
                            for item in list {
                                self.interp.scope.set_topic(item.clone());
                                let val = self.run_block_region(start, end, op_count)?;
                                let keep = if let Some(re) = val.as_regex() {
                                    re.is_match(&item.to_string())
                                } else {
                                    val.is_true()
                                };
                                if keep {
                                    result.push(item);
                                }
                            }
                            self.push(PerlValue::array(result));
                            Ok(())
                        } else {
                            let e = &self.grep_expr_entries[idx];
                            let mut result = Vec::new();
                            for item in list {
                                self.interp.scope.set_topic(item.clone());
                                let val = vm_interp_result(self.interp.eval_expr(e), self.line())?;
                                let keep = if let Some(re) = val.as_regex() {
                                    re.is_match(&item.to_string())
                                } else {
                                    val.is_true()
                                };
                                if keep {
                                    result.push(item);
                                }
                            }
                            self.push(PerlValue::array(result));
                            Ok(())
                        }
                    }
                    Op::SortWithBlock(block_idx) => {
                        let mut items = self.pop().to_list();
                        let idx = *block_idx as usize;
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let mut sort_err: Option<PerlError> = None;
                            items.sort_by(|a, b| {
                                if sort_err.is_some() {
                                    return std::cmp::Ordering::Equal;
                                }
                                let _ = self.interp.scope.set_scalar("a", a.clone());
                                let _ = self.interp.scope.set_scalar("b", b.clone());
                                let _ = self.interp.scope.set_scalar("_0", a.clone());
                                let _ = self.interp.scope.set_scalar("_1", b.clone());
                                match self.run_block_region(start, end, op_count) {
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
                                    Err(e) => {
                                        sort_err = Some(e);
                                        std::cmp::Ordering::Equal
                                    }
                                }
                            });
                            if let Some(e) = sort_err {
                                return Err(e);
                            }
                            self.push(PerlValue::array(items));
                            Ok(())
                        } else {
                            let block = self.blocks[idx].clone();
                            items.sort_by(|a, b| {
                                let _ = self.interp.scope.set_scalar("a", a.clone());
                                let _ = self.interp.scope.set_scalar("b", b.clone());
                                let _ = self.interp.scope.set_scalar("_0", a.clone());
                                let _ = self.interp.scope.set_scalar("_1", b.clone());
                                match self.interp.exec_block(&block) {
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
                            self.push(PerlValue::array(items));
                            Ok(())
                        }
                    }
                    Op::SortWithBlockFast(tag) => {
                        let mut items = self.pop().to_list();
                        let mode = match *tag {
                            0 => SortBlockFast::Numeric,
                            1 => SortBlockFast::String,
                            2 => SortBlockFast::NumericRev,
                            3 => SortBlockFast::StringRev,
                            _ => SortBlockFast::Numeric,
                        };
                        items.sort_by(|a, b| sort_magic_cmp(a, b, mode));
                        self.push(PerlValue::array(items));
                        Ok(())
                    }
                    Op::SortNoBlock => {
                        let mut items = self.pop().to_list();
                        items.sort_by_key(|a| a.to_string());
                        self.push(PerlValue::array(items));
                        Ok(())
                    }
                    Op::SortWithCodeComparator(wa) => {
                        let want = WantarrayCtx::from_byte(*wa);
                        let cmp_val = self.pop();
                        let mut items = self.pop().to_list();
                        let line = self.line();
                        let Some(sub) = cmp_val.as_code_ref() else {
                            return Err(PerlError::runtime(
                                "sort: comparator must be a code reference",
                                line,
                            ));
                        };
                        let interp = &mut self.interp;
                        items.sort_by(|a, b| {
                            let _ = interp.scope.set_scalar("a", a.clone());
                            let _ = interp.scope.set_scalar("b", b.clone());
                            let _ = interp.scope.set_scalar("_0", a.clone());
                            let _ = interp.scope.set_scalar("_1", b.clone());
                            match interp.call_sub(sub.as_ref(), vec![], want, line) {
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
                        self.push(PerlValue::array(items));
                        Ok(())
                    }
                    Op::ReverseListOp => {
                        let val = self.pop();
                        let mut items = val.to_list();
                        items.reverse();
                        self.push(PerlValue::array(items));
                        Ok(())
                    }
                    Op::ReverseScalarOp => {
                        let val = self.pop();
                        let items = val.to_list();
                        let s: String = items.iter().map(|v| v.to_string()).collect();
                        self.push(PerlValue::string(s.chars().rev().collect()));
                        Ok(())
                    }
                    Op::RevOp => {
                        let val = self.pop();
                        if val.is_iterator() {
                            self.push(PerlValue::iterator(std::sync::Arc::new(
                                crate::value::ScalarReverseIterator::new(val.into_iterator()),
                            )));
                        } else {
                            let items = val.to_list();
                            if items.len() <= 1 {
                                let s = if items.is_empty() {
                                    String::new()
                                } else {
                                    items[0].to_string()
                                };
                                self.push(PerlValue::string(s.chars().rev().collect()));
                            } else {
                                let mut items = items;
                                items.reverse();
                                self.push(PerlValue::array(items));
                            }
                        }
                        Ok(())
                    }
                    Op::StackArrayLen => {
                        let v = self.pop();
                        self.push(PerlValue::integer(v.to_list().len() as i64));
                        Ok(())
                    }
                    Op::ListSliceToScalar => {
                        let v = self.pop();
                        let items = v.to_list();
                        self.push(items.last().cloned().unwrap_or(PerlValue::UNDEF));
                        Ok(())
                    }

                    // ── Eval block ──
                    Op::EvalBlock(block_idx, want) => {
                        let block = self.blocks[*block_idx as usize].clone();
                        let tail = crate::interpreter::WantarrayCtx::from_byte(*want);
                        self.interp.eval_nesting += 1;
                        // Use exec_block (with scope frame) so local/my declarations
                        // inside the block are properly scoped.
                        match self.interp.exec_block_with_tail(&block, tail) {
                            Ok(v) => {
                                self.interp.clear_eval_error();
                                self.push(v);
                            }
                            Err(crate::interpreter::FlowOrError::Error(e)) => {
                                self.interp.set_eval_error_from_perl_error(&e);
                                self.push(PerlValue::UNDEF);
                            }
                            Err(_) => self.push(PerlValue::UNDEF),
                        }
                        self.interp.eval_nesting -= 1;
                        Ok(())
                    }
                    Op::TraceBlock(block_idx) => {
                        let block = self.blocks[*block_idx as usize].clone();
                        crate::parallel_trace::trace_enter();
                        self.interp.eval_nesting += 1;
                        match self.interp.exec_block(&block) {
                            Ok(v) => {
                                self.interp.clear_eval_error();
                                self.push(v);
                            }
                            Err(FlowOrError::Error(e)) => {
                                self.interp.set_eval_error_from_perl_error(&e);
                                self.push(PerlValue::UNDEF);
                            }
                            Err(_) => self.push(PerlValue::UNDEF),
                        }
                        self.interp.eval_nesting -= 1;
                        crate::parallel_trace::trace_leave();
                        Ok(())
                    }
                    Op::TimerBlock(block_idx) => {
                        let block = self.blocks[*block_idx as usize].clone();
                        let start = std::time::Instant::now();
                        self.interp.eval_nesting += 1;
                        let _ = match self.interp.exec_block(&block) {
                            Ok(v) => {
                                self.interp.clear_eval_error();
                                v
                            }
                            Err(FlowOrError::Error(e)) => {
                                self.interp.set_eval_error_from_perl_error(&e);
                                PerlValue::UNDEF
                            }
                            Err(_) => PerlValue::UNDEF,
                        };
                        self.interp.eval_nesting -= 1;
                        let ms = start.elapsed().as_secs_f64() * 1000.0;
                        self.push(PerlValue::float(ms));
                        Ok(())
                    }
                    Op::BenchBlock(block_idx) => {
                        let n_i = self.pop().to_int();
                        if n_i < 0 {
                            return Err(PerlError::runtime(
                                "bench: iteration count must be non-negative",
                                self.line(),
                            ));
                        }
                        let n = n_i as usize;
                        let block = self.blocks[*block_idx as usize].clone();
                        let v = vm_interp_result(
                            self.interp.run_bench_block(&block, n, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::Given(idx) => {
                        let i = *idx as usize;
                        let line = self.line();
                        let v = if let Some(&(start, end)) = self
                            .given_topic_bytecode_ranges
                            .get(i)
                            .and_then(|r| r.as_ref())
                        {
                            let topic_val = self.run_block_region(start, end, op_count)?;
                            let body = &self.given_entries[i].1;
                            vm_interp_result(
                                self.interp.exec_given_with_topic_value(topic_val, body),
                                line,
                            )?
                        } else {
                            let (topic, body) = &self.given_entries[i];
                            vm_interp_result(self.interp.exec_given(topic, body), line)?
                        };
                        self.push(v);
                        Ok(())
                    }
                    Op::EvalTimeout(idx) => {
                        let i = *idx as usize;
                        let body = self.eval_timeout_entries[i].1.clone();
                        let secs = if let Some(&(start, end)) = self
                            .eval_timeout_expr_bytecode_ranges
                            .get(i)
                            .and_then(|r| r.as_ref())
                        {
                            self.run_block_region(start, end, op_count)?.to_number()
                        } else {
                            let timeout_expr = &self.eval_timeout_entries[i].0;
                            vm_interp_result(self.interp.eval_expr(timeout_expr), self.line())?
                                .to_number()
                        };
                        let v = vm_interp_result(
                            self.interp.eval_timeout_block(&body, secs, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::AlgebraicMatch(idx) => {
                        let i = *idx as usize;
                        let line = self.line();
                        let v = if let Some(&(start, end)) = self
                            .algebraic_match_subject_bytecode_ranges
                            .get(i)
                            .and_then(|r| r.as_ref())
                        {
                            let subject_val = self.run_block_region(start, end, op_count)?;
                            let arms = &self.algebraic_match_entries[i].1;
                            vm_interp_result(
                                self.interp.eval_algebraic_match_with_subject_value(
                                    subject_val,
                                    arms,
                                    line,
                                ),
                                self.line(),
                            )?
                        } else {
                            let (subject, arms) = &self.algebraic_match_entries[i];
                            vm_interp_result(
                                self.interp.eval_algebraic_match(subject, arms, line),
                                self.line(),
                            )?
                        };
                        self.push(v);
                        Ok(())
                    }
                    Op::ParLines(idx) => {
                        let (path, callback, progress) = &self.par_lines_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_par_lines_expr(
                                path,
                                callback,
                                progress.as_ref(),
                                self.line(),
                            ),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::ParWalk(idx) => {
                        let (path, callback, progress) = &self.par_walk_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_par_walk_expr(
                                path,
                                callback,
                                progress.as_ref(),
                                self.line(),
                            ),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::Pwatch(idx) => {
                        let (path, callback) = &self.pwatch_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_pwatch_expr(path, callback, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }

                    // ── Parallel operations (rayon) ──
                    Op::PMapWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let (scope_capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        let n_workers = rayon::current_num_threads();
                        let pool: Vec<Mutex<Interpreter>> = (0..n_workers)
                            .map(|_| {
                                let mut interp = Interpreter::new();
                                interp.subs = subs.clone();
                                interp.scope.restore_capture(&scope_capture);
                                interp.scope.restore_atomics(&atomic_arrays, &atomic_hashes);
                                interp.enable_parallel_guard();
                                Mutex::new(interp)
                            })
                            .collect();
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            let results: Vec<PerlValue> = list
                                .into_par_iter()
                                .map(|item| {
                                    let tid =
                                        rayon::current_thread_index().unwrap_or(0) % pool.len();
                                    let mut local_interp = pool[tid].lock();
                                    local_interp.scope.set_topic(item);
                                    let mut vm = shared.worker_vm(&mut local_interp);
                                    let mut op_count = 0u64;
                                    let val = match vm.run_block_region(start, end, &mut op_count) {
                                        Ok(v) => v,
                                        Err(_) => PerlValue::UNDEF,
                                    };
                                    pmap_progress.tick();
                                    val
                                })
                                .collect();
                            pmap_progress.finish();
                            self.push(PerlValue::array(results));
                            Ok(())
                        } else {
                            let block = self.blocks[idx].clone();
                            let results: Vec<PerlValue> = list
                                .into_par_iter()
                                .map(|item| {
                                    let tid =
                                        rayon::current_thread_index().unwrap_or(0) % pool.len();
                                    let mut local_interp = pool[tid].lock();
                                    local_interp.scope.set_topic(item);
                                    local_interp.scope_push_hook();
                                    let val = match local_interp.exec_block_no_scope(&block) {
                                        Ok(val) => val,
                                        Err(_) => PerlValue::UNDEF,
                                    };
                                    local_interp.scope_pop_hook();
                                    pmap_progress.tick();
                                    val
                                })
                                .collect();
                            pmap_progress.finish();
                            self.push(PerlValue::array(results));
                            Ok(())
                        }
                    }
                    Op::PFlatMapWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let (scope_capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        let n_workers = rayon::current_num_threads();
                        let pool: Vec<Mutex<Interpreter>> = (0..n_workers)
                            .map(|_| {
                                let mut interp = Interpreter::new();
                                interp.subs = subs.clone();
                                interp.scope.restore_capture(&scope_capture);
                                interp.scope.restore_atomics(&atomic_arrays, &atomic_hashes);
                                interp.enable_parallel_guard();
                                Mutex::new(interp)
                            })
                            .collect();
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            let mut indexed: Vec<(usize, Vec<PerlValue>)> = list
                                .into_par_iter()
                                .enumerate()
                                .map(|(i, item)| {
                                    let tid =
                                        rayon::current_thread_index().unwrap_or(0) % pool.len();
                                    let mut local_interp = pool[tid].lock();
                                    local_interp.scope.set_topic(item);
                                    let mut vm = shared.worker_vm(&mut local_interp);
                                    let mut op_count = 0u64;
                                    let val = match vm.run_block_region(start, end, &mut op_count) {
                                        Ok(v) => v,
                                        Err(_) => PerlValue::UNDEF,
                                    };
                                    let out = val.map_flatten_outputs(true);
                                    pmap_progress.tick();
                                    (i, out)
                                })
                                .collect();
                            pmap_progress.finish();
                            indexed.sort_by_key(|(i, _)| *i);
                            let results: Vec<PerlValue> =
                                indexed.into_iter().flat_map(|(_, v)| v).collect();
                            self.push(PerlValue::array(results));
                            Ok(())
                        } else {
                            let block = self.blocks[idx].clone();
                            let mut indexed: Vec<(usize, Vec<PerlValue>)> = list
                                .into_par_iter()
                                .enumerate()
                                .map(|(i, item)| {
                                    let tid =
                                        rayon::current_thread_index().unwrap_or(0) % pool.len();
                                    let mut local_interp = pool[tid].lock();
                                    local_interp.scope.set_topic(item);
                                    local_interp.scope_push_hook();
                                    let val = match local_interp.exec_block_no_scope(&block) {
                                        Ok(val) => val,
                                        Err(_) => PerlValue::UNDEF,
                                    };
                                    local_interp.scope_pop_hook();
                                    let out = val.map_flatten_outputs(true);
                                    pmap_progress.tick();
                                    (i, out)
                                })
                                .collect();
                            pmap_progress.finish();
                            indexed.sort_by_key(|(i, _)| *i);
                            let results: Vec<PerlValue> =
                                indexed.into_iter().flat_map(|(_, v)| v).collect();
                            self.push(PerlValue::array(results));
                            Ok(())
                        }
                    }
                    Op::PMapRemote { block_idx, flat } => {
                        let cluster = self.pop();
                        let list_pv = self.pop();
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let block = self.blocks[idx].clone();
                        let flat_outputs = *flat != 0;
                        let v = vm_interp_result(
                            self.interp.eval_pmap_remote(
                                cluster,
                                list_pv,
                                progress_flag,
                                &block,
                                flat_outputs,
                                self.line(),
                            ),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::Puniq => {
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let n_threads = self.interp.parallel_thread_count();
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        let out = crate::par_list::puniq_run(list, n_threads, &pmap_progress);
                        pmap_progress.finish();
                        self.push(PerlValue::array(out));
                        Ok(())
                    }
                    Op::PFirstWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        let subs = self.interp.subs.clone();
                        let (scope_capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let out = if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            crate::par_list::pfirst_run(list, &pmap_progress, |item| {
                                let mut local_interp = Interpreter::new();
                                local_interp.subs = subs.clone();
                                local_interp.scope.restore_capture(&scope_capture);
                                local_interp
                                    .scope
                                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                                local_interp.enable_parallel_guard();
                                local_interp.scope.set_topic(item);
                                let mut vm = shared.worker_vm(&mut local_interp);
                                let mut op_count = 0u64;
                                match vm.run_block_region(start, end, &mut op_count) {
                                    Ok(v) => v.is_true(),
                                    Err(_) => false,
                                }
                            })
                        } else {
                            let block = self.blocks[idx].clone();
                            crate::par_list::pfirst_run(list, &pmap_progress, |item| {
                                let mut local_interp = Interpreter::new();
                                local_interp.subs = subs.clone();
                                local_interp.scope.restore_capture(&scope_capture);
                                local_interp
                                    .scope
                                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                                local_interp.enable_parallel_guard();
                                local_interp.scope.set_topic(item);
                                local_interp.scope_push_hook();
                                let ok = match local_interp.exec_block_no_scope(&block) {
                                    Ok(v) => v.is_true(),
                                    Err(_) => false,
                                };
                                local_interp.scope_pop_hook();
                                ok
                            })
                        };
                        pmap_progress.finish();
                        self.push(out.unwrap_or(PerlValue::UNDEF));
                        Ok(())
                    }
                    Op::PAnyWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        let subs = self.interp.subs.clone();
                        let (scope_capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let b = if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            crate::par_list::pany_run(list, &pmap_progress, |item| {
                                let mut local_interp = Interpreter::new();
                                local_interp.subs = subs.clone();
                                local_interp.scope.restore_capture(&scope_capture);
                                local_interp
                                    .scope
                                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                                local_interp.enable_parallel_guard();
                                local_interp.scope.set_topic(item);
                                let mut vm = shared.worker_vm(&mut local_interp);
                                let mut op_count = 0u64;
                                match vm.run_block_region(start, end, &mut op_count) {
                                    Ok(v) => v.is_true(),
                                    Err(_) => false,
                                }
                            })
                        } else {
                            let block = self.blocks[idx].clone();
                            crate::par_list::pany_run(list, &pmap_progress, |item| {
                                let mut local_interp = Interpreter::new();
                                local_interp.subs = subs.clone();
                                local_interp.scope.restore_capture(&scope_capture);
                                local_interp
                                    .scope
                                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                                local_interp.enable_parallel_guard();
                                local_interp.scope.set_topic(item);
                                local_interp.scope_push_hook();
                                let ok = match local_interp.exec_block_no_scope(&block) {
                                    Ok(v) => v.is_true(),
                                    Err(_) => false,
                                };
                                local_interp.scope_pop_hook();
                                ok
                            })
                        };
                        pmap_progress.finish();
                        self.push(PerlValue::integer(if b { 1 } else { 0 }));
                        Ok(())
                    }
                    Op::PMapChunkedWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let chunk_n = self.pop().to_int().max(1) as usize;
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let (scope_capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let indexed_chunks: Vec<(usize, Vec<PerlValue>)> = list
                            .chunks(chunk_n)
                            .enumerate()
                            .map(|(i, c)| (i, c.to_vec()))
                            .collect();
                        let n_chunks = indexed_chunks.len();
                        let pmap_progress = PmapProgress::new(progress_flag, n_chunks);
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
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
                                        let mut vm = shared.worker_vm(&mut local_interp);
                                        let mut op_count = 0u64;
                                        let val =
                                            match vm.run_block_region(start, end, &mut op_count) {
                                                Ok(v) => v,
                                                Err(_) => PerlValue::UNDEF,
                                            };
                                        out.push(val);
                                    }
                                    pmap_progress.tick();
                                    (chunk_idx, out)
                                })
                                .collect();
                            pmap_progress.finish();
                            chunk_results.sort_by_key(|(i, _)| *i);
                            let results: Vec<PerlValue> =
                                chunk_results.into_iter().flat_map(|(_, v)| v).collect();
                            self.push(PerlValue::array(results));
                            Ok(())
                        } else {
                            let block = self.blocks[idx].clone();
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
                                        let val = match local_interp.exec_block_no_scope(&block) {
                                            Ok(val) => val,
                                            Err(_) => PerlValue::UNDEF,
                                        };
                                        local_interp.scope_pop_hook();
                                        out.push(val);
                                    }
                                    pmap_progress.tick();
                                    (chunk_idx, out)
                                })
                                .collect();
                            pmap_progress.finish();
                            chunk_results.sort_by_key(|(i, _)| *i);
                            let results: Vec<PerlValue> =
                                chunk_results.into_iter().flat_map(|(_, v)| v).collect();
                            self.push(PerlValue::array(results));
                            Ok(())
                        }
                    }
                    Op::ReduceWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let scope_capture = self.interp.scope.capture();
                        if list.is_empty() {
                            self.push(PerlValue::UNDEF);
                            return Ok(());
                        }
                        if list.len() == 1 {
                            self.push(list.into_iter().next().unwrap());
                            return Ok(());
                        }
                        let mut items = list;
                        let mut acc = items.remove(0);
                        let rest = items;
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            for b in rest {
                                let mut local_interp = Interpreter::new();
                                local_interp.subs = subs.clone();
                                local_interp.scope.restore_capture(&scope_capture);
                                let _ = local_interp.scope.set_scalar("a", acc.clone());
                                let _ = local_interp.scope.set_scalar("b", b.clone());
                                let _ = local_interp.scope.set_scalar("_0", acc);
                                let _ = local_interp.scope.set_scalar("_1", b);
                                let mut vm = shared.worker_vm(&mut local_interp);
                                let mut op_count = 0u64;
                                acc = match vm.run_block_region(start, end, &mut op_count) {
                                    Ok(v) => v,
                                    Err(_) => PerlValue::UNDEF,
                                };
                            }
                        } else {
                            let block = self.blocks[idx].clone();
                            for b in rest {
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
                        }
                        self.push(acc);
                        Ok(())
                    }
                    Op::PReduceWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let scope_capture = self.interp.scope.capture();
                        if list.is_empty() {
                            self.push(PerlValue::UNDEF);
                            return Ok(());
                        }
                        if list.len() == 1 {
                            self.push(list.into_iter().next().unwrap());
                            return Ok(());
                        }
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            let result = list
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
                                    let mut vm = shared.worker_vm(&mut local_interp);
                                    let mut op_count = 0u64;
                                    match vm.run_block_region(start, end, &mut op_count) {
                                        Ok(val) => val,
                                        Err(_) => PerlValue::UNDEF,
                                    }
                                });
                            pmap_progress.finish();
                            self.push(result.unwrap_or(PerlValue::UNDEF));
                            Ok(())
                        } else {
                            let block = self.blocks[idx].clone();
                            let result = list
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
                            self.push(result.unwrap_or(PerlValue::UNDEF));
                            Ok(())
                        }
                    }
                    Op::PReduceInitWithBlock(block_idx) => {
                        let init_val = self.pop();
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let scope_capture = self.interp.scope.capture();
                        let cap: &[(String, PerlValue)] = scope_capture.as_slice();
                        let block = self.blocks[idx].clone();
                        if list.is_empty() {
                            self.push(init_val);
                            return Ok(());
                        }
                        if list.len() == 1 {
                            let v = fold_preduce_init_step(
                                &subs,
                                cap,
                                &block,
                                preduce_init_fold_identity(&init_val),
                                list.into_iter().next().unwrap(),
                            );
                            self.push(v);
                            return Ok(());
                        }
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        let result = list
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
                        self.push(result);
                        Ok(())
                    }
                    Op::PMapReduceWithBlocks(map_idx, reduce_idx) => {
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let map_i = *map_idx as usize;
                        let reduce_i = *reduce_idx as usize;
                        let subs = self.interp.subs.clone();
                        let scope_capture = self.interp.scope.capture();
                        if list.is_empty() {
                            self.push(PerlValue::UNDEF);
                            return Ok(());
                        }
                        if list.len() == 1 {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            local_interp
                                .scope
                                .set_topic(list.into_iter().next().unwrap());
                            let map_block = self.blocks[map_i].clone();
                            let v = match local_interp.exec_block_no_scope(&map_block) {
                                Ok(v) => v,
                                Err(_) => PerlValue::UNDEF,
                            };
                            self.push(v);
                            return Ok(());
                        }
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        let map_range = self
                            .block_bytecode_ranges
                            .get(map_i)
                            .and_then(|r| r.as_ref())
                            .copied();
                        let reduce_range = self
                            .block_bytecode_ranges
                            .get(reduce_i)
                            .and_then(|r| r.as_ref())
                            .copied();
                        if let (Some((map_start, map_end)), Some((reduce_start, reduce_end))) =
                            (map_range, reduce_range)
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            let result = list
                                .into_par_iter()
                                .map(|item| {
                                    let mut local_interp = Interpreter::new();
                                    local_interp.subs = subs.clone();
                                    local_interp.scope.restore_capture(&scope_capture);
                                    local_interp.scope.set_topic(item);
                                    let mut vm = shared.worker_vm(&mut local_interp);
                                    let mut op_count = 0u64;
                                    let val = match vm.run_block_region(
                                        map_start,
                                        map_end,
                                        &mut op_count,
                                    ) {
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
                                    let mut vm = shared.worker_vm(&mut local_interp);
                                    let mut op_count = 0u64;
                                    match vm.run_block_region(
                                        reduce_start,
                                        reduce_end,
                                        &mut op_count,
                                    ) {
                                        Ok(val) => val,
                                        Err(_) => PerlValue::UNDEF,
                                    }
                                });
                            pmap_progress.finish();
                            self.push(result.unwrap_or(PerlValue::UNDEF));
                            Ok(())
                        } else {
                            let map_block = self.blocks[map_i].clone();
                            let reduce_block = self.blocks[reduce_i].clone();
                            let result = list
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
                            self.push(result.unwrap_or(PerlValue::UNDEF));
                            Ok(())
                        }
                    }
                    Op::PcacheWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let scope_capture = self.interp.scope.capture();
                        let block = self.blocks[idx].clone();
                        let cache = &*crate::pcache::GLOBAL_PCACHE;
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            let results: Vec<PerlValue> = list
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
                                    let mut vm = shared.worker_vm(&mut local_interp);
                                    let mut op_count = 0u64;
                                    let val = match vm.run_block_region(start, end, &mut op_count) {
                                        Ok(v) => v,
                                        Err(_) => PerlValue::UNDEF,
                                    };
                                    cache.insert(k, val.clone());
                                    pmap_progress.tick();
                                    val
                                })
                                .collect();
                            pmap_progress.finish();
                            self.push(PerlValue::array(results));
                            Ok(())
                        } else {
                            let results: Vec<PerlValue> = list
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
                            self.push(PerlValue::array(results));
                            Ok(())
                        }
                    }
                    Op::Pselect { n_rx, has_timeout } => {
                        let timeout = if *has_timeout {
                            let t = self.pop().to_number();
                            Some(std::time::Duration::from_secs_f64(t.max(0.0)))
                        } else {
                            None
                        };
                        let mut rx_vals = Vec::with_capacity(*n_rx as usize);
                        for _ in 0..*n_rx {
                            rx_vals.push(self.pop());
                        }
                        rx_vals.reverse();
                        let line = self.line();
                        let v = crate::pchannel::pselect_recv_with_optional_timeout(
                            &rx_vals, timeout, line,
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::PGrepWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let (scope_capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        let n_workers = rayon::current_num_threads();
                        let pool: Vec<Mutex<Interpreter>> = (0..n_workers)
                            .map(|_| {
                                let mut interp = Interpreter::new();
                                interp.subs = subs.clone();
                                interp.scope.restore_capture(&scope_capture);
                                interp.scope.restore_atomics(&atomic_arrays, &atomic_hashes);
                                interp.enable_parallel_guard();
                                Mutex::new(interp)
                            })
                            .collect();
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            let results: Vec<PerlValue> = list
                                .into_par_iter()
                                .filter_map(|item| {
                                    let tid =
                                        rayon::current_thread_index().unwrap_or(0) % pool.len();
                                    let mut local_interp = pool[tid].lock();
                                    local_interp.scope.set_topic(item.clone());
                                    let mut vm = shared.worker_vm(&mut local_interp);
                                    let mut op_count = 0u64;
                                    let keep = match vm.run_block_region(start, end, &mut op_count)
                                    {
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
                            self.push(PerlValue::array(results));
                            Ok(())
                        } else {
                            let block = self.blocks[idx].clone();
                            let results: Vec<PerlValue> = list
                                .into_par_iter()
                                .filter_map(|item| {
                                    let tid =
                                        rayon::current_thread_index().unwrap_or(0) % pool.len();
                                    let mut local_interp = pool[tid].lock();
                                    local_interp.scope.set_topic(item.clone());
                                    local_interp.scope_push_hook();
                                    let keep = match local_interp.exec_block_no_scope(&block) {
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
                            self.push(PerlValue::array(results));
                            Ok(())
                        }
                    }
                    Op::PMapsWithBlock(block_idx) => {
                        let val = self.pop();
                        let block = self.blocks[*block_idx as usize].clone();
                        let source = crate::map_stream::into_pull_iter(val);
                        let sub = self.interp.anon_coderef_from_block(&block);
                        let (capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let out = PerlValue::iterator(Arc::new(
                            crate::map_stream::PMapStreamIterator::new(
                                source,
                                sub,
                                self.interp.subs.clone(),
                                capture,
                                atomic_arrays,
                                atomic_hashes,
                                false,
                            ),
                        ));
                        self.push(out);
                        Ok(())
                    }
                    Op::PFlatMapsWithBlock(block_idx) => {
                        let val = self.pop();
                        let block = self.blocks[*block_idx as usize].clone();
                        let source = crate::map_stream::into_pull_iter(val);
                        let sub = self.interp.anon_coderef_from_block(&block);
                        let (capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let out = PerlValue::iterator(Arc::new(
                            crate::map_stream::PMapStreamIterator::new(
                                source,
                                sub,
                                self.interp.subs.clone(),
                                capture,
                                atomic_arrays,
                                atomic_hashes,
                                true,
                            ),
                        ));
                        self.push(out);
                        Ok(())
                    }
                    Op::PGrepsWithBlock(block_idx) => {
                        let val = self.pop();
                        let block = self.blocks[*block_idx as usize].clone();
                        let source = crate::map_stream::into_pull_iter(val);
                        let sub = self.interp.anon_coderef_from_block(&block);
                        let (capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let out = PerlValue::iterator(Arc::new(
                            crate::map_stream::PGrepStreamIterator::new(
                                source,
                                sub,
                                self.interp.subs.clone(),
                                capture,
                                atomic_arrays,
                                atomic_hashes,
                            ),
                        ));
                        self.push(out);
                        Ok(())
                    }
                    Op::PForWithBlock(block_idx) => {
                        let line = self.line();
                        let list = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let pmap_progress = PmapProgress::new(progress_flag, list.len());
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let (scope_capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let first_err: Arc<Mutex<Option<PerlError>>> = Arc::new(Mutex::new(None));
                        let n_workers = rayon::current_num_threads();
                        let pool: Vec<Mutex<Interpreter>> = (0..n_workers)
                            .map(|_| {
                                let mut interp = Interpreter::new();
                                interp.subs = subs.clone();
                                interp.scope.restore_capture(&scope_capture);
                                interp.scope.restore_atomics(&atomic_arrays, &atomic_hashes);
                                interp.enable_parallel_guard();
                                Mutex::new(interp)
                            })
                            .collect();
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            list.into_par_iter().for_each(|item| {
                                if first_err.lock().is_some() {
                                    return;
                                }
                                let tid = rayon::current_thread_index().unwrap_or(0) % pool.len();
                                let mut local_interp = pool[tid].lock();
                                local_interp.scope.set_topic(item);
                                let mut vm = shared.worker_vm(&mut local_interp);
                                let mut op_count = 0u64;
                                match vm.run_block_region(start, end, &mut op_count) {
                                    Ok(_) => {}
                                    Err(e) => {
                                        let mut g = first_err.lock();
                                        if g.is_none() {
                                            *g = Some(e);
                                        }
                                    }
                                }
                                pmap_progress.tick();
                            });
                        } else {
                            let block = self.blocks[idx].clone();
                            list.into_par_iter().for_each(|item| {
                                if first_err.lock().is_some() {
                                    return;
                                }
                                let tid = rayon::current_thread_index().unwrap_or(0) % pool.len();
                                let mut local_interp = pool[tid].lock();
                                local_interp.scope.set_topic(item);
                                local_interp.scope_push_hook();
                                match local_interp.exec_block_no_scope(&block) {
                                    Ok(_) => {}
                                    Err(e) => {
                                        let stryke = match e {
                                            FlowOrError::Error(stryke) => stryke,
                                            FlowOrError::Flow(_) => PerlError::runtime(
                                                "return/last/next/redo not supported inside pfor block",
                                                line,
                                            ),
                                        };
                                        let mut g = first_err.lock();
                                        if g.is_none() {
                                            *g = Some(stryke);
                                        }
                                    }
                                }
                                local_interp.scope_pop_hook();
                                pmap_progress.tick();
                            });
                        }
                        pmap_progress.finish();
                        if let Some(e) = first_err.lock().take() {
                            return Err(e);
                        }
                        self.push(PerlValue::UNDEF);
                        Ok(())
                    }
                    Op::PSortWithBlock(block_idx) => {
                        let mut items = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let pmap_progress = PmapProgress::new(progress_flag, 2);
                        pmap_progress.tick();
                        let idx = *block_idx as usize;
                        let subs = self.interp.subs.clone();
                        let (scope_capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            items.par_sort_by(|a, b| {
                                let mut local_interp = Interpreter::new();
                                local_interp.subs = subs.clone();
                                local_interp.scope.restore_capture(&scope_capture);
                                local_interp
                                    .scope
                                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                                local_interp.enable_parallel_guard();
                                let _ = local_interp.scope.set_scalar("a", a.clone());
                                let _ = local_interp.scope.set_scalar("b", b.clone());
                                let _ = local_interp.scope.set_scalar("_0", a.clone());
                                let _ = local_interp.scope.set_scalar("_1", b.clone());
                                let mut vm = shared.worker_vm(&mut local_interp);
                                let mut op_count = 0u64;
                                match vm.run_block_region(start, end, &mut op_count) {
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
                            let block = self.blocks[idx].clone();
                            items.par_sort_by(|a, b| {
                                let mut local_interp = Interpreter::new();
                                local_interp.subs = subs.clone();
                                local_interp.scope.restore_capture(&scope_capture);
                                local_interp
                                    .scope
                                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                                local_interp.enable_parallel_guard();
                                let _ = local_interp.scope.set_scalar("a", a.clone());
                                let _ = local_interp.scope.set_scalar("b", b.clone());
                                let _ = local_interp.scope.set_scalar("_0", a.clone());
                                let _ = local_interp.scope.set_scalar("_1", b.clone());
                                local_interp.scope_push_hook();
                                let ord = match local_interp.exec_block_no_scope(&block) {
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
                        pmap_progress.tick();
                        pmap_progress.finish();
                        self.push(PerlValue::array(items));
                        Ok(())
                    }
                    Op::PSortWithBlockFast(tag) => {
                        let mut items = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let pmap_progress = PmapProgress::new(progress_flag, 2);
                        pmap_progress.tick();
                        let mode = match *tag {
                            0 => SortBlockFast::Numeric,
                            1 => SortBlockFast::String,
                            2 => SortBlockFast::NumericRev,
                            3 => SortBlockFast::StringRev,
                            _ => SortBlockFast::Numeric,
                        };
                        items.par_sort_by(|a, b| sort_magic_cmp(a, b, mode));
                        pmap_progress.tick();
                        pmap_progress.finish();
                        self.push(PerlValue::array(items));
                        Ok(())
                    }
                    Op::PSortNoBlockParallel => {
                        let mut items = self.pop().to_list();
                        let progress_flag = self.pop().is_true();
                        let pmap_progress = PmapProgress::new(progress_flag, 2);
                        pmap_progress.tick();
                        items.par_sort_by(|a, b| a.to_string().cmp(&b.to_string()));
                        pmap_progress.tick();
                        pmap_progress.finish();
                        self.push(PerlValue::array(items));
                        Ok(())
                    }
                    Op::FanWithBlock(block_idx) => {
                        let line = self.line();
                        let n = self.pop().to_int().max(0) as usize;
                        let progress_flag = self.pop().is_true();
                        self.run_fan_block(*block_idx, n, line, progress_flag)?;
                        Ok(())
                    }
                    Op::FanWithBlockAuto(block_idx) => {
                        let line = self.line();
                        let n = self.interp.parallel_thread_count();
                        let progress_flag = self.pop().is_true();
                        self.run_fan_block(*block_idx, n, line, progress_flag)?;
                        Ok(())
                    }
                    Op::FanCapWithBlock(block_idx) => {
                        let line = self.line();
                        let n = self.pop().to_int().max(0) as usize;
                        let progress_flag = self.pop().is_true();
                        self.run_fan_cap_block(*block_idx, n, line, progress_flag)?;
                        Ok(())
                    }
                    Op::FanCapWithBlockAuto(block_idx) => {
                        let line = self.line();
                        let n = self.interp.parallel_thread_count();
                        let progress_flag = self.pop().is_true();
                        self.run_fan_cap_block(*block_idx, n, line, progress_flag)?;
                        Ok(())
                    }

                    Op::AsyncBlock(block_idx) => {
                        let block = self.blocks[*block_idx as usize].clone();
                        let subs = self.interp.subs.clone();
                        let (scope_capture, atomic_arrays, atomic_hashes) =
                            self.interp.scope.capture_with_atomics();
                        let result_slot: Arc<Mutex<Option<PerlResult<PerlValue>>>> =
                            Arc::new(Mutex::new(None));
                        let join_slot: Arc<Mutex<Option<std::thread::JoinHandle<()>>>> =
                            Arc::new(Mutex::new(None));
                        let rs = Arc::clone(&result_slot);
                        let h = std::thread::spawn(move || {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs;
                            local_interp.scope.restore_capture(&scope_capture);
                            local_interp
                                .scope
                                .restore_atomics(&atomic_arrays, &atomic_hashes);
                            local_interp.enable_parallel_guard();
                            local_interp.scope_push_hook();
                            let out = match local_interp.exec_block_no_scope(&block) {
                                Ok(v) => Ok(v),
                                Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
                                Err(FlowOrError::Error(e)) => Err(e),
                                Err(_) => Ok(PerlValue::UNDEF),
                            };
                            local_interp.scope_pop_hook();
                            *rs.lock() = Some(out);
                        });
                        *join_slot.lock() = Some(h);
                        self.push(PerlValue::async_task(Arc::new(PerlAsyncTask {
                            result: result_slot,
                            join: join_slot,
                        })));
                        Ok(())
                    }
                    Op::Await => {
                        let v = self.pop();
                        if let Some(t) = v.as_async_task() {
                            let r = t.await_result();
                            self.push(r?);
                        } else {
                            self.push(v);
                        }
                        Ok(())
                    }

                    Op::LoadCurrentSub => {
                        if let Some(sub) = self.interp.current_sub_stack.last().cloned() {
                            self.push(PerlValue::code_ref(sub));
                        } else {
                            self.push(PerlValue::UNDEF);
                        }
                        Ok(())
                    }

                    Op::DeferBlock => {
                        let coderef = self.pop();
                        self.interp.scope.push_defer(coderef);
                        Ok(())
                    }

                    // ── try / catch / finally ──
                    Op::TryPush { .. } => {
                        self.try_stack.push(TryFrame {
                            try_push_op_idx: self.ip - 1,
                        });
                        Ok(())
                    }
                    Op::TryContinueNormal => {
                        let frame = self.try_stack.last().ok_or_else(|| {
                            PerlError::runtime("TryContinueNormal without active try", self.line())
                        })?;
                        let Op::TryPush {
                            finally_ip,
                            after_ip,
                            ..
                        } = &self.ops[frame.try_push_op_idx]
                        else {
                            return Err(PerlError::runtime(
                                "TryContinueNormal: corrupt try frame",
                                self.line(),
                            ));
                        };
                        if let Some(fin_ip) = *finally_ip {
                            self.ip = fin_ip;
                            Ok(())
                        } else {
                            self.try_stack.pop();
                            self.ip = *after_ip;
                            Ok(())
                        }
                    }
                    Op::TryFinallyEnd => {
                        let frame = self.try_stack.pop().ok_or_else(|| {
                            PerlError::runtime("TryFinallyEnd without active try", self.line())
                        })?;
                        let Op::TryPush { after_ip, .. } = &self.ops[frame.try_push_op_idx] else {
                            return Err(PerlError::runtime(
                                "TryFinallyEnd: corrupt try frame",
                                self.line(),
                            ));
                        };
                        self.ip = *after_ip;
                        Ok(())
                    }
                    Op::CatchReceive(idx) => {
                        let msg = self.pending_catch_error.take().ok_or_else(|| {
                            PerlError::runtime(
                                "CatchReceive without pending exception",
                                self.line(),
                            )
                        })?;
                        let n = names[*idx as usize].as_str();
                        self.interp.scope_pop_hook();
                        self.interp.scope_push_hook();
                        self.interp.scope.declare_scalar(n, PerlValue::string(msg));
                        self.interp.english_note_lexical_scalar(n);
                        Ok(())
                    }

                    Op::DeclareMySyncScalar(name_idx) => {
                        let val = self.pop();
                        let n = names[*name_idx as usize].as_str();
                        let stored = if val.is_mysync_deque_or_heap() {
                            val
                        } else {
                            PerlValue::atomic(Arc::new(Mutex::new(val)))
                        };
                        self.interp.scope.declare_scalar(n, stored);
                        Ok(())
                    }
                    Op::DeclareMySyncArray(name_idx) => {
                        let val = self.pop();
                        let n = names[*name_idx as usize].as_str();
                        self.interp.scope.declare_atomic_array(n, val.to_list());
                        Ok(())
                    }
                    Op::DeclareMySyncHash(name_idx) => {
                        let val = self.pop();
                        let n = names[*name_idx as usize].as_str();
                        let items = val.to_list();
                        let mut map = IndexMap::new();
                        let mut i = 0usize;
                        while i + 1 < items.len() {
                            map.insert(items[i].to_string(), items[i + 1].clone());
                            i += 2;
                        }
                        self.interp.scope.declare_atomic_hash(n, map);
                        Ok(())
                    }
                    Op::RuntimeSubDecl(idx) => {
                        let rs = &self.runtime_sub_decls[*idx as usize];
                        let key = self.interp.qualify_sub_key(&rs.name);
                        let captured = self.interp.scope.capture();
                        let closure_env = if captured.is_empty() {
                            None
                        } else {
                            Some(captured)
                        };
                        let mut sub = PerlSub {
                            name: rs.name.clone(),
                            params: rs.params.clone(),
                            body: rs.body.clone(),
                            closure_env,
                            prototype: rs.prototype.clone(),
                            fib_like: None,
                        };
                        sub.fib_like = crate::fib_like_tail::detect_fib_like_recursive_add(&sub);
                        self.interp.subs.insert(key, Arc::new(sub));
                        Ok(())
                    }
                    Op::Tie {
                        target_kind,
                        name_idx,
                        argc,
                    } => {
                        let argc = *argc as usize;
                        let mut stack_vals = Vec::with_capacity(argc);
                        for _ in 0..argc {
                            stack_vals.push(self.pop());
                        }
                        stack_vals.reverse();
                        let name = names[*name_idx as usize].as_str();
                        let line = self.line();
                        self.interp
                            .tie_execute(*target_kind, name, stack_vals, line)
                            .map_err(|e| e.at_line(line))?;
                        Ok(())
                    }
                    Op::FormatDecl(idx) => {
                        let (basename, lines) = &self.format_decls[*idx as usize];
                        let line = self.line();
                        self.interp
                            .install_format_decl(basename.as_str(), lines, line)
                            .map_err(|e| e.at_line(line))?;
                        Ok(())
                    }
                    Op::UseOverload(idx) => {
                        let pairs = &self.use_overload_entries[*idx as usize];
                        self.interp.install_use_overload_pairs(pairs);
                        Ok(())
                    }
                    Op::ScalarCompoundAssign { name_idx, op: op_b } => {
                        let rhs = self.pop();
                        let n = names[*name_idx as usize].as_str();
                        let op = scalar_compound_op_from_byte(*op_b).ok_or_else(|| {
                            PerlError::runtime("ScalarCompoundAssign: invalid op byte", self.line())
                        })?;
                        let en = self.interp.english_scalar_name(n);
                        let val = self
                            .interp
                            .scalar_compound_assign_scalar_target(en, op, rhs)
                            .map_err(|e| e.at_line(self.line()))?;
                        self.push(val);
                        Ok(())
                    }

                    Op::SetGlobalPhase(phase) => {
                        let s = match *phase {
                            crate::bytecode::GP_START => "START",
                            crate::bytecode::GP_UNITCHECK => "UNITCHECK",
                            crate::bytecode::GP_CHECK => "CHECK",
                            crate::bytecode::GP_INIT => "INIT",
                            crate::bytecode::GP_RUN => "RUN",
                            crate::bytecode::GP_END => "END",
                            _ => {
                                return Err(PerlError::runtime(
                                    format!("SetGlobalPhase: invalid phase byte {}", phase),
                                    self.line(),
                                ));
                            }
                        };
                        self.interp.global_phase = s.to_string();
                        Ok(())
                    }

                    // ── Halt ──
                    Op::Halt => {
                        self.halt = true;
                        Ok(())
                    }
                }
            })();
            if let (Some(prof), Some(t0)) = (&mut self.interp.profiler, op_prof_t0) {
                prof.on_line(&self.interp.file, line, t0.elapsed());
            }
            if let Err(e) = __op_res {
                if self.try_recover_from_exception(&e)? {
                    continue;
                }
                return Err(e);
            }
            // Blessed refcount drops enqueue from `PerlValue::drop`; drain before the next opcode
            // so `$x = undef; f()` runs `DESTROY` before `f` (Perl semantics).
            if crate::pending_destroy::pending_destroy_vm_sync_needed() {
                self.interp.drain_pending_destroys(line)?;
            }
            if self.exit_main_dispatch {
                if let Some(v) = self.exit_main_dispatch_value.take() {
                    last = v;
                }
                break;
            }
            if self.halt {
                break;
            }
        }

        if !self.stack.is_empty() {
            last = self.stack.last().cloned().unwrap_or(PerlValue::UNDEF);
            // Drain iterators left on the stack so side effects fire
            // (e.g. `pmaps { system(...) } @list` with no consumer).
            if last.is_iterator() {
                let iter = last.clone().into_iterator();
                while iter.next_item().is_some() {}
                last = PerlValue::UNDEF;
            }
        }

        Ok(last)
    }

    /// Called from Cranelift (`stryke_jit_call_sub`) to run a compiled sub by bytecode IP with `i64` args.
    pub(crate) fn jit_trampoline_run_sub(
        &mut self,
        entry_ip: usize,
        want: WantarrayCtx,
        args: &[i64],
    ) -> PerlResult<PerlValue> {
        let saved_wa = self.interp.wantarray_kind;
        for a in args {
            self.push(PerlValue::integer(*a));
        }
        let stack_base = self.stack.len() - args.len();
        let mut sub_prof_t0 = None;
        if let Some(nidx) = self.sub_entry_name_idx(entry_ip) {
            sub_prof_t0 = self.interp.profiler.is_some().then(std::time::Instant::now);
            if let Some(p) = &mut self.interp.profiler {
                p.enter_sub(self.names[nidx as usize].as_str());
            }
        }
        self.call_stack.push(CallFrame {
            return_ip: 0,
            stack_base,
            scope_depth: self.interp.scope.depth(),
            saved_wantarray: saved_wa,
            jit_trampoline_return: true,
            block_region: false,
            sub_profiler_start: sub_prof_t0,
        });
        self.interp.wantarray_kind = want;
        self.interp.scope_push_hook();
        if let Some(nidx) = self.sub_entry_name_idx(entry_ip) {
            let nm = self.names[nidx as usize].as_str();
            if let Some(sub) = self.interp.subs.get(nm).cloned() {
                if let Some(ref env) = sub.closure_env {
                    self.interp.scope.restore_capture(env);
                }
            }
        }
        self.ip = entry_ip;
        self.jit_trampoline_out = None;
        self.jit_trampoline_depth = self.jit_trampoline_depth.saturating_add(1);
        let mut op_count = 0u64;
        let last = PerlValue::UNDEF;
        let r = self.run_main_dispatch_loop(last, &mut op_count, true);
        self.jit_trampoline_depth = self.jit_trampoline_depth.saturating_sub(1);
        r?;
        self.jit_trampoline_out.take().ok_or_else(|| {
            PerlError::runtime("JIT trampoline: subroutine did not return", self.line())
        })
    }

    #[inline]
    fn find_sub_entry(&self, name_idx: u16) -> Option<(usize, bool)> {
        self.sub_entry_by_name.get(&name_idx).copied()
    }

    /// Name pool index for a compiled sub entry IP (for closure env + JIT trampoline).
    fn sub_entry_name_idx(&self, entry_ip: usize) -> Option<u16> {
        for &(n, ip, _) in &self.sub_entries {
            if ip == entry_ip {
                return Some(n);
            }
        }
        None
    }

    fn exec_builtin(&mut self, id: u16, args: Vec<PerlValue>) -> PerlResult<PerlValue> {
        let line = self.line();
        let bid = BuiltinId::from_u16(id);
        match bid {
            Some(BuiltinId::Length) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
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
            Some(BuiltinId::Defined) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::integer(if val.is_undef() { 0 } else { 1 }))
            }
            Some(BuiltinId::Abs) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::float(val.to_number().abs()))
            }
            Some(BuiltinId::Int) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::integer(val.to_number() as i64))
            }
            Some(BuiltinId::Sqrt) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::float(val.to_number().sqrt()))
            }
            Some(BuiltinId::Sin) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::float(val.to_number().sin()))
            }
            Some(BuiltinId::Cos) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::float(val.to_number().cos()))
            }
            Some(BuiltinId::Atan2) => {
                let mut it = args.into_iter();
                let y = it.next().unwrap_or(PerlValue::UNDEF);
                let x = it.next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::float(y.to_number().atan2(x.to_number())))
            }
            Some(BuiltinId::Exp) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::float(val.to_number().exp()))
            }
            Some(BuiltinId::Log) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::float(val.to_number().ln()))
            }
            Some(BuiltinId::Rand) => {
                let upper = match args.len() {
                    0 => 1.0,
                    _ => args[0].to_number(),
                };
                Ok(PerlValue::float(self.interp.perl_rand(upper)))
            }
            Some(BuiltinId::Srand) => {
                let seed = match args.len() {
                    0 => None,
                    _ => Some(args[0].to_number()),
                };
                Ok(PerlValue::integer(self.interp.perl_srand(seed)))
            }
            Some(BuiltinId::Crypt) => {
                let mut it = args.into_iter();
                let p = it.next().unwrap_or(PerlValue::UNDEF).to_string();
                let salt = it.next().unwrap_or(PerlValue::UNDEF).to_string();
                Ok(PerlValue::string(crate::crypt_util::perl_crypt(&p, &salt)))
            }
            Some(BuiltinId::Fc) => {
                let s = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::string(default_case_fold_str(&s.to_string())))
            }
            Some(BuiltinId::Pos) => {
                let key = if args.is_empty() {
                    "_".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(self
                    .interp
                    .regex_pos
                    .get(&key)
                    .copied()
                    .flatten()
                    .map(|n| PerlValue::integer(n as i64))
                    .unwrap_or(PerlValue::UNDEF))
            }
            Some(BuiltinId::Study) => {
                let s = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(Interpreter::study_return_value(&s.to_string()))
            }
            Some(BuiltinId::Chr) => {
                let n = args.into_iter().next().unwrap_or(PerlValue::UNDEF).to_int() as u32;
                Ok(PerlValue::string(
                    char::from_u32(n).map(|c| c.to_string()).unwrap_or_default(),
                ))
            }
            Some(BuiltinId::Ord) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                Ok(PerlValue::integer(
                    s.chars().next().map(|c| c as i64).unwrap_or(0),
                ))
            }
            Some(BuiltinId::Hex) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
                Ok(PerlValue::integer(
                    i64::from_str_radix(clean, 16).unwrap_or(0),
                ))
            }
            Some(BuiltinId::Oct) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                let s = s.trim();
                let n = if s.starts_with("0x") || s.starts_with("0X") {
                    i64::from_str_radix(&s[2..], 16).unwrap_or(0)
                } else if s.starts_with("0b") || s.starts_with("0B") {
                    i64::from_str_radix(&s[2..], 2).unwrap_or(0)
                } else if s.starts_with("0o") || s.starts_with("0O") {
                    i64::from_str_radix(&s[2..], 8).unwrap_or(0)
                } else {
                    i64::from_str_radix(s.trim_start_matches('0'), 8).unwrap_or(0)
                };
                Ok(PerlValue::integer(n))
            }
            Some(BuiltinId::Uc) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                Ok(PerlValue::string(s.to_uppercase()))
            }
            Some(BuiltinId::Lc) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                Ok(PerlValue::string(s.to_lowercase()))
            }
            Some(BuiltinId::Ref) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(val.ref_type())
            }
            Some(BuiltinId::Scalar) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(val.scalar_context())
            }
            Some(BuiltinId::Join) => {
                let mut iter = args.into_iter();
                let sep = iter.next().unwrap_or(PerlValue::UNDEF).to_string();
                let list = iter.next().unwrap_or(PerlValue::UNDEF).to_list();
                let mut strs = Vec::with_capacity(list.len());
                for v in list {
                    let s = match self.interp.stringify_value(v, line) {
                        Ok(s) => s,
                        Err(FlowOrError::Error(e)) => return Err(e),
                        Err(FlowOrError::Flow(_)) => {
                            return Err(PerlError::runtime("join: unexpected control flow", line));
                        }
                    };
                    strs.push(s);
                }
                Ok(PerlValue::string(strs.join(&sep)))
            }
            Some(BuiltinId::Split) => {
                let mut iter = args.into_iter();
                let pat = iter
                    .next()
                    .unwrap_or(PerlValue::string(" ".into()))
                    .to_string();
                let s = iter.next().unwrap_or(PerlValue::UNDEF).to_string();
                let lim = iter.next().map(|v| v.to_int() as usize);
                let re =
                    regex::Regex::new(&pat).unwrap_or_else(|_| regex::Regex::new(" ").unwrap());
                let parts: Vec<PerlValue> = if let Some(l) = lim {
                    re.splitn(&s, l)
                        .map(|p| PerlValue::string(p.to_string()))
                        .collect()
                } else {
                    re.split(&s)
                        .map(|p| PerlValue::string(p.to_string()))
                        .collect()
                };
                Ok(PerlValue::array(parts))
            }
            Some(BuiltinId::Sprintf) => {
                // sprintf arg list is Perl list context; flatten ranges / arrays / reverse
                // output into individual format arguments (same splatting as printf).
                let mut flat: Vec<PerlValue> = Vec::with_capacity(args.len());
                for a in args.into_iter() {
                    if let Some(items) = a.as_array_vec() {
                        flat.extend(items);
                    } else {
                        flat.push(a);
                    }
                }
                let args = flat;
                if args.is_empty() {
                    return Ok(PerlValue::string(String::new()));
                }
                let fmt = args[0].to_string();
                let rest = &args[1..];
                match self.interp.perl_sprintf_stringify(&fmt, rest, line) {
                    Ok(s) => Ok(PerlValue::string(s)),
                    Err(FlowOrError::Error(e)) => Err(e),
                    Err(FlowOrError::Flow(_)) => {
                        Err(PerlError::runtime("sprintf: unexpected control flow", line))
                    }
                }
            }
            Some(BuiltinId::Reverse) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(if let Some(mut a) = val.as_array_vec() {
                    a.reverse();
                    PerlValue::array(a)
                } else if let Some(s) = val.as_str() {
                    PerlValue::string(s.chars().rev().collect())
                } else {
                    PerlValue::string(val.to_string().chars().rev().collect())
                })
            }
            Some(BuiltinId::Die) => {
                let mut msg = String::new();
                for a in &args {
                    msg.push_str(&a.to_string());
                }
                if msg.is_empty() {
                    msg = "Died".to_string();
                }
                if !msg.ends_with('\n') {
                    msg.push_str(&self.interp.die_warn_at_suffix(line));
                    msg.push('\n');
                }
                Err(PerlError::die(msg, line))
            }
            Some(BuiltinId::Warn) => {
                let mut msg = String::new();
                for a in &args {
                    msg.push_str(&a.to_string());
                }
                if msg.is_empty() {
                    msg = "Warning: something's wrong".to_string();
                }
                if !msg.ends_with('\n') {
                    msg.push_str(&self.interp.die_warn_at_suffix(line));
                    msg.push('\n');
                }
                eprint!("{}", msg);
                Ok(PerlValue::integer(1))
            }
            Some(BuiltinId::Exit) => {
                let code = args
                    .into_iter()
                    .next()
                    .map(|v| v.to_int() as i32)
                    .unwrap_or(0);
                Err(PerlError::new(
                    ErrorKind::Exit(code),
                    "",
                    line,
                    &self.interp.file,
                ))
            }
            Some(BuiltinId::System) => {
                let cmd = args
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .status();
                match status {
                    Ok(s) => {
                        self.interp.record_child_exit_status(s);
                        Ok(PerlValue::integer(s.code().unwrap_or(-1) as i64))
                    }
                    Err(e) => {
                        self.interp.errno = e.to_string();
                        self.interp.child_exit_status = -1;
                        Ok(PerlValue::integer(-1))
                    }
                }
            }
            Some(BuiltinId::Ssh) => self.interp.ssh_builtin_execute(&args),
            Some(BuiltinId::Chomp) => {
                // Chomp modifies the variable in-place — but in CallBuiltin we get the value, not a reference.
                // Return the number of chars removed (like Perl).
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                let s = val.to_string();
                Ok(PerlValue::integer(if s.ends_with('\n') { 1 } else { 0 }))
            }
            Some(BuiltinId::Chop) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                let s = val.to_string();
                Ok(s.chars()
                    .last()
                    .map(|c| PerlValue::string(c.to_string()))
                    .unwrap_or(PerlValue::UNDEF))
            }
            Some(BuiltinId::Substr) => {
                let s = args.first().map(|v| v.to_string()).unwrap_or_default();
                let slen = s.len() as i64;
                let off = args.get(1).map(|v| v.to_int()).unwrap_or(0);
                let start = if off < 0 { (slen + off).max(0) } else { off }.min(slen) as usize;

                let end = if let Some(l_val) = args.get(2) {
                    let l = l_val.to_int();
                    if l < 0 {
                        (slen + l).max(start as i64)
                    } else {
                        (start as i64 + l).min(slen)
                    }
                } else {
                    slen
                } as usize;

                Ok(PerlValue::string(
                    s.get(start..end).unwrap_or("").to_string(),
                ))
            }
            Some(BuiltinId::Index) => {
                let s = args.first().map(|v| v.to_string()).unwrap_or_default();
                let sub = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                let pos = args.get(2).map(|v| v.to_int() as usize).unwrap_or(0);
                Ok(PerlValue::integer(
                    s[pos..].find(&sub).map(|i| (i + pos) as i64).unwrap_or(-1),
                ))
            }
            Some(BuiltinId::Rindex) => {
                let s = args.first().map(|v| v.to_string()).unwrap_or_default();
                let sub = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                let end = args
                    .get(2)
                    .map(|v| v.to_int() as usize + sub.len())
                    .unwrap_or(s.len());
                Ok(PerlValue::integer(
                    s[..end.min(s.len())]
                        .rfind(&sub)
                        .map(|i| i as i64)
                        .unwrap_or(-1),
                ))
            }
            Some(BuiltinId::Ucfirst) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                let mut chars = s.chars();
                let result = match chars.next() {
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                };
                Ok(PerlValue::string(result))
            }
            Some(BuiltinId::Lcfirst) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                let mut chars = s.chars();
                let result = match chars.next() {
                    Some(c) => c.to_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                };
                Ok(PerlValue::string(result))
            }
            Some(BuiltinId::Splice) => self.interp.splice_builtin_execute(&args, line),
            Some(BuiltinId::Unshift) => self.interp.unshift_builtin_execute(&args, line),
            Some(BuiltinId::Printf) => {
                // Flatten list-context operands (ranges, arrays, `reverse`, …) so format
                // placeholders line up with individual values instead of an array reference.
                let mut flat: Vec<PerlValue> = Vec::with_capacity(args.len());
                for a in args.into_iter() {
                    if let Some(items) = a.as_array_vec() {
                        flat.extend(items);
                    } else {
                        flat.push(a);
                    }
                }
                let args = flat;
                let (fmt, rest): (String, &[PerlValue]) = if args.is_empty() {
                    let s = match self
                        .interp
                        .stringify_value(self.interp.scope.get_scalar("_").clone(), line)
                    {
                        Ok(s) => s,
                        Err(FlowOrError::Error(e)) => return Err(e),
                        Err(FlowOrError::Flow(_)) => {
                            return Err(PerlError::runtime(
                                "printf: unexpected control flow",
                                line,
                            ));
                        }
                    };
                    (s, &[])
                } else {
                    (args[0].to_string(), &args[1..])
                };
                let out = match self.interp.perl_sprintf_stringify(&fmt, rest, line) {
                    Ok(s) => s,
                    Err(FlowOrError::Error(e)) => return Err(e),
                    Err(FlowOrError::Flow(_)) => {
                        return Err(PerlError::runtime("printf: unexpected control flow", line));
                    }
                };
                print!("{}", out);
                if self.interp.output_autoflush {
                    let _ = io::stdout().flush();
                }
                Ok(PerlValue::integer(1))
            }
            Some(BuiltinId::Open) => {
                if args.len() < 2 {
                    return Err(PerlError::runtime(
                        "open requires at least 2 arguments",
                        line,
                    ));
                }
                let handle_name = args[0].to_string();
                let mode_s = args[1].to_string();
                let file_opt = args.get(2).map(|v| v.to_string());
                self.interp
                    .open_builtin_execute(handle_name, mode_s, file_opt, line)
            }
            Some(BuiltinId::Close) => {
                let name = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                self.interp.close_builtin_execute(name)
            }
            Some(BuiltinId::Eof) => self.interp.eof_builtin_execute(&args, line),
            Some(BuiltinId::ReadLine) => {
                let h = if args.is_empty() {
                    None
                } else {
                    Some(args[0].to_string())
                };
                self.interp.readline_builtin_execute(h.as_deref())
            }
            Some(BuiltinId::ReadLineList) => {
                let h = if args.is_empty() {
                    None
                } else {
                    Some(args[0].to_string())
                };
                self.interp.readline_builtin_execute_list(h.as_deref())
            }
            Some(BuiltinId::Exec) => {
                let cmd = args
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .status();
                std::process::exit(status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1));
            }
            Some(BuiltinId::Chdir) => {
                let path = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                Ok(PerlValue::integer(
                    if std::env::set_current_dir(&path).is_ok() {
                        1
                    } else {
                        0
                    },
                ))
            }
            Some(BuiltinId::Mkdir) => {
                let path = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(PerlValue::integer(if std::fs::create_dir(&path).is_ok() {
                    1
                } else {
                    0
                }))
            }
            Some(BuiltinId::Unlink) => {
                let mut count = 0i64;
                for a in &args {
                    if std::fs::remove_file(a.to_string()).is_ok() {
                        count += 1;
                    }
                }
                Ok(PerlValue::integer(count))
            }
            Some(BuiltinId::Rmdir) => self.interp.builtin_rmdir_execute(&args, line),
            Some(BuiltinId::Utime) => self.interp.builtin_utime_execute(&args, line),
            Some(BuiltinId::Umask) => self.interp.builtin_umask_execute(&args, line),
            Some(BuiltinId::Getcwd) => self.interp.builtin_getcwd_execute(&args, line),
            Some(BuiltinId::Pipe) => self.interp.builtin_pipe_execute(&args, line),
            Some(BuiltinId::Rename) => {
                let old = args.first().map(|v| v.to_string()).unwrap_or_default();
                let new = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                Ok(crate::perl_fs::rename_paths(&old, &new))
            }
            Some(BuiltinId::Chmod) => {
                if args.is_empty() {
                    return Ok(PerlValue::integer(0));
                }
                let mode = args[0].to_int();
                let paths: Vec<String> = args.iter().skip(1).map(|v| v.to_string()).collect();
                Ok(PerlValue::integer(crate::perl_fs::chmod_paths(
                    &paths, mode,
                )))
            }
            Some(BuiltinId::Chown) => {
                if args.len() < 3 {
                    return Ok(PerlValue::integer(0));
                }
                let uid = args[0].to_int();
                let gid = args[1].to_int();
                let paths: Vec<String> = args.iter().skip(2).map(|v| v.to_string()).collect();
                Ok(PerlValue::integer(crate::perl_fs::chown_paths(
                    &paths, uid, gid,
                )))
            }
            Some(BuiltinId::Stat) => {
                let path = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(crate::perl_fs::stat_path(&path, false))
            }
            Some(BuiltinId::Lstat) => {
                let path = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(crate::perl_fs::stat_path(&path, true))
            }
            Some(BuiltinId::Link) => {
                let old = args.first().map(|v| v.to_string()).unwrap_or_default();
                let new = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                Ok(crate::perl_fs::link_hard(&old, &new))
            }
            Some(BuiltinId::Symlink) => {
                let old = args.first().map(|v| v.to_string()).unwrap_or_default();
                let new = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                Ok(crate::perl_fs::link_sym(&old, &new))
            }
            Some(BuiltinId::Readlink) => {
                let path = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(crate::perl_fs::read_link(&path))
            }
            Some(BuiltinId::Glob) => {
                let pats: Vec<String> = args.iter().map(|v| v.to_string()).collect();
                Ok(crate::perl_fs::glob_patterns(&pats))
            }
            Some(BuiltinId::Files) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(crate::perl_fs::list_files(&dir))
            }
            Some(BuiltinId::Filesf) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(crate::perl_fs::list_filesf(&dir))
            }
            Some(BuiltinId::FilesfRecursive) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(PerlValue::iterator(std::sync::Arc::new(
                    crate::value::FsWalkIterator::new(&dir, true),
                )))
            }
            Some(BuiltinId::Dirs) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(crate::perl_fs::list_dirs(&dir))
            }
            Some(BuiltinId::DirsRecursive) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(PerlValue::iterator(std::sync::Arc::new(
                    crate::value::FsWalkIterator::new(&dir, false),
                )))
            }
            Some(BuiltinId::SymLinks) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(crate::perl_fs::list_sym_links(&dir))
            }
            Some(BuiltinId::Sockets) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(crate::perl_fs::list_sockets(&dir))
            }
            Some(BuiltinId::Pipes) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(crate::perl_fs::list_pipes(&dir))
            }
            Some(BuiltinId::BlockDevices) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(crate::perl_fs::list_block_devices(&dir))
            }
            Some(BuiltinId::CharDevices) => {
                let dir = if args.is_empty() {
                    ".".to_string()
                } else {
                    args[0].to_string()
                };
                Ok(crate::perl_fs::list_char_devices(&dir))
            }
            Some(BuiltinId::GlobPar) => {
                let pats: Vec<String> = args.iter().map(|v| v.to_string()).collect();
                Ok(crate::perl_fs::glob_par_patterns(&pats))
            }
            Some(BuiltinId::GlobParProgress) => {
                let progress = args.last().map(|v| v.is_true()).unwrap_or(false);
                let pats: Vec<String> = args[..args.len().saturating_sub(1)]
                    .iter()
                    .map(|v| v.to_string())
                    .collect();
                Ok(crate::perl_fs::glob_par_patterns_with_progress(
                    &pats, progress,
                ))
            }
            Some(BuiltinId::ParSed) => self.interp.builtin_par_sed(&args, line, false),
            Some(BuiltinId::ParSedProgress) => self.interp.builtin_par_sed(&args, line, true),
            Some(BuiltinId::Opendir) => {
                let handle = args.first().map(|v| v.to_string()).unwrap_or_default();
                let path = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                Ok(self.interp.opendir_handle(&handle, &path))
            }
            Some(BuiltinId::Readdir) => {
                let handle = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(self.interp.readdir_handle(&handle))
            }
            Some(BuiltinId::ReaddirList) => {
                let handle = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(self.interp.readdir_handle_list(&handle))
            }
            Some(BuiltinId::Closedir) => {
                let handle = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(self.interp.closedir_handle(&handle))
            }
            Some(BuiltinId::Rewinddir) => {
                let handle = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(self.interp.rewinddir_handle(&handle))
            }
            Some(BuiltinId::Telldir) => {
                let handle = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(self.interp.telldir_handle(&handle))
            }
            Some(BuiltinId::Seekdir) => {
                let handle = args.first().map(|v| v.to_string()).unwrap_or_default();
                let pos = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
                Ok(self.interp.seekdir_handle(&handle, pos))
            }
            Some(BuiltinId::Slurp) => {
                let path = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                read_file_text_perl_compat(&path)
                    .map(PerlValue::string)
                    .map_err(|e| PerlError::runtime(format!("slurp: {}", e), line))
            }
            Some(BuiltinId::Capture) => {
                let cmd = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                crate::capture::run_capture(self.interp, &cmd, line)
            }
            Some(BuiltinId::Ppool) => {
                let n = args
                    .first()
                    .map(|v| v.to_int().max(0) as usize)
                    .unwrap_or(1);
                crate::ppool::create_pool(n)
            }
            Some(BuiltinId::Wantarray) => Ok(match self.interp.wantarray_kind {
                crate::interpreter::WantarrayCtx::Void => PerlValue::UNDEF,
                crate::interpreter::WantarrayCtx::Scalar => PerlValue::integer(0),
                crate::interpreter::WantarrayCtx::List => PerlValue::integer(1),
            }),
            Some(BuiltinId::FetchUrl) => {
                let url = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                ureq::get(&url)
                    .call()
                    .map_err(|e| PerlError::runtime(format!("fetch_url: {}", e), line))
                    .and_then(|r| {
                        r.into_string()
                            .map(PerlValue::string)
                            .map_err(|e| PerlError::runtime(format!("fetch_url: {}", e), line))
                    })
            }
            Some(BuiltinId::Pchannel) => {
                if args.is_empty() {
                    Ok(crate::pchannel::create_pair())
                } else if args.len() == 1 {
                    let n = args[0].to_int().max(1) as usize;
                    Ok(crate::pchannel::create_bounded_pair(n))
                } else {
                    Err(PerlError::runtime(
                        "pchannel() takes 0 or 1 arguments (capacity)",
                        line,
                    ))
                }
            }
            Some(BuiltinId::Pselect) => crate::pchannel::pselect_recv(&args, line),
            Some(BuiltinId::DequeNew) => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("deque() takes no arguments", line));
                }
                Ok(PerlValue::deque(Arc::new(Mutex::new(VecDeque::new()))))
            }
            Some(BuiltinId::HeapNew) => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "heap() expects one comparator sub",
                        line,
                    ));
                }
                let a0 = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                if let Some(sub) = a0.as_code_ref() {
                    Ok(PerlValue::heap(Arc::new(Mutex::new(PerlHeap {
                        items: Vec::new(),
                        cmp: Arc::clone(&sub),
                    }))))
                } else {
                    Err(PerlError::runtime("heap() requires a code reference", line))
                }
            }
            Some(BuiltinId::BarrierNew) => {
                let n = args
                    .first()
                    .map(|v| v.to_int().max(1) as usize)
                    .unwrap_or(1);
                Ok(PerlValue::barrier(PerlBarrier(Arc::new(Barrier::new(n)))))
            }
            Some(BuiltinId::Pipeline) => {
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
            Some(BuiltinId::ParPipeline) => {
                if crate::par_pipeline::is_named_par_pipeline_args(&args) {
                    return crate::par_pipeline::run_par_pipeline(self.interp, &args, line);
                }
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
                    par_stream: true,
                    streaming: false,
                    streaming_workers: 0,
                    streaming_buffer: 256,
                }))))
            }
            Some(BuiltinId::ParPipelineStream) => {
                if crate::par_pipeline::is_named_par_pipeline_args(&args) {
                    return crate::par_pipeline::run_par_pipeline_streaming(
                        self.interp,
                        &args,
                        line,
                    );
                }
                self.interp.builtin_par_pipeline_stream_new(&args, line)
            }
            Some(BuiltinId::Each) => {
                let _arg = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                Ok(PerlValue::array(vec![]))
            }
            Some(BuiltinId::Readpipe) => {
                let cmd = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                crate::capture::run_readpipe(self.interp, &cmd, line)
            }
            Some(BuiltinId::Eval) => {
                let arg = args.into_iter().next().unwrap_or(PerlValue::UNDEF);
                self.interp.eval_nesting += 1;
                let out = if let Some(sub) = arg.as_code_ref() {
                    match self.interp.exec_block(&sub.body) {
                        Ok(v) => {
                            self.interp.clear_eval_error();
                            Ok(v)
                        }
                        Err(crate::interpreter::FlowOrError::Error(e)) => {
                            self.interp.set_eval_error_from_perl_error(&e);
                            Ok(PerlValue::UNDEF)
                        }
                        Err(crate::interpreter::FlowOrError::Flow(_)) => {
                            self.interp.clear_eval_error();
                            Ok(PerlValue::UNDEF)
                        }
                    }
                } else {
                    let code = arg.to_string();
                    match crate::parse_and_run_string(&code, self.interp) {
                        Ok(v) => {
                            self.interp.clear_eval_error();
                            Ok(v)
                        }
                        Err(e) => {
                            self.interp.set_eval_error_from_perl_error(&e);
                            Ok(PerlValue::UNDEF)
                        }
                    }
                };
                self.interp.eval_nesting -= 1;
                out
            }
            Some(BuiltinId::Do) => {
                let filename = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                match read_file_text_perl_compat(&filename) {
                    Ok(code) => {
                        let code = crate::data_section::strip_perl_end_marker(&code);
                        crate::parse_and_run_string_in_file(code, self.interp, &filename)
                            .or(Ok(PerlValue::UNDEF))
                    }
                    Err(_) => Ok(PerlValue::UNDEF),
                }
            }
            Some(BuiltinId::Require) => {
                let name = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::UNDEF)
                    .to_string();
                self.interp.require_execute(&name, line)
            }
            Some(BuiltinId::Bless) => {
                let ref_val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
                let class = args
                    .get(1)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| self.interp.scope.get_scalar("__PACKAGE__").to_string());
                Ok(PerlValue::blessed(Arc::new(
                    crate::value::BlessedRef::new_blessed(class, ref_val),
                )))
            }
            Some(BuiltinId::Caller) => Ok(PerlValue::array(vec![
                PerlValue::string("main".into()),
                PerlValue::string(self.interp.file.clone()),
                PerlValue::integer(line as i64),
            ])),
            // Parallel ops (shouldn't reach here — handled by block ops)
            Some(BuiltinId::PMap)
            | Some(BuiltinId::PGrep)
            | Some(BuiltinId::PFor)
            | Some(BuiltinId::PSort)
            | Some(BuiltinId::Fan)
            | Some(BuiltinId::MapBlock)
            | Some(BuiltinId::GrepBlock)
            | Some(BuiltinId::SortBlock)
            | Some(BuiltinId::Sort) => Ok(PerlValue::UNDEF),
            _ => Err(PerlError::runtime(
                format!("Unimplemented builtin {:?}", bid),
                line,
            )),
        }
    }
}

/// Integer fast-path comparison helper.
#[inline]
fn int_cmp(
    a: &PerlValue,
    b: &PerlValue,
    int_op: fn(&i64, &i64) -> bool,
    float_op: fn(f64, f64) -> bool,
) -> PerlValue {
    if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
        PerlValue::integer(if int_op(&x, &y) { 1 } else { 0 })
    } else {
        PerlValue::integer(if float_op(a.to_number(), b.to_number()) {
            1
        } else {
            0
        })
    }
}

/// Block JIT hook: string concat with `use overload` / `""` stringify (matches [`Op::Concat`]).
///
/// # Safety
///
/// `vm` must be a valid, non-null pointer to a live [`VM`] for the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn stryke_jit_concat_vm(vm: *mut std::ffi::c_void, a: i64, b: i64) -> i64 {
    let vm: &mut VM<'static> = unsafe { &mut *(vm as *mut VM<'static>) };
    let pa = PerlValue::from_raw_bits(crate::jit::perl_value_bits_from_jit_string_operand(a));
    let pb = PerlValue::from_raw_bits(crate::jit::perl_value_bits_from_jit_string_operand(b));
    match vm.concat_stack_values(pa, pb) {
        Ok(pv) => pv.raw_bits() as i64,
        Err(_) => PerlValue::UNDEF.raw_bits() as i64,
    }
}

/// Cranelift host hook: re-enter the VM for [`Op::Call`] to a compiled sub (stack-args, scalar `i64` args).
/// `sub_ip`, `argc`, `wa` are passed as `i64` for a uniform Cranelift signature.
///
/// # Safety
///
/// `vm` must be a valid, non-null pointer to a live [`VM`] for the duration of this call (JIT only
/// invokes this while the VM is executing).
#[no_mangle]
pub unsafe extern "C" fn stryke_jit_call_sub(
    vm: *mut std::ffi::c_void,
    sub_ip: i64,
    argc: i64,
    wa: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
    a7: i64,
) -> i64 {
    let vm: &mut VM<'static> = unsafe { &mut *(vm as *mut VM<'static>) };
    let want = WantarrayCtx::from_byte(wa as u8);
    if want != WantarrayCtx::Scalar {
        return PerlValue::UNDEF.raw_bits() as i64;
    }
    let argc = argc.clamp(0, 8) as usize;
    let args = [a0, a1, a2, a3, a4, a5, a6, a7];
    let args = &args[..argc];
    match vm.jit_trampoline_run_sub(sub_ip as usize, want, args) {
        Ok(pv) => {
            if let Some(n) = pv.as_integer() {
                n
            } else {
                pv.raw_bits() as i64
            }
        }
        Err(_) => PerlValue::UNDEF.raw_bits() as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{Chunk, Op};
    use crate::value::PerlValue;

    fn run_chunk(chunk: &Chunk) -> PerlResult<PerlValue> {
        let mut interp = Interpreter::new();
        let mut vm = VM::new(chunk, &mut interp);
        vm.execute()
    }

    /// Block-JIT-eligible loop: `for ($i=0; $i<limit; $i++) { $sum += $i }` — sum 0..limit-1.
    fn block_jit_sum_chunk(limit: i64) -> Chunk {
        let mut c = Chunk::new();
        let ni = c.intern_name("i");
        let ns = c.intern_name("sum");
        c.emit(Op::LoadInt(0), 1);
        c.emit(Op::DeclareScalarSlot(0, ni), 1);
        c.emit(Op::LoadInt(0), 1);
        c.emit(Op::DeclareScalarSlot(1, ns), 1);
        c.emit(Op::GetScalarSlot(0), 1);
        c.emit(Op::LoadInt(limit), 1);
        c.emit(Op::NumLt, 1);
        c.emit(Op::JumpIfFalse(15), 1);
        c.emit(Op::GetScalarSlot(1), 1);
        c.emit(Op::GetScalarSlot(0), 1);
        c.emit(Op::Add, 1);
        c.emit(Op::SetScalarSlot(1), 1);
        c.emit(Op::PostIncSlot(0), 1);
        c.emit(Op::Pop, 1);
        c.emit(Op::Jump(4), 1);
        c.emit(Op::GetScalarSlot(1), 1);
        c.emit(Op::Halt, 1);
        c
    }

    #[test]
    fn jit_disabled_same_result_as_jit_block_loop() {
        let limit = 500i64;
        let chunk = block_jit_sum_chunk(limit);
        let expect = limit * (limit - 1) / 2;

        let mut interp_on = Interpreter::new();
        let mut vm_on = VM::new(&chunk, &mut interp_on);
        assert_eq!(vm_on.execute().expect("vm").to_int(), expect);

        let mut interp_off = Interpreter::new();
        let mut vm_off = VM::new(&chunk, &mut interp_off);
        vm_off.set_jit_enabled(false);
        assert_eq!(vm_off.execute().expect("vm").to_int(), expect);
    }

    #[test]
    fn vm_add_two_integers() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::LoadInt(3), 1);
        c.emit(Op::Add, 1);
        c.emit(Op::Halt, 1);
        let v = run_chunk(&c).expect("vm");
        assert_eq!(v.to_int(), 5);
    }

    #[test]
    fn vm_sub_mul_div() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(10), 1);
        c.emit(Op::LoadInt(3), 1);
        c.emit(Op::Sub, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 7);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(6), 1);
        c.emit(Op::LoadInt(7), 1);
        c.emit(Op::Mul, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 42);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(20), 1);
        c.emit(Op::LoadInt(4), 1);
        c.emit(Op::Div, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 5);
    }

    #[test]
    fn vm_mod_and_pow() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(17), 1);
        c.emit(Op::LoadInt(5), 1);
        c.emit(Op::Mod, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 2);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::LoadInt(3), 1);
        c.emit(Op::Pow, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 8);
    }

    #[test]
    fn vm_negate() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(7), 1);
        c.emit(Op::Negate, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), -7);
    }

    #[test]
    fn vm_dup_and_pop() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::Dup, 1);
        c.emit(Op::Add, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 2);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::Pop, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 1);
    }

    #[test]
    fn vm_set_get_scalar() {
        let mut c = Chunk::new();
        let i = c.intern_name("v");
        c.emit(Op::LoadInt(99), 1);
        c.emit(Op::SetScalar(i), 1);
        c.emit(Op::GetScalar(i), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 99);
    }

    #[test]
    fn vm_scalar_plain_roundtrip_and_keep() {
        let mut c = Chunk::new();
        let i = c.intern_name("plainvar");
        c.emit(Op::LoadInt(99), 1);
        c.emit(Op::SetScalarPlain(i), 1);
        c.emit(Op::GetScalarPlain(i), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 99);

        let mut c = Chunk::new();
        let k = c.intern_name("keepme");
        c.emit(Op::LoadInt(5), 1);
        c.emit(Op::SetScalarKeepPlain(k), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 5);
    }

    #[test]
    fn vm_get_scalar_plain_skips_special_global_zero() {
        let mut c = Chunk::new();
        let idx = c.intern_name("0");
        c.emit(Op::GetScalar(idx), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_string(), "stryke");

        let mut c = Chunk::new();
        let idx = c.intern_name("0");
        c.emit(Op::GetScalarPlain(idx), 1);
        c.emit(Op::Halt, 1);
        assert!(run_chunk(&c).expect("vm").is_undef());
    }

    #[test]
    fn vm_slot_pre_post_inc_dec() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(10), 1);
        c.emit(Op::DeclareScalarSlot(0, u16::MAX), 1);
        c.emit(Op::PostIncSlot(0), 1);
        c.emit(Op::Pop, 1);
        c.emit(Op::GetScalarSlot(0), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 11);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(0), 1);
        c.emit(Op::DeclareScalarSlot(0, u16::MAX), 1);
        c.emit(Op::PreIncSlot(0), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 1);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(5), 1);
        c.emit(Op::DeclareScalarSlot(0, u16::MAX), 1);
        c.emit(Op::PreDecSlot(0), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 4);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(3), 1);
        c.emit(Op::DeclareScalarSlot(0, u16::MAX), 1);
        c.emit(Op::PostDecSlot(0), 1);
        c.emit(Op::Pop, 1);
        c.emit(Op::GetScalarSlot(0), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 2);
    }

    #[test]
    fn vm_str_eq_ne_heap_strings() {
        let mut c = Chunk::new();
        let a = c.add_constant(PerlValue::string("same".into()));
        let b = c.add_constant(PerlValue::string("same".into()));
        c.emit(Op::LoadConst(a), 1);
        c.emit(Op::LoadConst(b), 1);
        c.emit(Op::StrEq, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 1);

        let mut c = Chunk::new();
        let a = c.add_constant(PerlValue::string("a".into()));
        let b = c.add_constant(PerlValue::string("b".into()));
        c.emit(Op::LoadConst(a), 1);
        c.emit(Op::LoadConst(b), 1);
        c.emit(Op::StrNe, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 1);
    }

    #[test]
    fn vm_num_eq_ine() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::NumEq, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 1);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::NumNe, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 1);
    }

    #[test]
    fn vm_num_ordering() {
        for (a, b, op, want) in [
            (1i64, 2i64, Op::NumLt, 1),
            (3i64, 2i64, Op::NumGt, 1),
            (2i64, 2i64, Op::NumLe, 1),
            (2i64, 2i64, Op::NumGe, 1),
        ] {
            let mut c = Chunk::new();
            c.emit(Op::LoadInt(a), 1);
            c.emit(Op::LoadInt(b), 1);
            c.emit(op, 1);
            c.emit(Op::Halt, 1);
            assert_eq!(run_chunk(&c).expect("vm").to_int(), want);
        }
    }

    #[test]
    fn vm_concat_and_str_cmp() {
        let mut c = Chunk::new();
        let i1 = c.add_constant(PerlValue::string("a".into()));
        let i2 = c.add_constant(PerlValue::string("b".into()));
        c.emit(Op::LoadConst(i1), 1);
        c.emit(Op::LoadConst(i2), 1);
        c.emit(Op::Concat, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_string(), "ab");

        let mut c = Chunk::new();
        let i1 = c.add_constant(PerlValue::string("a".into()));
        let i2 = c.add_constant(PerlValue::string("b".into()));
        c.emit(Op::LoadConst(i1), 1);
        c.emit(Op::LoadConst(i2), 1);
        c.emit(Op::StrCmp, 1);
        c.emit(Op::Halt, 1);
        let v = run_chunk(&c).expect("vm");
        assert!(v.to_int() < 0);
    }

    #[test]
    fn vm_log_not() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(0), 1);
        c.emit(Op::LogNot, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 1);
    }

    #[test]
    fn vm_bit_and_or_xor_not() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(0b1100), 1);
        c.emit(Op::LoadInt(0b1010), 1);
        c.emit(Op::BitAnd, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 0b1000);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(0b1100), 1);
        c.emit(Op::LoadInt(0b1010), 1);
        c.emit(Op::BitOr, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 0b1110);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(0b1100), 1);
        c.emit(Op::LoadInt(0b1010), 1);
        c.emit(Op::BitXor, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 0b0110);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(0), 1);
        c.emit(Op::BitNot, 1);
        c.emit(Op::Halt, 1);
        assert!((run_chunk(&c).expect("vm").to_int() & 0xFF) != 0);
    }

    #[test]
    fn vm_shl_shr() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::LoadInt(3), 1);
        c.emit(Op::Shl, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 8);

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(16), 1);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::Shr, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 4);
    }

    #[test]
    fn vm_load_undef_float_constant() {
        let mut c = Chunk::new();
        c.emit(Op::LoadUndef, 1);
        c.emit(Op::Halt, 1);
        assert!(run_chunk(&c).expect("vm").is_undef());

        let mut c = Chunk::new();
        c.emit(Op::LoadFloat(2.5), 1);
        c.emit(Op::Halt, 1);
        assert!((run_chunk(&c).expect("vm").to_number() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn vm_jump_skips_ops() {
        let mut c = Chunk::new();
        let j = c.emit(Op::Jump(0), 1);
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::Add, 1);
        c.patch_jump_here(j);
        c.emit(Op::LoadInt(40), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 40);
    }

    #[test]
    fn vm_jump_if_false() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(0), 1);
        let j = c.emit(Op::JumpIfFalse(0), 1);
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::Halt, 1);
        c.patch_jump_here(j);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 2);
    }

    #[test]
    fn vm_call_builtin_defined() {
        let mut c = Chunk::new();
        c.emit(Op::LoadUndef, 1);
        c.emit(Op::CallBuiltin(BuiltinId::Defined as u16, 1), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 0);
    }

    #[test]
    fn vm_call_builtin_length_string() {
        let mut c = Chunk::new();
        let idx = c.add_constant(PerlValue::string("abc".into()));
        c.emit(Op::LoadConst(idx), 1);
        c.emit(Op::CallBuiltin(BuiltinId::Length as u16, 1), 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), 3);
    }

    #[test]
    fn vm_make_array_two() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::MakeArray(2), 1);
        c.emit(Op::Halt, 1);
        let v = run_chunk(&c).expect("vm");
        let a = v.as_array_vec().expect("array");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].to_int(), 1);
        assert_eq!(a[1].to_int(), 2);
    }

    #[test]
    fn vm_spaceship() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::Spaceship, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_int(), -1);
    }

    #[test]
    fn compiled_try_catch_catches_die_via_vm() {
        let program = crate::parse(
            r#"
        try {
            die "boom";
        } catch ($err) {
            42;
        }
    "#,
        )
        .expect("parse");
        let chunk = crate::compiler::Compiler::new()
            .compile_program(&program)
            .expect("compile");
        let tp = chunk
            .ops
            .iter()
            .position(|o| matches!(o, Op::TryPush { .. }))
            .expect("TryPush op");
        match &chunk.ops[tp] {
            Op::TryPush {
                catch_ip, after_ip, ..
            } => {
                assert_ne!(*catch_ip, 0, "catch_ip must be patched");
                assert_ne!(*after_ip, 0, "after_ip must be patched");
            }
            _ => unreachable!(),
        }
        let mut interp = Interpreter::new();
        let mut vm = VM::new(&chunk, &mut interp);
        vm.set_jit_enabled(false);
        let v = vm.execute().expect("vm should catch die");
        assert_eq!(v.to_int(), 42);
    }
}
