use std::collections::VecDeque;
use std::io::{self, Write as IoWrite};
use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::RwLock;
use rayon::prelude::*;

use caseless::default_case_fold_str;

use crate::ast::{Block, Expr, MatchArm, PerlTypeName, Sigil};
use crate::bytecode::{BuiltinId, Chunk, Op, RuntimeSubDecl};
use crate::compiler::scalar_compound_op_from_byte;
use crate::error::{ErrorKind, PerlError, PerlResult};
use crate::interpreter::{
    fold_preduce_init_step, merge_preduce_init_partials, preduce_init_fold_identity, Flow,
    FlowOrError, Interpreter, WantarrayCtx,
};
use crate::pmap_progress::{FanProgress, PmapProgress};
use crate::sort_fast::{sort_magic_cmp, SortBlockFast};
use crate::value::{PerlAsyncTask, PerlBarrier, PerlHeap, PerlSub, PerlValue, PipelineInner};
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
    blocks: Vec<Block>,
    block_bytecode_ranges: Vec<Option<(usize, usize)>>,
    given_entries: Vec<(Expr, Block)>,
    eval_timeout_entries: Vec<(Expr, Block)>,
    algebraic_match_entries: Vec<(Expr, Vec<MatchArm>)>,
    par_lines_entries: Vec<(Expr, Expr, Option<Expr>)>,
    par_walk_entries: Vec<(Expr, Expr, Option<Expr>)>,
    pwatch_entries: Vec<(Expr, Expr)>,
    substr_four_arg_entries: Vec<(Expr, Expr, Option<Expr>, Expr)>,
    keys_expr_entries: Vec<Expr>,
    grep_expr_entries: Vec<Expr>,
    values_expr_entries: Vec<Expr>,
    delete_expr_entries: Vec<Expr>,
    exists_expr_entries: Vec<Expr>,
    push_expr_entries: Vec<(Expr, Vec<Expr>)>,
    pop_expr_entries: Vec<Expr>,
    shift_expr_entries: Vec<Expr>,
    unshift_expr_entries: Vec<(Expr, Vec<Expr>)>,
    splice_expr_entries: Vec<(Expr, Option<Expr>, Option<Expr>, Vec<Expr>)>,
    lvalues: Vec<Expr>,
    runtime_sub_decls: Arc<Vec<RuntimeSubDecl>>,
    jit_sub_invoke_threshold: u32,
    op_len_plus_one: usize,
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
            blocks: vm.blocks.clone(),
            block_bytecode_ranges: vm.block_bytecode_ranges.clone(),
            given_entries: vm.given_entries.clone(),
            eval_timeout_entries: vm.eval_timeout_entries.clone(),
            algebraic_match_entries: vm.algebraic_match_entries.clone(),
            par_lines_entries: vm.par_lines_entries.clone(),
            par_walk_entries: vm.par_walk_entries.clone(),
            pwatch_entries: vm.pwatch_entries.clone(),
            substr_four_arg_entries: vm.substr_four_arg_entries.clone(),
            keys_expr_entries: vm.keys_expr_entries.clone(),
            grep_expr_entries: vm.grep_expr_entries.clone(),
            values_expr_entries: vm.values_expr_entries.clone(),
            delete_expr_entries: vm.delete_expr_entries.clone(),
            exists_expr_entries: vm.exists_expr_entries.clone(),
            push_expr_entries: vm.push_expr_entries.clone(),
            pop_expr_entries: vm.pop_expr_entries.clone(),
            shift_expr_entries: vm.shift_expr_entries.clone(),
            unshift_expr_entries: vm.unshift_expr_entries.clone(),
            splice_expr_entries: vm.splice_expr_entries.clone(),
            lvalues: vm.lvalues.clone(),
            runtime_sub_decls: Arc::clone(&vm.runtime_sub_decls),
            jit_sub_invoke_threshold: vm.jit_sub_invoke_threshold,
            op_len_plus_one: n,
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
            blocks: self.blocks.clone(),
            block_bytecode_ranges: self.block_bytecode_ranges.clone(),
            given_entries: self.given_entries.clone(),
            eval_timeout_entries: self.eval_timeout_entries.clone(),
            algebraic_match_entries: self.algebraic_match_entries.clone(),
            par_lines_entries: self.par_lines_entries.clone(),
            par_walk_entries: self.par_walk_entries.clone(),
            pwatch_entries: self.pwatch_entries.clone(),
            substr_four_arg_entries: self.substr_four_arg_entries.clone(),
            keys_expr_entries: self.keys_expr_entries.clone(),
            grep_expr_entries: self.grep_expr_entries.clone(),
            values_expr_entries: self.values_expr_entries.clone(),
            delete_expr_entries: self.delete_expr_entries.clone(),
            exists_expr_entries: self.exists_expr_entries.clone(),
            push_expr_entries: self.push_expr_entries.clone(),
            pop_expr_entries: self.pop_expr_entries.clone(),
            shift_expr_entries: self.shift_expr_entries.clone(),
            unshift_expr_entries: self.unshift_expr_entries.clone(),
            splice_expr_entries: self.splice_expr_entries.clone(),
            lvalues: self.lvalues.clone(),
            runtime_sub_decls: Arc::clone(&self.runtime_sub_decls),
            ip: 0,
            stack: Vec::with_capacity(256),
            call_stack: Vec::with_capacity(32),
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
    /// [`perlrs_jit_call_sub`] — no bytecode resume; result stored in [`VM::jit_trampoline_out`].
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
    blocks: Vec<Block>,
    /// Optional `ops[start..end]` lowering for [`Self::blocks`] (see [`Chunk::block_bytecode_ranges`]).
    block_bytecode_ranges: Vec<Option<(usize, usize)>>,
    given_entries: Vec<(Expr, Block)>,
    eval_timeout_entries: Vec<(Expr, Block)>,
    algebraic_match_entries: Vec<(Expr, Vec<MatchArm>)>,
    par_lines_entries: Vec<(Expr, Expr, Option<Expr>)>,
    par_walk_entries: Vec<(Expr, Expr, Option<Expr>)>,
    pwatch_entries: Vec<(Expr, Expr)>,
    substr_four_arg_entries: Vec<(Expr, Expr, Option<Expr>, Expr)>,
    keys_expr_entries: Vec<Expr>,
    grep_expr_entries: Vec<Expr>,
    values_expr_entries: Vec<Expr>,
    delete_expr_entries: Vec<Expr>,
    exists_expr_entries: Vec<Expr>,
    push_expr_entries: Vec<(Expr, Vec<Expr>)>,
    pop_expr_entries: Vec<Expr>,
    shift_expr_entries: Vec<Expr>,
    unshift_expr_entries: Vec<(Expr, Vec<Expr>)>,
    splice_expr_entries: Vec<(Expr, Option<Expr>, Option<Expr>, Vec<Expr>)>,
    lvalues: Vec<Expr>,
    runtime_sub_decls: Arc<Vec<RuntimeSubDecl>>,
    ip: usize,
    stack: Vec<PerlValue>,
    call_stack: Vec<CallFrame>,
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
    /// Minimum invocations before attempting subroutine JIT. Override with `PERLRS_JIT_SUB_INVOKES` (default 50).
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
    /// When executing [`Chunk::block_bytecode_ranges`] via [`Self::run_block_region`].
    block_region_mode: bool,
    block_region_end: usize,
    block_region_return: Option<PerlValue>,
}

impl<'a> VM<'a> {
    pub fn new(chunk: &Chunk, interp: &'a mut Interpreter) -> Self {
        Self {
            names: Arc::new(chunk.names.clone()),
            constants: Arc::new(chunk.constants.clone()),
            ops: Arc::new(chunk.ops.clone()),
            lines: Arc::new(chunk.lines.clone()),
            sub_entries: chunk.sub_entries.clone(),
            blocks: chunk.blocks.clone(),
            block_bytecode_ranges: chunk.block_bytecode_ranges.clone(),
            given_entries: chunk.given_entries.clone(),
            eval_timeout_entries: chunk.eval_timeout_entries.clone(),
            algebraic_match_entries: chunk.algebraic_match_entries.clone(),
            par_lines_entries: chunk.par_lines_entries.clone(),
            par_walk_entries: chunk.par_walk_entries.clone(),
            pwatch_entries: chunk.pwatch_entries.clone(),
            substr_four_arg_entries: chunk.substr_four_arg_entries.clone(),
            keys_expr_entries: chunk.keys_expr_entries.clone(),
            grep_expr_entries: chunk.grep_expr_entries.clone(),
            values_expr_entries: chunk.values_expr_entries.clone(),
            delete_expr_entries: chunk.delete_expr_entries.clone(),
            exists_expr_entries: chunk.exists_expr_entries.clone(),
            push_expr_entries: chunk.push_expr_entries.clone(),
            pop_expr_entries: chunk.pop_expr_entries.clone(),
            shift_expr_entries: chunk.shift_expr_entries.clone(),
            unshift_expr_entries: chunk.unshift_expr_entries.clone(),
            splice_expr_entries: chunk.splice_expr_entries.clone(),
            lvalues: chunk.lvalues.clone(),
            runtime_sub_decls: Arc::new(chunk.runtime_sub_decls.clone()),
            ip: 0,
            stack: Vec::with_capacity(256),
            call_stack: Vec::with_capacity(32),
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
            jit_sub_invoke_threshold: std::env::var("PERLRS_JIT_SUB_INVOKES")
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
                    None if pv.is_undef() => {
                        if crate::jit::slot_undef_prefill_ok_seq(seg, i) {
                            0
                        } else {
                            ok = false;
                            break;
                        }
                    }
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
                        None if pv.is_undef() => {
                            if crate::jit::block_slot_undef_prefill_ok(full_body, i) {
                                0
                            } else {
                                ok = false;
                                break;
                            }
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
            } else if let Some(def) = self.interp.struct_defs.get(&class) {
                let v = crate::native_data::struct_new(def, &all_args, self.line())?;
                self.push(v);
            } else {
                let mut map = IndexMap::new();
                let mut i = 1;
                while i + 1 < all_args.len() {
                    map.insert(all_args[i].to_string(), all_args[i + 1].clone());
                    i += 2;
                }
                self.push(PerlValue::blessed(Arc::new(crate::value::BlessedRef {
                    class,
                    data: RwLock::new(PerlValue::hash(map)),
                })));
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
            let _ = local_interp
                .scope
                .set_scalar("_", PerlValue::integer(i as i64));
            crate::parallel_trace::fan_worker_set_index(Some(i as i64));
            local_interp.scope_push_hook();
            match local_interp.exec_block_no_scope(&block) {
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
                let _ = local_interp
                    .scope
                    .set_scalar("_", PerlValue::integer(i as i64));
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
                    let pe = match e {
                        FlowOrError::Error(pe) => pe,
                        FlowOrError::Flow(_) => PerlError::runtime(
                            "return/last/next/redo not supported inside fan_cap block",
                            line,
                        ),
                    };
                    return Err(pe);
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
                        None if pv.is_undef() => {
                            if crate::jit::slot_undef_prefill_ok(ops, i) {
                                0
                            } else {
                                ok = false;
                                break;
                            }
                        }
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
                                None if pv.is_undef() => {
                                    if crate::jit::block_slot_undef_prefill_ok(ops, i) {
                                        0
                                    } else {
                                        ok = false;
                                        break;
                                    }
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
                self.sub_entry_invoke_count[sub_ip] =
                    self.sub_entry_invoke_count[sub_ip].saturating_add(1);
                if self.sub_entry_invoke_count[sub_ip] > self.jit_sub_invoke_threshold {
                    if self.try_jit_subroutine_linear()? {
                        continue;
                    }
                    if self.try_jit_subroutine_block()? {
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
            let op_prof_t0 = self.interp.profiler.is_some().then(std::time::Instant::now);
            // Closure: `?` / `return Err` inside `match op` must not return from
            // `run_main_dispatch_loop` — they must become `__op_res` so `try_recover_from_exception`
            // can run before propagating.
            let __op_res: PerlResult<()> = (|| -> PerlResult<()> {
                match op {
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
                        self.pop();
                        Ok(())
                    }
                    Op::Dup => {
                        let v = self.peek().clone();
                        self.push(v);
                        Ok(())
                    }
                    Op::Dup2 => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(a.clone());
                        self.push(b.clone());
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
                        self.interp
                            .set_special_var(n, &val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::SetScalarPlain(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        self.interp
                            .scope
                            .set_scalar(n, val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::SetScalarKeep(idx) => {
                        let val = self.peek().clone();
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
                        self.interp
                            .set_special_var(n, &val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::SetScalarKeepPlain(idx) => {
                        let val = self.peek().clone();
                        let n = names[*idx as usize].as_str();
                        self.require_scalar_mutable(n)?;
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
                        self.interp
                            .scope
                            .push_to_array(n, val)
                            .map_err(|e| e.at_line(line))?;
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
                    Op::ArrayLen(idx) => {
                        let len = self.interp.scope.array_len(&self.names[*idx as usize]);
                        self.push(PerlValue::integer(len as i64));
                        Ok(())
                    }

                    // ── Hashes ──
                    Op::GetHash(idx) => {
                        let n = names[*idx as usize].as_str();
                        if n == "ENV" {
                            self.interp.materialize_env_if_needed();
                        }
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
                        self.interp
                            .scope
                            .local_set_scalar(n, val)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::LocalDeclareArray(idx) => {
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.interp
                            .scope
                            .local_set_array(n, val.to_list())
                            .map_err(|e| e.at_line(self.line()))?;
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
                        if n == "ENV" {
                            self.interp.materialize_env_if_needed();
                        }
                        self.interp
                            .scope
                            .local_set_hash(n, map)
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }
                    Op::GetHashElem(idx) => {
                        let key = self.pop().to_string();
                        let n = names[*idx as usize].as_str();
                        if n == "ENV" {
                            self.interp.materialize_env_if_needed();
                        }
                        let val = self.interp.scope.get_hash_element(n, &key);
                        self.push(val);
                        Ok(())
                    }
                    Op::SetHashElem(idx) => {
                        let key = self.pop().to_string();
                        let val = self.pop();
                        let n = names[*idx as usize].as_str();
                        self.require_hash_mutable(n)?;
                        if n == "ENV" {
                            self.interp.materialize_env_if_needed();
                        }
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
                        if n == "ENV" {
                            self.interp.materialize_env_if_needed();
                        }
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
                        if n == "ENV" {
                            self.interp.materialize_env_if_needed();
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
                        if n == "ENV" {
                            self.interp.materialize_env_if_needed();
                        }
                        let exists = self.interp.scope.exists_hash_element(n, &key);
                        self.push(PerlValue::integer(if exists { 1 } else { 0 }));
                        Ok(())
                    }
                    Op::HashKeys(idx) => {
                        let n = names[*idx as usize].as_str();
                        if n == "ENV" {
                            self.interp.materialize_env_if_needed();
                        }
                        let h = self.interp.scope.get_hash(n);
                        let keys: Vec<PerlValue> =
                            h.keys().map(|k| PerlValue::string(k.clone())).collect();
                        self.push(PerlValue::array(keys));
                        Ok(())
                    }
                    Op::HashValues(idx) => {
                        let n = names[*idx as usize].as_str();
                        if n == "ENV" {
                            self.interp.materialize_env_if_needed();
                        }
                        let h = self.interp.scope.get_hash(n);
                        let vals: Vec<PerlValue> = h.values().cloned().collect();
                        self.push(PerlValue::array(vals));
                        Ok(())
                    }

                    // ── Arithmetic (integer fast paths) ──
                    Op::Add => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(
                            if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                PerlValue::integer(x.wrapping_add(y))
                            } else {
                                PerlValue::float(a.to_number() + b.to_number())
                            },
                        );
                        Ok(())
                    }
                    Op::Sub => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(
                            if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                PerlValue::integer(x.wrapping_sub(y))
                            } else {
                                PerlValue::float(a.to_number() - b.to_number())
                            },
                        );
                        Ok(())
                    }
                    Op::Mul => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(
                            if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                PerlValue::integer(x.wrapping_mul(y))
                            } else {
                                PerlValue::float(a.to_number() * b.to_number())
                            },
                        );
                        Ok(())
                    }
                    Op::Div => {
                        let b = self.pop();
                        let a = self.pop();
                        if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                            if y == 0 {
                                return Err(PerlError::runtime(
                                    "Illegal division by zero",
                                    self.line(),
                                ));
                            }
                            self.push(if x % y == 0 {
                                PerlValue::integer(x / y)
                            } else {
                                PerlValue::float(x as f64 / y as f64)
                            });
                        } else {
                            let d = b.to_number();
                            if d == 0.0 {
                                return Err(PerlError::runtime(
                                    "Illegal division by zero",
                                    self.line(),
                                ));
                            }
                            self.push(PerlValue::float(a.to_number() / d));
                        }
                        Ok(())
                    }
                    Op::Mod => {
                        let b = self.pop().to_int();
                        let a = self.pop().to_int();
                        if b == 0 {
                            return Err(PerlError::runtime("Illegal modulus zero", self.line()));
                        }
                        self.push(PerlValue::integer(a % b));
                        Ok(())
                    }
                    Op::Pow => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(
                            if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                                if (0..=63).contains(&y) {
                                    PerlValue::integer(x.wrapping_pow(y as u32))
                                } else {
                                    PerlValue::float(a.to_number().powf(b.to_number()))
                                }
                            } else {
                                PerlValue::float(a.to_number().powf(b.to_number()))
                            },
                        );
                        Ok(())
                    }
                    Op::Negate => {
                        let a = self.pop();
                        self.push(if let Some(n) = a.as_integer() {
                            PerlValue::integer(-n)
                        } else {
                            PerlValue::float(-a.to_number())
                        });
                        Ok(())
                    }

                    // ── String ──
                    Op::Concat => {
                        let b = self.pop();
                        let a = self.pop();
                        let mut s = a.into_string();
                        b.append_to(&mut s);
                        self.push(PerlValue::string(s));
                        Ok(())
                    }
                    Op::StringRepeat => {
                        let n = self.pop().to_int().max(0) as usize;
                        let val = self.pop();
                        self.push(PerlValue::string(val.to_string().repeat(n)));
                        Ok(())
                    }

                    // ── Numeric comparison ──
                    Op::NumEq => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(int_cmp(&a, &b, |x, y| x == y, |x, y| x == y));
                        Ok(())
                    }
                    Op::NumNe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(int_cmp(&a, &b, |x, y| x != y, |x, y| x != y));
                        Ok(())
                    }
                    Op::NumLt => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(int_cmp(&a, &b, |x, y| x < y, |x, y| x < y));
                        Ok(())
                    }
                    Op::NumGt => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(int_cmp(&a, &b, |x, y| x > y, |x, y| x > y));
                        Ok(())
                    }
                    Op::NumLe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(int_cmp(&a, &b, |x, y| x <= y, |x, y| x <= y));
                        Ok(())
                    }
                    Op::NumGe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(int_cmp(&a, &b, |x, y| x >= y, |x, y| x >= y));
                        Ok(())
                    }
                    Op::Spaceship => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(
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
                        );
                        Ok(())
                    }

                    // ── String comparison ──
                    Op::StrEq => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(PerlValue::integer(if a.str_eq(&b) { 1 } else { 0 }));
                        Ok(())
                    }
                    Op::StrNe => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(PerlValue::integer(if !a.str_eq(&b) { 1 } else { 0 }));
                        Ok(())
                    }
                    Op::StrLt => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(PerlValue::integer(
                            if a.str_cmp(&b) == std::cmp::Ordering::Less {
                                1
                            } else {
                                0
                            },
                        ));
                        Ok(())
                    }
                    Op::StrGt => {
                        let b = self.pop();
                        let a = self.pop();
                        self.push(PerlValue::integer(
                            if a.str_cmp(&b) == std::cmp::Ordering::Greater {
                                1
                            } else {
                                0
                            },
                        ));
                        Ok(())
                    }
                    Op::StrLe => {
                        let b = self.pop();
                        let a = self.pop();
                        let o = a.str_cmp(&b);
                        self.push(PerlValue::integer(
                            if matches!(o, std::cmp::Ordering::Less | std::cmp::Ordering::Equal) {
                                1
                            } else {
                                0
                            },
                        ));
                        Ok(())
                    }
                    Op::StrGe => {
                        let b = self.pop();
                        let a = self.pop();
                        let o = a.str_cmp(&b);
                        self.push(PerlValue::integer(
                            if matches!(o, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                            {
                                1
                            } else {
                                0
                            },
                        ));
                        Ok(())
                    }
                    Op::StrCmp => {
                        let b = self.pop();
                        let a = self.pop();
                        let cmp = a.str_cmp(&b);
                        self.push(PerlValue::integer(match cmp {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Greater => 1,
                            std::cmp::Ordering::Equal => 0,
                        }));
                        Ok(())
                    }

                    // ── Logical / Bitwise ──
                    Op::LogNot => {
                        let a = self.pop();
                        self.push(PerlValue::integer(if a.is_true() { 0 } else { 1 }));
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
                        let name = names[*name_idx as usize].as_str();
                        let argc = *argc as usize;
                        let want = WantarrayCtx::from_byte(*wa);

                        // Check if sub is compiled (has bytecode entry)
                        if let Some((entry_ip, stack_args)) = self.find_sub_entry(*name_idx) {
                            let saved_wa = self.interp.wantarray_kind;
                            let sub_prof_t0 =
                                self.interp.profiler.is_some().then(std::time::Instant::now);
                            if let Some(p) = &mut self.interp.profiler {
                                p.enter_sub(name);
                            }

                            if stack_args {
                                // Fast path: leave args on stack, sub reads via GetArg(idx).
                                // stack_base points at first arg.
                                let eff_argc = if argc == 0 {
                                    // Zero-arg call passes `$_` (same as tree `call_named_sub`).
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
                                if let Some(sub) = self.interp.resolve_sub_by_name(name) {
                                    if let Some(ref env) = sub.closure_env {
                                        self.interp.scope.restore_capture(env);
                                    }
                                }
                                self.ip = entry_ip;
                            } else {
                                // Slow path: collect args into @_
                                let mut args = Vec::with_capacity(argc);
                                for _ in 0..argc {
                                    let v = self.pop();
                                    if let Some(items) = v.as_array_vec() {
                                        args.extend(items);
                                    } else {
                                        args.push(v);
                                    }
                                }
                                args.reverse();
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
                                if let Some(sub) = self.interp.resolve_sub_by_name(name) {
                                    if let Some(ref env) = sub.closure_env {
                                        self.interp.scope.restore_capture(env);
                                    }
                                }
                                self.ip = entry_ip;
                            }
                        } else {
                            // Non-compiled path: collect args from stack
                            let mut args = Vec::with_capacity(argc);
                            for _ in 0..argc {
                                let v = self.pop();
                                if let Some(items) = v.as_array_vec() {
                                    args.extend(items);
                                } else {
                                    args.push(v);
                                }
                            }
                            args.reverse();

                            if let Some(r) =
                                crate::builtins::try_builtin(self.interp, name, &args, self.line())
                            {
                                self.push(r?);
                            } else if let Some(sub) = self.interp.resolve_sub_by_name(name) {
                                // Fall back to tree-walker for non-compiled subs
                                let t0 =
                                    self.interp.profiler.is_some().then(std::time::Instant::now);
                                if let Some(p) = &mut self.interp.profiler {
                                    p.enter_sub(name);
                                }
                                let args = self.interp.with_topic_default_args(args);
                                let saved_wa = self.interp.wantarray_kind;
                                self.interp.wantarray_kind = want;
                                self.interp.scope_push_hook();
                                let argv = args.clone();
                                self.interp.scope.declare_array("_", args);
                                if let Some(ref env) = sub.closure_env {
                                    self.interp.scope.restore_capture(env);
                                }
                                let result = if let Some(r) = crate::list_util::native_dispatch(
                                    self.interp,
                                    &sub,
                                    &argv,
                                    want,
                                ) {
                                    r
                                } else {
                                    self.interp.exec_block_no_scope(&sub.body)
                                };
                                self.interp.wantarray_kind = saved_wa;
                                self.interp.scope_pop_hook();
                                match result {
                                    Ok(v) => self.push(v),
                                    Err(crate::interpreter::FlowOrError::Flow(
                                        crate::interpreter::Flow::Return(v),
                                    )) => self.push(v),
                                    Err(crate::interpreter::FlowOrError::Error(e)) => {
                                        if let (Some(p), Some(t0)) = (&mut self.interp.profiler, t0)
                                        {
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
                                self.interp.with_topic_default_args(args),
                                self.line(),
                                want,
                                None,
                            ) {
                                let t0 =
                                    self.interp.profiler.is_some().then(std::time::Instant::now);
                                if let Some(p) = &mut self.interp.profiler {
                                    p.enter_sub(name);
                                }
                                match result {
                                    Ok(v) => self.push(v),
                                    Err(crate::interpreter::FlowOrError::Flow(
                                        crate::interpreter::Flow::Return(v),
                                    )) => self.push(v),
                                    Err(crate::interpreter::FlowOrError::Error(e)) => {
                                        if let (Some(p), Some(t0)) = (&mut self.interp.profiler, t0)
                                        {
                                            p.exit_sub(t0.elapsed());
                                        }
                                        return Err(e);
                                    }
                                    Err(_) => self.push(PerlValue::UNDEF),
                                }
                                if let (Some(p), Some(t0)) = (&mut self.interp.profiler, t0) {
                                    p.exit_sub(t0.elapsed());
                                }
                            } else {
                                return Err(PerlError::runtime(
                                    format!("Undefined subroutine &{}", name),
                                    self.line(),
                                ));
                            }
                        } // close outer else (non-compiled path)
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
                    Op::TriangularForAccum {
                        limit,
                        sum_name_idx,
                        i_name_idx,
                    } => {
                        let sum_name = names[*sum_name_idx as usize].as_str();
                        let i_name = names[*i_name_idx as usize].as_str();
                        self.require_scalar_mutable(sum_name)?;
                        self.require_scalar_mutable(i_name)?;
                        let lim = *limit;
                        if lim < 0 {
                            return Err(PerlError::runtime(
                                "TriangularForAccum: negative limit",
                                self.line(),
                            ));
                        }
                        let sum = {
                            let a = lim as i128;
                            let b = lim as i128 - 1;
                            (a * b / 2) as i64
                        };
                        let final_i = if lim == 0 { 0 } else { lim };
                        self.interp
                            .scope
                            .set_scalar(sum_name, PerlValue::integer(sum))
                            .map_err(|e| e.at_line(self.line()))?;
                        self.interp
                            .scope
                            .set_scalar(i_name, PerlValue::integer(final_i))
                            .map_err(|e| e.at_line(self.line()))?;
                        Ok(())
                    }

                    // ── I/O ──
                    Op::Print(argc) => {
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
                                output.push_str(&arg.to_string());
                            }
                        }
                        output.push_str(&self.interp.ors);
                        print!("{}", output);
                        if self.interp.output_autoflush {
                            let _ = io::stdout().flush();
                        }
                        self.push(PerlValue::integer(1));
                        Ok(())
                    }
                    Op::Say(argc) => {
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
                                output.push_str(&arg.to_string());
                            }
                        }
                        output.push('\n');
                        print!("{}", output);
                        if self.interp.output_autoflush {
                            let _ = io::stdout().flush();
                        }
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

                    // ── List / Range ──
                    Op::MakeArray(n) => {
                        let n = *n as usize;
                        let mut arr = Vec::with_capacity(n);
                        for _ in 0..n {
                            let v = self.pop();
                            if let Some(items) = v.as_array_vec() {
                                arr.extend(items);
                            } else {
                                arr.push(v);
                            }
                        }
                        arr.reverse();
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
                            Interpreter::hash_slice_deref_values(&container, &key_vals, line),
                            line,
                        )?;
                        self.push(out);
                        Ok(())
                    }
                    Op::ArrowArraySlice(n) => {
                        let n = *n as usize;
                        let mut idxs = Vec::with_capacity(n);
                        for _ in 0..n {
                            idxs.push(self.pop().to_int());
                        }
                        idxs.reverse();
                        let r = self.pop();
                        let line = self.line();
                        let mut out = Vec::with_capacity(n);
                        for idx in idxs {
                            let v = vm_interp_result(
                                self.interp.read_arrow_array_element(r.clone(), idx, line),
                                line,
                            )?;
                            out.push(v);
                        }
                        self.push(PerlValue::array(out));
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
                    Op::SetArrowArraySlice(n) => {
                        let n = *n as usize;
                        let mut idxs = Vec::with_capacity(n);
                        for _ in 0..n {
                            idxs.push(self.pop().to_int());
                        }
                        idxs.reverse();
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
                        let mut idxs = Vec::with_capacity(n);
                        for _ in 0..n {
                            idxs.push(self.pop().to_int());
                        }
                        idxs.reverse();
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
                        let mut idxs = Vec::with_capacity(n);
                        for _ in 0..n {
                            idxs.push(self.pop().to_int());
                        }
                        idxs.reverse();
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
                        let to = self.pop().to_int();
                        let from = self.pop().to_int();
                        let arr: Vec<PerlValue> = (from..=to).map(PerlValue::integer).collect();
                        self.push(PerlValue::array(arr));
                        Ok(())
                    }

                    // ── Regex ──
                    Op::RegexMatch(pat_idx, flags_idx, scalar_g, pos_key_idx) => {
                        let string = self.pop().into_string();
                        let pattern = constants[*pat_idx as usize].as_str_or_empty();
                        let flags = constants[*flags_idx as usize].as_str_or_empty();
                        let pos_key_owned = if *pos_key_idx == u16::MAX {
                            None
                        } else {
                            Some(constants[*pos_key_idx as usize].as_str_or_empty())
                        };
                        let pos_key: &str = pos_key_owned.as_deref().unwrap_or("_");
                        let line = self.line();
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
                        let string = self.pop().into_string();
                        let pattern = constants[*pat_idx as usize].as_str_or_empty();
                        let replacement = constants[*repl_idx as usize].as_str_or_empty();
                        let flags = constants[*flags_idx as usize].as_str_or_empty();
                        let target = &self.lvalues[*lvalue_idx as usize];
                        let line = self.line();
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
                        let string = self.pop().into_string();
                        let from = constants[*from_idx as usize].as_str_or_empty();
                        let to = constants[*to_idx as usize].as_str_or_empty();
                        let flags = constants[*flags_idx as usize].as_str_or_empty();
                        let target = &self.lvalues[*lvalue_idx as usize];
                        let line = self.line();
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
                        self.interp.scope.set_scalar_slot(*slot, val);
                        Ok(())
                    }
                    Op::SetScalarSlotKeep(slot) => {
                        let val = self.peek().clone();
                        self.interp.scope.set_scalar_slot(*slot, val);
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
                        let e = &self.keys_expr_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_keys_expr(e, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::ValuesExpr(idx) => {
                        let e = &self.values_expr_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_values_expr(e, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
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
                    Op::MakeCodeRef(block_idx) => {
                        let block = self.blocks[*block_idx as usize].clone();
                        let captured = self.interp.scope.capture();
                        self.push(PerlValue::code_ref(Arc::new(crate::value::PerlSub {
                            name: "__ANON__".to_string(),
                            params: vec![],
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
                                format!(
                                    "Undefined subroutine {}",
                                    self.interp.qualify_sub_key(name)
                                ),
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
                                format!(
                                    "Undefined subroutine {}",
                                    self.interp.qualify_sub_key(&name)
                                ),
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
                        if let Some(a) = r.as_array_ref() {
                            let arr = a.read();
                            let i = if idx < 0 {
                                (arr.len() as i64 + idx) as usize
                            } else {
                                idx as usize
                            };
                            self.push(arr.get(i).cloned().unwrap_or(PerlValue::UNDEF));
                        } else {
                            self.push(PerlValue::UNDEF);
                        }
                        Ok(())
                    }
                    Op::ArrowHash => {
                        let key = self.pop().to_string();
                        let r = self.pop();
                        if let Some(h) = r.as_hash_ref() {
                            self.push(h.read().get(&key).cloned().unwrap_or(PerlValue::UNDEF));
                        } else if let Some(b) = r.as_blessed_ref() {
                            let data = b.data.read();
                            if let Some(v) = data.hash_get(&key) {
                                self.push(v);
                            } else {
                                self.push(PerlValue::UNDEF);
                            }
                        } else {
                            self.push(PerlValue::UNDEF);
                        }
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
                            let saved_wa = self.interp.wantarray_kind;
                            self.interp.wantarray_kind = want;
                            self.interp.scope_push_hook();
                            self.interp.scope.declare_array("_", args);
                            if let Some(ref env) = sub.closure_env {
                                self.interp.scope.restore_capture(env);
                            }
                            let result = self.interp.exec_block_no_scope(&sub.body);
                            self.interp.wantarray_kind = saved_wa;
                            self.interp.scope_pop_hook();
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
                        let result = match *test as char {
                            'e' => std::path::Path::new(&path).exists(),
                            'f' => std::path::Path::new(&path).is_file(),
                            'd' => std::path::Path::new(&path).is_dir(),
                            'l' => std::path::Path::new(&path).is_symlink(),
                            'r' | 'w' => std::fs::metadata(&path).is_ok(),
                            's' => std::fs::metadata(&path)
                                .map(|m| m.len() > 0)
                                .unwrap_or(false),
                            'z' => std::fs::metadata(&path)
                                .map(|m| m.len() == 0)
                                .unwrap_or(true),
                            't' => crate::perl_fs::filetest_is_tty(&path),
                            _ => false,
                        };
                        self.push(PerlValue::integer(if result { 1 } else { 0 }));
                        Ok(())
                    }

                    // ── Map/Grep/Sort with blocks (opcodes when lowered; else tree-walker) ──
                    Op::MapIntMul(k) => {
                        let list = self.pop().to_list();
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
                        let idx = *block_idx as usize;
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let mut result = Vec::new();
                            for item in list {
                                let _ = self.interp.scope.set_scalar("_", item);
                                let val = self.run_block_region(start, end, op_count)?;
                                if let Some(a) = val.as_array_vec() {
                                    result.extend(a);
                                } else {
                                    result.push(val);
                                }
                            }
                            self.push(PerlValue::array(result));
                            Ok(())
                        } else {
                            let block = self.blocks[idx].clone();
                            let mut result = Vec::new();
                            for item in list {
                                let _ = self.interp.scope.set_scalar("_", item);
                                match self.interp.exec_block(&block) {
                                    Ok(val) => {
                                        if let Some(a) = val.as_array_vec() {
                                            result.extend(a);
                                        } else {
                                            result.push(val);
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
                    Op::GrepWithBlock(block_idx) => {
                        let list = self.pop().to_list();
                        let idx = *block_idx as usize;
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let mut result = Vec::new();
                            for item in list {
                                let _ = self.interp.scope.set_scalar("_", item.clone());
                                let val = self.run_block_region(start, end, op_count)?;
                                if val.is_true() {
                                    result.push(item);
                                }
                            }
                            self.push(PerlValue::array(result));
                            Ok(())
                        } else {
                            let block = self.blocks[idx].clone();
                            let mut result = Vec::new();
                            for item in list {
                                let _ = self.interp.scope.set_scalar("_", item.clone());
                                match self.interp.exec_block(&block) {
                                    Ok(val) => {
                                        if val.is_true() {
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
                    Op::GrepWithExpr(expr_idx) => {
                        let list = self.pop().to_list();
                        let e = &self.grep_expr_entries[*expr_idx as usize];
                        let mut result = Vec::new();
                        for item in list {
                            let _ = self.interp.scope.set_scalar("_", item.clone());
                            let val = vm_interp_result(self.interp.eval_expr(e), self.line())?;
                            if val.is_true() {
                                result.push(item);
                            }
                        }
                        self.push(PerlValue::array(result));
                        Ok(())
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
                    Op::ReverseOp => {
                        let val = self.pop();
                        if let Some(mut a) = val.as_array_vec() {
                            a.reverse();
                            self.push(PerlValue::array(a));
                        } else if let Some(s) = val.as_str() {
                            self.push(PerlValue::string(s.chars().rev().collect()));
                        } else {
                            self.push(PerlValue::string(val.to_string().chars().rev().collect()));
                        }
                        Ok(())
                    }

                    // ── Eval block ──
                    Op::EvalBlock(block_idx) => {
                        let block = self.blocks[*block_idx as usize].clone();
                        self.interp.eval_nesting += 1;
                        // Use exec_block (with scope frame) so local/my declarations
                        // inside the block are properly scoped.
                        match self.interp.exec_block(&block) {
                            Ok(v) => {
                                self.interp.clear_eval_error();
                                self.push(v);
                            }
                            Err(crate::interpreter::FlowOrError::Error(e)) => {
                                self.interp.set_eval_error(e.to_string());
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
                                self.interp.set_eval_error(e.to_string());
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
                                self.interp.set_eval_error(e.to_string());
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
                        let (topic, body) = &self.given_entries[*idx as usize];
                        let v = vm_interp_result(self.interp.exec_given(topic, body), self.line())?;
                        self.push(v);
                        Ok(())
                    }
                    Op::EvalTimeout(idx) => {
                        let (timeout_expr, body) = &self.eval_timeout_entries[*idx as usize];
                        let secs =
                            vm_interp_result(self.interp.eval_expr(timeout_expr), self.line())?
                                .to_number();
                        let v = vm_interp_result(
                            self.interp.eval_timeout_block(body, secs, self.line()),
                            self.line(),
                        )?;
                        self.push(v);
                        Ok(())
                    }
                    Op::AlgebraicMatch(idx) => {
                        let (subject, arms) = &self.algebraic_match_entries[*idx as usize];
                        let v = vm_interp_result(
                            self.interp.eval_algebraic_match(subject, arms, self.line()),
                            self.line(),
                        )?;
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
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            let results: Vec<PerlValue> = list
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
                                    let mut local_interp = Interpreter::new();
                                    local_interp.subs = subs.clone();
                                    local_interp.scope.restore_capture(&scope_capture);
                                    local_interp
                                        .scope
                                        .restore_atomics(&atomic_arrays, &atomic_hashes);
                                    local_interp.enable_parallel_guard();
                                    let _ = local_interp.scope.set_scalar("_", item);
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
                                        let _ = local_interp.scope.set_scalar("_", item);
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
                                        let _ = local_interp.scope.set_scalar("_", item);
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
                                let _ = local_interp.scope.set_scalar("a", acc);
                                let _ = local_interp.scope.set_scalar("b", b);
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
                                let _ = local_interp.scope.set_scalar("a", acc);
                                let _ = local_interp.scope.set_scalar("b", b);
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
                                    let _ = local_interp.scope.set_scalar("a", a);
                                    let _ = local_interp.scope.set_scalar("b", b);
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
                                    let _ = local_interp.scope.set_scalar("a", a);
                                    let _ = local_interp.scope.set_scalar("b", b);
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
                            let _ = local_interp
                                .scope
                                .set_scalar("_", list.into_iter().next().unwrap());
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
                                    let _ = local_interp.scope.set_scalar("_", item);
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
                                    let _ = local_interp.scope.set_scalar("a", a);
                                    let _ = local_interp.scope.set_scalar("b", b);
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
                                    let _ = local_interp.scope.set_scalar("_", item.clone());
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
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            let results: Vec<PerlValue> = list
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
                                    let mut local_interp = Interpreter::new();
                                    local_interp.subs = subs.clone();
                                    local_interp.scope.restore_capture(&scope_capture);
                                    local_interp
                                        .scope
                                        .restore_atomics(&atomic_arrays, &atomic_hashes);
                                    local_interp.enable_parallel_guard();
                                    let _ = local_interp.scope.set_scalar("_", item.clone());
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
                        if let Some(&(start, end)) =
                            self.block_bytecode_ranges.get(idx).and_then(|r| r.as_ref())
                        {
                            let shared = Arc::new(ParallelBlockVmShared::from_vm(self));
                            list.into_par_iter().for_each(|item| {
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
                                let mut local_interp = Interpreter::new();
                                local_interp.subs = subs.clone();
                                local_interp.scope.restore_capture(&scope_capture);
                                local_interp
                                    .scope
                                    .restore_atomics(&atomic_arrays, &atomic_hashes);
                                local_interp.enable_parallel_guard();
                                let _ = local_interp.scope.set_scalar("_", item);
                                local_interp.scope_push_hook();
                                match local_interp.exec_block_no_scope(&block) {
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
                    Op::ScalarCompoundAssign { name_idx, op: op_b } => {
                        let rhs = self.pop();
                        let n = names[*name_idx as usize].as_str();
                        let op = scalar_compound_op_from_byte(*op_b).ok_or_else(|| {
                            PerlError::runtime("ScalarCompoundAssign: invalid op byte", self.line())
                        })?;
                        let en = self.interp.english_scalar_name(n);
                        let val = self
                            .interp
                            .scalar_compound_assign_scalar_target(en, op, rhs);
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
        }

        Ok(last)
    }

    /// Called from Cranelift (`perlrs_jit_call_sub`) to run a compiled sub by bytecode IP with `i64` args.
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
            if let Some(sub) = self.interp.resolve_sub_by_name(nm) {
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

    fn find_sub_entry(&self, name_idx: u16) -> Option<(usize, bool)> {
        for &(n, ip, stack_args) in &self.sub_entries {
            if n == name_idx {
                return Some((ip, stack_args));
            }
        }
        None
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
                Ok(PerlValue::integer(s.to_string().len() as i64))
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
                let joined = list
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(&sep);
                Ok(PerlValue::string(joined))
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
                    msg.push_str(&format!(" at {} line {}.", self.interp.file, line));
                    msg.push('\n');
                }
                Err(PerlError::die(msg, line))
            }
            Some(BuiltinId::Warn) => {
                let mut msg = String::new();
                for a in &args {
                    msg.push_str(&a.to_string());
                }
                if !msg.ends_with('\n') {
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
                let off = args.get(1).map(|v| v.to_int()).unwrap_or(0);
                let start = if off < 0 {
                    (s.len() as i64 + off).max(0) as usize
                } else {
                    off as usize
                };
                let len = args
                    .get(2)
                    .map(|v| v.to_int() as usize)
                    .unwrap_or(s.len() - start);
                let end = (start + len).min(s.len());
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
            Some(BuiltinId::Eof) => {
                if args.is_empty() {
                    Ok(PerlValue::integer(0))
                } else {
                    let name = args[0].to_string();
                    let at_eof = !self.interp.has_input_handle(&name);
                    Ok(PerlValue::integer(if at_eof { 1 } else { 0 }))
                }
            }
            Some(BuiltinId::ReadLine) => {
                let h = if args.is_empty() {
                    None
                } else {
                    Some(args[0].to_string())
                };
                self.interp.readline_builtin_execute(h.as_deref())
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
                std::fs::read_to_string(&path)
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
                            self.interp.set_eval_error(e.to_string());
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
                            self.interp.set_eval_error(e.to_string());
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
                match std::fs::read_to_string(&filename) {
                    Ok(code) => crate::parse_and_run_string_in_file(&code, self.interp, &filename)
                        .or(Ok(PerlValue::UNDEF)),
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
                Ok(PerlValue::blessed(Arc::new(crate::value::BlessedRef {
                    class,
                    data: RwLock::new(ref_val),
                })))
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

/// Cranelift host hook: re-enter the VM for [`Op::Call`] to a compiled sub (stack-args, scalar `i64` args).
/// `sub_ip`, `argc`, `wa` are passed as `i64` for a uniform Cranelift signature.
///
/// # Safety
///
/// `vm` must be a valid, non-null pointer to a live [`VM`] for the duration of this call (JIT only
/// invokes this while the VM is executing).
#[no_mangle]
pub unsafe extern "C" fn perlrs_jit_call_sub(
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
        assert_eq!(run_chunk(&c).expect("vm").to_string(), "perlrs");

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
