use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::bytecode::{
    BuiltinId, Chunk, Op, RuntimeSubDecl, GP_CHECK, GP_END, GP_INIT, GP_RUN, GP_START,
};
use crate::interpreter::{assign_rhs_wantarray, Interpreter, WantarrayCtx};
use crate::sort_fast::detect_sort_block_fast;
use crate::value::PerlValue;

/// True when one `{…}` entry expands to multiple hash keys (`qw/a b/`, or a list literal with 2+ elems).
pub(crate) fn hash_slice_key_expr_is_multi_key(k: &Expr) -> bool {
    match &k.kind {
        ExprKind::QW(ws) => ws.len() > 1,
        ExprKind::List(el) => el.len() > 1,
        _ => false,
    }
}

/// Use [`Op::HashSliceDeref`] / [`Op::HashSliceDerefCompound`] / [`Op::HashSliceDerefIncDec`], or
/// [`Op::NamedHashSliceCompound`] / [`Op::NamedHashSliceIncDec`] for stash `@h{…}`, instead of arrow-hash single-slot ops.
pub(crate) fn hash_slice_needs_slice_ops(keys: &[Expr]) -> bool {
    keys.len() != 1 || keys.first().is_some_and(hash_slice_key_expr_is_multi_key)
}

/// `$r->[EXPR] //=` / `||=` / `&&=` — the bytecode fast path uses [`Op::ArrowArray`] (scalar index).
/// Range / multi-word `qw`/list subscripts need different semantics; keep those on the tree walker.
/// `$r->[IX]` reads/writes via [`Op::ArrowArray`] only when `IX` is a **plain scalar** subscript.
/// `..` / `qw/.../` / `(a,b)` / nested lists always go through slice ops (flattened index specs).
pub(crate) fn arrow_deref_arrow_subscript_is_plain_scalar_index(index: &Expr) -> bool {
    match &index.kind {
        ExprKind::Range { .. } => false,
        ExprKind::QW(_) => false,
        ExprKind::List(el) => {
            if el.len() == 1 {
                arrow_deref_arrow_subscript_is_plain_scalar_index(&el[0])
            } else {
                false
            }
        }
        _ => !hash_slice_key_expr_is_multi_key(index),
    }
}

/// Compilation error — triggers fallback to tree-walker.
#[derive(Debug)]
pub enum CompileError {
    Unsupported(String),
    /// Immutable binding reassignment (e.g. `frozen my $x` then `$x = 1`).
    Frozen {
        line: usize,
        detail: String,
    },
}

#[derive(Default)]
struct ScopeLayer {
    declared_scalars: HashSet<String>,
    declared_arrays: HashSet<String>,
    declared_hashes: HashSet<String>,
    frozen_scalars: HashSet<String>,
    frozen_arrays: HashSet<String>,
    frozen_hashes: HashSet<String>,
    /// Slot-index mapping for `my` scalars in compiled subroutines.
    /// When `use_slots` is true, `my $x` is assigned a u8 slot index
    /// and the VM accesses it via `GetScalarSlot(idx)` — O(1).
    scalar_slots: HashMap<String, u8>,
    next_scalar_slot: u8,
    /// True when compiling a subroutine body (enables slot assignment).
    use_slots: bool,
    /// `mysync @name` — element `++`/`--`/compound assign must stay on the tree-walker (atomic RMW).
    mysync_arrays: HashSet<String>,
    /// `mysync %name` — same as [`Self::mysync_arrays`].
    mysync_hashes: HashSet<String>,
}

/// Loop context for resolving `last`/`next` jumps.
///
/// Pushed onto [`Compiler::loop_stack`] at every loop entry so `last`/`next` (including those
/// nested inside `if`/`unless`/`{ }` blocks) can find the matching loop and patch their jumps.
///
/// `entry_frame_depth` is [`Compiler::frame_depth`] at loop entry — `last`/`next` from inside
/// emits `(frame_depth - entry_frame_depth)` `Op::PopFrame` instructions before jumping so any
/// `if`/block-pushed scope frames are torn down.
///
/// `entry_try_depth` mirrors `try { }` nesting; if a `last`/`next` would have to cross a try
/// frame the compiler bails to `Unsupported` (try-frame unwind on flow control is not yet
/// modeled in bytecode — the catch handler would still see the next exception).
struct LoopCtx {
    label: Option<String>,
    entry_frame_depth: usize,
    entry_try_depth: usize,
    /// First bytecode IP of the loop **body** (after `while`/`until` condition, after `for` condition,
    /// after `foreach` assigns `$var` from the list, or `do` body start) — target for `redo`.
    body_start_ip: usize,
    /// Positions of `last`/`next` jumps to patch after the loop body is fully compiled.
    break_jumps: Vec<usize>,
    /// `Op::Jump(0)` placeholders for `next` — patched to the loop increment / condition entry.
    continue_jumps: Vec<usize>,
}

pub struct Compiler {
    pub chunk: Chunk,
    /// During compilation: stable [`Expr`] pointer → [`Chunk::ast_expr_pool`] index.
    ast_expr_intern: HashMap<usize, u32>,
    pub begin_blocks: Vec<Block>,
    pub unit_check_blocks: Vec<Block>,
    pub check_blocks: Vec<Block>,
    pub init_blocks: Vec<Block>,
    pub end_blocks: Vec<Block>,
    /// Lexical `my` declarations per scope frame (mirrors `PushFrame` / sub bodies).
    scope_stack: Vec<ScopeLayer>,
    /// Current `package` for stash qualification (`@ISA`, `@EXPORT`, …), matching [`Interpreter::stash_array_name_for_package`].
    current_package: String,
    /// Set while compiling the main program body when the last statement must leave its value on the
    /// stack (implicit return). Enables `try`/`catch` blocks to match `emit_block_value` semantics.
    program_last_stmt_takes_value: bool,
    /// Source path for `__FILE__` in bytecode (must match the interpreter's notion of current file when using the VM).
    pub source_file: String,
    /// Runtime activation depth — `Op::PushFrame` count minus `Op::PopFrame` count emitted so far.
    /// Used by `last`/`next` to compute how many frames to pop before jumping.
    frame_depth: usize,
    /// `try { }` nesting depth — `last`/`next` cannot currently cross a try-frame in bytecode.
    try_depth: usize,
    /// Active loops, innermost at the back. `last`/`next` consult this stack.
    loop_stack: Vec<LoopCtx>,
    /// Per-function (top-level program or sub body) `goto LABEL` tracking. Top of the stack holds
    /// the label→IP map and forward-goto patch list for the innermost enclosing label-scoped
    /// region. `goto` is only resolved against the top frame (matches Perl's "goto must target a
    /// label in the same lexical context" intuition).
    goto_ctx_stack: Vec<GotoCtx>,
    /// `use strict 'vars'` — reject access to undeclared globals at compile time (mirrors the
    /// tree-walker's `Interpreter::check_strict_*_var` runtime checks). Set via
    /// [`Self::with_strict_vars`] before `compile_program` runs; stable throughout a single
    /// compile because `use strict` is resolved in `prepare_program_top_level` before the VM
    /// compile begins.
    strict_vars: bool,
}

/// Label tracking for `goto LABEL` within a single label-scoped region (top-level main program
/// or subroutine body). See [`Compiler::enter_goto_scope`] / [`Compiler::exit_goto_scope`].
#[derive(Default)]
struct GotoCtx {
    /// `label_name → (bytecode IP of the labeled statement's first op, frame_depth at label)`
    labels: HashMap<String, (usize, usize)>,
    /// `(jump_op_ip, label_name, source_line, frame_depth_at_goto)` for forward `goto LABEL`.
    pending: Vec<(usize, String, usize, usize)>,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    /// Array/hash slice subscripts: `1..3` is list context (range list); other exprs stay scalar.
    fn compile_array_slice_index_expr(&mut self, index_expr: &Expr) -> Result<(), CompileError> {
        if matches!(&index_expr.kind, ExprKind::Range { .. }) {
            self.compile_expr_ctx(index_expr, WantarrayCtx::List)
        } else {
            self.compile_expr(index_expr)
        }
    }

    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
            ast_expr_intern: HashMap::new(),
            begin_blocks: Vec::new(),
            unit_check_blocks: Vec::new(),
            check_blocks: Vec::new(),
            init_blocks: Vec::new(),
            end_blocks: Vec::new(),
            // Main program `my $x` uses [`Op::GetScalarSlot`] / [`Op::SetScalarSlot`] like subs,
            // so hot loops are not stuck on [`Op::GetScalarPlain`] (linear scan per access).
            scope_stack: vec![ScopeLayer {
                use_slots: true,
                ..Default::default()
            }],
            current_package: String::new(),
            program_last_stmt_takes_value: false,
            source_file: String::new(),
            frame_depth: 0,
            try_depth: 0,
            loop_stack: Vec::new(),
            goto_ctx_stack: Vec::new(),
            strict_vars: false,
        }
    }

    /// Set `use strict 'vars'` at compile time. When enabled, [`compile_expr`] rejects any read
    /// or write of an undeclared global scalar / array / hash with `CompileError::Frozen` — the
    /// same diagnostic the tree-walker emits at runtime (`Global symbol "$name" requires
    /// explicit package name`). `try_vm_execute` pulls the flag from `Interpreter::strict_vars`
    /// before constructing the compiler, matching the timing of the tree path's
    /// `prepare_program_top_level` (which processes `use strict` before main body execution).
    pub fn with_strict_vars(mut self, v: bool) -> Self {
        self.strict_vars = v;
        self
    }

    /// Enter a `goto LABEL` scope (called when compiling the top-level main program or a sub
    /// body). Labels defined inside can be targeted from any `goto` inside the same scope;
    /// labels are *not* shared across nested functions.
    fn enter_goto_scope(&mut self) {
        self.goto_ctx_stack.push(GotoCtx::default());
    }

    /// Resolve all pending forward gotos and pop the scope. Returns `CompileError::Frozen` if a
    /// `goto` targets a label that was never defined in this scope (same diagnostic the tree
    /// interpreter returns at runtime: `goto: unknown label NAME`). Returns `Unsupported` if a
    /// `goto` crosses a frame boundary (e.g. from inside an `if` body out to an outer label) —
    /// crossing frames would skip `PopFrame` ops and corrupt the scope stack. That case falls
    /// back to the tree interpreter for now.
    fn exit_goto_scope(&mut self) -> Result<(), CompileError> {
        let ctx = self
            .goto_ctx_stack
            .pop()
            .expect("exit_goto_scope called without matching enter");
        for (jump_ip, label, line, goto_frame_depth) in ctx.pending {
            if let Some(&(target_ip, label_frame_depth)) = ctx.labels.get(&label) {
                if label_frame_depth != goto_frame_depth {
                    return Err(CompileError::Unsupported(format!(
                        "goto LABEL crosses a scope frame (label `{}` at depth {} vs goto at depth {})",
                        label, label_frame_depth, goto_frame_depth
                    )));
                }
                self.chunk.patch_jump_to(jump_ip, target_ip);
            } else {
                return Err(CompileError::Frozen {
                    line,
                    detail: format!("goto: unknown label {}", label),
                });
            }
        }
        Ok(())
    }

    /// Record `label → current IP` if a goto-scope is active. Called before each labeled
    /// statement is emitted; the label points to the first op of the statement.
    fn record_stmt_label(&mut self, label: &str) {
        if let Some(top) = self.goto_ctx_stack.last_mut() {
            top.labels
                .insert(label.to_string(), (self.chunk.len(), self.frame_depth));
        }
    }

    /// If `target` is a compile-time-known label name (bareword or literal string), emit a
    /// forward `Jump(0)` and record it for patching on goto-scope exit. Returns `true` if the
    /// goto was handled (so the caller should not emit a fallback). Returns `false` if the target
    /// is dynamic — the caller should bail to `CompileError::Unsupported` so the tree path can
    /// still handle it in future.
    fn try_emit_goto_label(&mut self, target: &Expr, line: usize) -> bool {
        let name = match &target.kind {
            ExprKind::Bareword(n) => n.clone(),
            ExprKind::String(s) => s.clone(),
            _ => return false,
        };
        if self.goto_ctx_stack.is_empty() {
            return false;
        }
        let jump_ip = self.chunk.emit(Op::Jump(0), line);
        let frame_depth = self.frame_depth;
        self.goto_ctx_stack
            .last_mut()
            .expect("goto scope must be active")
            .pending
            .push((jump_ip, name, line, frame_depth));
        true
    }

    /// Emit `Op::PushFrame` and bump [`Self::frame_depth`].
    fn emit_push_frame(&mut self, line: usize) {
        self.chunk.emit(Op::PushFrame, line);
        self.frame_depth += 1;
    }

    /// Emit `Op::PopFrame` and decrement [`Self::frame_depth`] (saturating).
    fn emit_pop_frame(&mut self, line: usize) {
        self.chunk.emit(Op::PopFrame, line);
        self.frame_depth = self.frame_depth.saturating_sub(1);
    }

    pub fn with_source_file(mut self, path: String) -> Self {
        self.source_file = path;
        self
    }

    /// `@ISA` / `@EXPORT` / `@EXPORT_OK` outside `main` → `Pkg::NAME` (see interpreter stash rules).
    fn qualify_stash_array_name(&self, name: &str) -> String {
        if matches!(name, "ISA" | "EXPORT" | "EXPORT_OK") {
            let pkg = &self.current_package;
            if !pkg.is_empty() && pkg != "main" {
                return format!("{}::{}", pkg, name);
            }
        }
        name.to_string()
    }

    /// Stash key for a subroutine name in the current package (matches [`Interpreter::qualify_sub_key`]).
    fn qualify_sub_key(&self, name: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        let pkg = &self.current_package;
        if pkg.is_empty() || pkg == "main" {
            name.to_string()
        } else {
            format!("{}::{}", pkg, name)
        }
    }

    /// First-pass sub registration: walk `package` statements like [`Self::compile_program`] does for
    /// sub bodies so forward `sub` entries use the same stash key as runtime registration.
    fn qualify_sub_decl_pass1(name: &str, pending_pkg: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        if pending_pkg.is_empty() || pending_pkg == "main" {
            name.to_string()
        } else {
            format!("{}::{}", pending_pkg, name)
        }
    }

    /// After all `sub` bodies are lowered, replace [`Op::Call`] with [`Op::CallStaticSubId`] when the
    /// callee has a compiled entry (avoids linear `sub_entries` scan + extra stash work per call).
    fn patch_static_sub_calls(chunk: &mut Chunk) {
        for i in 0..chunk.ops.len() {
            if let Op::Call(name_idx, argc, wa) = chunk.ops[i] {
                if let Some((entry_ip, stack_args)) = chunk.find_sub_entry(name_idx) {
                    if chunk.static_sub_calls.len() < u16::MAX as usize {
                        let sid = chunk.static_sub_calls.len() as u16;
                        chunk
                            .static_sub_calls
                            .push((entry_ip, stack_args, name_idx));
                        chunk.ops[i] = Op::CallStaticSubId(sid, name_idx, argc, wa);
                    }
                }
            }
        }
    }

    /// For `$aref->[ix]` / `@$r[ix]` arrow-array ops: the stack must hold the **array reference**
    /// (scalar), not `@{...}` / `@$r` expansion (which would push a cloned plain array).
    fn compile_arrow_array_base_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        if let ExprKind::Deref {
            expr: inner,
            kind: Sigil::Array | Sigil::Scalar,
        } = &expr.kind
        {
            self.compile_expr(inner)
        } else {
            self.compile_expr(expr)
        }
    }

    /// For `$href->{k}` / `$$r{k}`: stack holds the hash **reference** scalar, not a copied `%` value.
    fn compile_arrow_hash_base_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        if let ExprKind::Deref {
            expr: inner,
            kind: Sigil::Scalar,
        } = &expr.kind
        {
            self.compile_expr(inner)
        } else {
            self.compile_expr(expr)
        }
    }

    fn push_scope_layer(&mut self) {
        self.scope_stack.push(ScopeLayer::default());
    }

    /// Push a scope layer with slot assignment enabled (for subroutine bodies).
    fn push_scope_layer_with_slots(&mut self) {
        self.scope_stack.push(ScopeLayer {
            use_slots: true,
            ..Default::default()
        });
    }

    fn pop_scope_layer(&mut self) {
        if self.scope_stack.len() > 1 {
            self.scope_stack.pop();
        }
    }

    /// Look up a scalar's slot index in the current scope layer (if slots are enabled).
    fn scalar_slot(&self, name: &str) -> Option<u8> {
        if let Some(layer) = self.scope_stack.last() {
            if layer.use_slots {
                return layer.scalar_slots.get(name).copied();
            }
        }
        None
    }

    /// Intern an [`Expr`] for [`Chunk::op_ast_expr`] (pointer-stable during compile).
    fn intern_ast_expr(&mut self, expr: &Expr) -> u32 {
        let p = expr as *const Expr as usize;
        if let Some(&id) = self.ast_expr_intern.get(&p) {
            return id;
        }
        let id = self.chunk.ast_expr_pool.len() as u32;
        self.chunk.ast_expr_pool.push(expr.clone());
        self.ast_expr_intern.insert(p, id);
        id
    }

    /// Emit one opcode with optional link to the originating expression (expression compiler path).
    #[inline]
    fn emit_op(&mut self, op: Op, line: usize, ast: Option<&Expr>) -> usize {
        let idx = ast.map(|e| self.intern_ast_expr(e));
        self.chunk.emit_with_ast_idx(op, line, idx)
    }

    /// Emit GetScalar or GetScalarSlot depending on whether the variable has a slot.
    fn emit_get_scalar(&mut self, name_idx: u16, line: usize, ast: Option<&Expr>) {
        let name = &self.chunk.names[name_idx as usize];
        if let Some(slot) = self.scalar_slot(name) {
            self.emit_op(Op::GetScalarSlot(slot), line, ast);
        } else if Interpreter::is_special_scalar_name_for_get(name) {
            self.emit_op(Op::GetScalar(name_idx), line, ast);
        } else {
            self.emit_op(Op::GetScalarPlain(name_idx), line, ast);
        }
    }

    /// Boolean rvalue: bare `/.../` is `$_ =~ /.../` (Perl), not “regex object is truthy”.
    /// Emits `$_` + pattern and [`Op::RegexMatchDyn`] so match vars and truthy 0/1 match `=~`.
    fn compile_boolean_rvalue_condition(&mut self, cond: &Expr) -> Result<(), CompileError> {
        let line = cond.line;
        if let ExprKind::Regex(pattern, flags) = &cond.kind {
            let name_idx = self.chunk.intern_name("_");
            self.emit_get_scalar(name_idx, line, Some(cond));
            let pat_idx = self.chunk.add_constant(PerlValue::string(pattern.clone()));
            let flags_idx = self.chunk.add_constant(PerlValue::string(flags.clone()));
            self.emit_op(Op::LoadRegex(pat_idx, flags_idx), line, Some(cond));
            self.emit_op(Op::RegexMatchDyn(false), line, Some(cond));
            Ok(())
        } else if matches!(&cond.kind, ExprKind::ReadLine(_)) {
            // `while (<STDIN>)` — assign line to `$_` then test definedness (Perl).
            self.compile_expr(cond)?;
            let name_idx = self.chunk.intern_name("_");
            self.emit_set_scalar_keep(name_idx, line, Some(cond));
            self.emit_op(
                Op::CallBuiltin(BuiltinId::Defined as u16, 1),
                line,
                Some(cond),
            );
            Ok(())
        } else {
            self.compile_expr(cond)
        }
    }

    /// Emit SetScalar or SetScalarSlot depending on slot availability.
    fn emit_set_scalar(&mut self, name_idx: u16, line: usize, ast: Option<&Expr>) {
        let name = &self.chunk.names[name_idx as usize];
        if let Some(slot) = self.scalar_slot(name) {
            self.emit_op(Op::SetScalarSlot(slot), line, ast);
        } else if Interpreter::is_special_scalar_name_for_set(name) {
            self.emit_op(Op::SetScalar(name_idx), line, ast);
        } else {
            self.emit_op(Op::SetScalarPlain(name_idx), line, ast);
        }
    }

    /// Emit SetScalarKeep or SetScalarSlotKeep depending on slot availability.
    fn emit_set_scalar_keep(&mut self, name_idx: u16, line: usize, ast: Option<&Expr>) {
        let name = &self.chunk.names[name_idx as usize];
        if let Some(slot) = self.scalar_slot(name) {
            self.emit_op(Op::SetScalarSlotKeep(slot), line, ast);
        } else if Interpreter::is_special_scalar_name_for_set(name) {
            self.emit_op(Op::SetScalarKeep(name_idx), line, ast);
        } else {
            self.emit_op(Op::SetScalarKeepPlain(name_idx), line, ast);
        }
    }

    fn emit_pre_inc(&mut self, name_idx: u16, line: usize, ast: Option<&Expr>) {
        let name = &self.chunk.names[name_idx as usize];
        if let Some(slot) = self.scalar_slot(name) {
            self.emit_op(Op::PreIncSlot(slot), line, ast);
        } else {
            self.emit_op(Op::PreInc(name_idx), line, ast);
        }
    }

    fn emit_pre_dec(&mut self, name_idx: u16, line: usize, ast: Option<&Expr>) {
        let name = &self.chunk.names[name_idx as usize];
        if let Some(slot) = self.scalar_slot(name) {
            self.emit_op(Op::PreDecSlot(slot), line, ast);
        } else {
            self.emit_op(Op::PreDec(name_idx), line, ast);
        }
    }

    fn emit_post_inc(&mut self, name_idx: u16, line: usize, ast: Option<&Expr>) {
        let name = &self.chunk.names[name_idx as usize];
        if let Some(slot) = self.scalar_slot(name) {
            self.emit_op(Op::PostIncSlot(slot), line, ast);
        } else {
            self.emit_op(Op::PostInc(name_idx), line, ast);
        }
    }

    fn emit_post_dec(&mut self, name_idx: u16, line: usize, ast: Option<&Expr>) {
        let name = &self.chunk.names[name_idx as usize];
        if let Some(slot) = self.scalar_slot(name) {
            self.emit_op(Op::PostDecSlot(slot), line, ast);
        } else {
            self.emit_op(Op::PostDec(name_idx), line, ast);
        }
    }

    /// Assign a new slot index for a scalar in the current scope layer.
    /// Returns the slot index if slots are enabled, None otherwise.
    fn assign_scalar_slot(&mut self, name: &str) -> Option<u8> {
        if let Some(layer) = self.scope_stack.last_mut() {
            if layer.use_slots && layer.next_scalar_slot < 255 {
                let slot = layer.next_scalar_slot;
                layer.scalar_slots.insert(name.to_string(), slot);
                layer.next_scalar_slot += 1;
                return Some(slot);
            }
        }
        None
    }

    fn register_declare(&mut self, sigil: Sigil, name: &str, frozen: bool) {
        let layer = self.scope_stack.last_mut().expect("scope stack");
        match sigil {
            Sigil::Scalar => {
                layer.declared_scalars.insert(name.to_string());
                if frozen {
                    layer.frozen_scalars.insert(name.to_string());
                }
            }
            Sigil::Array => {
                layer.declared_arrays.insert(name.to_string());
                if frozen {
                    layer.frozen_arrays.insert(name.to_string());
                }
            }
            Sigil::Hash => {
                layer.declared_hashes.insert(name.to_string());
                if frozen {
                    layer.frozen_hashes.insert(name.to_string());
                }
            }
            Sigil::Typeglob => {
                layer.declared_scalars.insert(name.to_string());
            }
        }
    }

    /// `use strict 'vars'` check for a scalar `$name`. Mirrors [`Interpreter::check_strict_scalar_var`]:
    /// ok if strict is off, the name contains `::` (package-qualified), the name is a Perl special
    /// scalar, or the name is declared via `my`/`our` in any enclosing compiler scope layer.
    /// Otherwise errors with the exact tree-walker diagnostic message so the user sees the same
    /// error whether execution goes via VM or tree fallback.
    fn check_strict_scalar_access(&self, name: &str, line: usize) -> Result<(), CompileError> {
        if !self.strict_vars
            || name.contains("::")
            || Interpreter::strict_scalar_exempt(name)
            || Interpreter::is_special_scalar_name_for_get(name)
            || self
                .scope_stack
                .iter()
                .any(|l| l.declared_scalars.contains(name))
        {
            return Ok(());
        }
        Err(CompileError::Frozen {
            line,
            detail: format!(
                "Global symbol \"${}\" requires explicit package name (did you forget to declare \"my ${}\"?)",
                name, name
            ),
        })
    }

    /// Array names that are always bound at runtime (Perl built-ins) and must not trigger a
    /// `use strict 'vars'` compile error even though they're never `my`-declared.
    fn strict_array_exempt(name: &str) -> bool {
        matches!(
            name,
            "_" | "ARGV" | "INC" | "ENV" | "ISA" | "EXPORT" | "EXPORT_OK" | "EXPORT_FAIL"
        )
    }

    /// Hash names that are always bound at runtime.
    fn strict_hash_exempt(name: &str) -> bool {
        matches!(
            name,
            "ENV" | "INC" | "SIG" | "EXPORT_TAGS" | "ISA" | "OVERLOAD"
        )
    }

    fn check_strict_array_access(&self, name: &str, line: usize) -> Result<(), CompileError> {
        if !self.strict_vars
            || name.contains("::")
            || Self::strict_array_exempt(name)
            || self
                .scope_stack
                .iter()
                .any(|l| l.declared_arrays.contains(name))
        {
            return Ok(());
        }
        Err(CompileError::Frozen {
            line,
            detail: format!(
                "Global symbol \"@{}\" requires explicit package name (did you forget to declare \"my @{}\"?)",
                name, name
            ),
        })
    }

    fn check_strict_hash_access(&self, name: &str, line: usize) -> Result<(), CompileError> {
        if !self.strict_vars
            || name.contains("::")
            || Self::strict_hash_exempt(name)
            || self
                .scope_stack
                .iter()
                .any(|l| l.declared_hashes.contains(name))
        {
            return Ok(());
        }
        Err(CompileError::Frozen {
            line,
            detail: format!(
                "Global symbol \"%{}\" requires explicit package name (did you forget to declare \"my %{}\"?)",
                name, name
            ),
        })
    }

    fn check_scalar_mutable(&self, name: &str, line: usize) -> Result<(), CompileError> {
        for layer in self.scope_stack.iter().rev() {
            if layer.declared_scalars.contains(name) {
                if layer.frozen_scalars.contains(name) {
                    return Err(CompileError::Frozen {
                        line,
                        detail: format!("cannot assign to frozen variable `${}`", name),
                    });
                }
                return Ok(());
            }
        }
        Ok(())
    }

    fn check_array_mutable(&self, name: &str, line: usize) -> Result<(), CompileError> {
        for layer in self.scope_stack.iter().rev() {
            if layer.declared_arrays.contains(name) {
                if layer.frozen_arrays.contains(name) {
                    return Err(CompileError::Frozen {
                        line,
                        detail: format!("cannot modify frozen array `@{}`", name),
                    });
                }
                return Ok(());
            }
        }
        Ok(())
    }

    fn check_hash_mutable(&self, name: &str, line: usize) -> Result<(), CompileError> {
        for layer in self.scope_stack.iter().rev() {
            if layer.declared_hashes.contains(name) {
                if layer.frozen_hashes.contains(name) {
                    return Err(CompileError::Frozen {
                        line,
                        detail: format!("cannot modify frozen hash `%{}`", name),
                    });
                }
                return Ok(());
            }
        }
        Ok(())
    }

    /// Emit an `Op::RuntimeErrorConst` that matches the tree-walker's
    /// `Can't modify {array,hash} dereference in {pre,post}{increment,decrement} (++|--)` message.
    /// Used for `++@{…}`, `%{…}--`, `@$r++`, etc. — constructs that are invalid in Perl 5.
    /// Pushes `LoadUndef` afterwards so the rvalue position has a value on the stack for any
    /// surrounding `Pop` from statement-expression dispatch (the error op aborts the VM before
    /// the `LoadUndef` is reached, but it keeps the emitted sequence well-formed for stack tracking).
    fn emit_aggregate_symbolic_inc_dec_error(
        &mut self,
        kind: Sigil,
        is_pre: bool,
        is_inc: bool,
        line: usize,
        root: &Expr,
    ) -> Result<(), CompileError> {
        let agg = match kind {
            Sigil::Array => "array",
            Sigil::Hash => "hash",
            _ => {
                return Err(CompileError::Unsupported(
                    "internal: non-aggregate sigil passed to symbolic ++/-- error emitter".into(),
                ));
            }
        };
        let op_str = match (is_pre, is_inc) {
            (true, true) => "preincrement (++)",
            (true, false) => "predecrement (--)",
            (false, true) => "postincrement (++)",
            (false, false) => "postdecrement (--)",
        };
        let msg = format!("Can't modify {} dereference in {}", agg, op_str);
        let idx = self.chunk.add_constant(PerlValue::string(msg));
        self.emit_op(Op::RuntimeErrorConst(idx), line, Some(root));
        // The op never returns; this LoadUndef is dead code but keeps any unreachable
        // `Pop` / rvalue consumer emitted by the enclosing dispatch well-formed.
        self.emit_op(Op::LoadUndef, line, Some(root));
        Ok(())
    }

    /// `mysync @arr` / `mysync %h` — aggregate element updates use `atomic_*_mutate` in the tree interpreter only.
    fn is_mysync_array(&self, array_name: &str) -> bool {
        let q = self.qualify_stash_array_name(array_name);
        self.scope_stack
            .iter()
            .rev()
            .any(|l| l.mysync_arrays.contains(&q))
    }

    fn is_mysync_hash(&self, hash_name: &str) -> bool {
        self.scope_stack
            .iter()
            .rev()
            .any(|l| l.mysync_hashes.contains(hash_name))
    }

    pub fn compile_program(mut self, program: &Program) -> Result<Chunk, CompileError> {
        // Extract BEGIN/END blocks before compiling.
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::Begin(block) => self.begin_blocks.push(block.clone()),
                StmtKind::UnitCheck(block) => self.unit_check_blocks.push(block.clone()),
                StmtKind::Check(block) => self.check_blocks.push(block.clone()),
                StmtKind::Init(block) => self.init_blocks.push(block.clone()),
                StmtKind::End(block) => self.end_blocks.push(block.clone()),
                _ => {}
            }
        }

        // First pass: register sub names for forward calls (qualified stash keys, same as runtime).
        let mut pending_pkg = String::new();
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::Package { name } => pending_pkg = name.clone(),
                StmtKind::SubDecl { name, .. } => {
                    let q = Self::qualify_sub_decl_pass1(name, &pending_pkg);
                    let name_idx = self.chunk.intern_name(&q);
                    self.chunk.sub_entries.push((name_idx, 0, false));
                }
                _ => {}
            }
        }

        // Second pass: compile main body.
        // The last expression statement keeps its value on the stack so the
        // caller can read the program's return value (like Perl's implicit return).
        let main_stmts: Vec<&Statement> = program
            .statements
            .iter()
            .filter(|s| {
                !matches!(
                    s.kind,
                    StmtKind::SubDecl { .. }
                        | StmtKind::Begin(_)
                        | StmtKind::UnitCheck(_)
                        | StmtKind::Check(_)
                        | StmtKind::Init(_)
                        | StmtKind::End(_)
                )
            })
            .collect();
        let last_idx = main_stmts.len().saturating_sub(1);
        self.program_last_stmt_takes_value = main_stmts
            .last()
            .map(|s| matches!(s.kind, StmtKind::TryCatch { .. }))
            .unwrap_or(false);
        // BEGIN blocks run before main (same order as [`Interpreter::execute_tree`]).
        if !self.begin_blocks.is_empty() {
            self.chunk.emit(Op::SetGlobalPhase(GP_START), 0);
        }
        for block in &self.begin_blocks.clone() {
            self.compile_block(block)?;
        }
        // Perl: `${^GLOBAL_PHASE}` stays **`START`** during UNITCHECK blocks (see `execute_tree`).
        let unit_check_rev: Vec<Block> = self.unit_check_blocks.iter().rev().cloned().collect();
        for block in unit_check_rev {
            self.compile_block(&block)?;
        }
        if !self.check_blocks.is_empty() {
            self.chunk.emit(Op::SetGlobalPhase(GP_CHECK), 0);
        }
        let check_rev: Vec<Block> = self.check_blocks.iter().rev().cloned().collect();
        for block in check_rev {
            self.compile_block(&block)?;
        }
        if !self.init_blocks.is_empty() {
            self.chunk.emit(Op::SetGlobalPhase(GP_INIT), 0);
        }
        let inits = self.init_blocks.clone();
        for block in inits {
            self.compile_block(&block)?;
        }
        self.chunk.emit(Op::SetGlobalPhase(GP_RUN), 0);

        // Top-level `goto LABEL` scope: labels defined on main-program statements are targetable
        // from `goto` statements in the same main program. Pushed before the main loop and
        // resolved after it (but before END blocks, which run in their own scope).
        self.enter_goto_scope();

        let mut i = 0;
        while i < main_stmts.len() {
            let stmt = main_stmts[i];
            if i == last_idx {
                // The specialized `last statement leaves its value on the stack` path bypasses
                // `compile_statement` for Expression/If/Unless shapes, so we must record any
                // `LABEL:` on this statement manually before emitting its ops.
                if let Some(lbl) = &stmt.label {
                    self.record_stmt_label(lbl);
                }
                match &stmt.kind {
                    StmtKind::Expression(expr) => {
                        // Last statement of program: still not a regex *value* — bare `/pat/` matches `$_`.
                        if matches!(&expr.kind, ExprKind::Regex(..)) {
                            self.compile_boolean_rvalue_condition(expr)?;
                        } else {
                            self.compile_expr(expr)?;
                        }
                    }
                    StmtKind::If {
                        condition,
                        body,
                        elsifs,
                        else_block,
                    } => {
                        self.compile_boolean_rvalue_condition(condition)?;
                        let j0 = self.chunk.emit(Op::JumpIfFalse(0), stmt.line);
                        self.emit_block_value(body, stmt.line)?;
                        let mut ends = vec![self.chunk.emit(Op::Jump(0), stmt.line)];
                        self.chunk.patch_jump_here(j0);
                        for (c, blk) in elsifs {
                            self.compile_boolean_rvalue_condition(c)?;
                            let j = self.chunk.emit(Op::JumpIfFalse(0), c.line);
                            self.emit_block_value(blk, c.line)?;
                            ends.push(self.chunk.emit(Op::Jump(0), c.line));
                            self.chunk.patch_jump_here(j);
                        }
                        if let Some(eb) = else_block {
                            self.emit_block_value(eb, stmt.line)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, stmt.line);
                        }
                        for j in ends {
                            self.chunk.patch_jump_here(j);
                        }
                    }
                    StmtKind::Unless {
                        condition,
                        body,
                        else_block,
                    } => {
                        self.compile_boolean_rvalue_condition(condition)?;
                        let j0 = self.chunk.emit(Op::JumpIfFalse(0), stmt.line);
                        if let Some(eb) = else_block {
                            self.emit_block_value(eb, stmt.line)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, stmt.line);
                        }
                        let end = self.chunk.emit(Op::Jump(0), stmt.line);
                        self.chunk.patch_jump_here(j0);
                        self.emit_block_value(body, stmt.line)?;
                        self.chunk.patch_jump_here(end);
                    }
                    _ => self.compile_statement(stmt)?,
                }
            } else {
                self.compile_statement(stmt)?;
            }
            i += 1;
        }
        self.program_last_stmt_takes_value = false;

        // Resolve all forward `goto LABEL` against labels recorded in the main scope.
        self.exit_goto_scope()?;

        // END blocks run after main, before halt (same order as [`Interpreter::execute_tree`]).
        if !self.end_blocks.is_empty() {
            self.chunk.emit(Op::SetGlobalPhase(GP_END), 0);
        }
        for block in &self.end_blocks.clone() {
            self.compile_block(block)?;
        }

        self.chunk.emit(Op::Halt, 0);

        // Third pass: compile sub bodies after Halt
        let mut entries: Vec<(String, Vec<Statement>, String)> = Vec::new();
        let mut pending_pkg = String::new();
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::Package { name } => pending_pkg = name.clone(),
                StmtKind::SubDecl { name, body, .. } => {
                    entries.push((name.clone(), body.clone(), pending_pkg.clone()));
                }
                _ => {}
            }
        }

        for (name, body, sub_pkg) in &entries {
            let saved_pkg = self.current_package.clone();
            self.current_package = sub_pkg.clone();
            self.push_scope_layer_with_slots();
            let entry_ip = self.chunk.len();
            let q = self.qualify_sub_key(name);
            let name_idx = self.chunk.intern_name(&q);
            // Patch the entry point
            for e in &mut self.chunk.sub_entries {
                if e.0 == name_idx {
                    e.1 = entry_ip;
                }
            }
            // Each sub body gets its own `goto LABEL` scope: labels are not visible across
            // different subs or between a sub and the main program.
            self.enter_goto_scope();
            // Compile sub body (VM `Call` pushes a scope frame; mirror for frozen tracking).
            self.emit_subroutine_body_return(body)?;
            self.exit_goto_scope()?;
            self.pop_scope_layer();

            // Peephole: convert leading `ShiftArray("_")` to `GetArg(n)` if @_ is
            // not referenced by any other op in this sub. This eliminates Vec
            // allocation + string-based @_ lookup on every call.
            let underscore_idx = self.chunk.intern_name("_");
            self.peephole_stack_args(name_idx, entry_ip, underscore_idx);
            self.current_package = saved_pkg;
        }

        // Fourth pass: lower simple map/grep/sort block bodies to bytecode (after subs; same `ops` vec).
        self.chunk.block_bytecode_ranges = vec![None; self.chunk.blocks.len()];
        for i in 0..self.chunk.blocks.len() {
            let b = self.chunk.blocks[i].clone();
            if Self::block_has_return(&b) {
                continue;
            }
            if let Ok(range) = self.try_compile_block_region(&b) {
                self.chunk.block_bytecode_ranges[i] = Some(range);
            }
        }

        // Fifth pass: `map EXPR, LIST` — list-context expression per `$_` (same `ops` vec as blocks).
        self.chunk.map_expr_bytecode_ranges = vec![None; self.chunk.map_expr_entries.len()];
        for i in 0..self.chunk.map_expr_entries.len() {
            let e = self.chunk.map_expr_entries[i].clone();
            if let Ok(range) = self.try_compile_grep_expr_region(&e, WantarrayCtx::List) {
                self.chunk.map_expr_bytecode_ranges[i] = Some(range);
            }
        }

        // Fifth pass (a): `grep EXPR, LIST` — single-expression filter bodies (same `ops` vec as blocks).
        self.chunk.grep_expr_bytecode_ranges = vec![None; self.chunk.grep_expr_entries.len()];
        for i in 0..self.chunk.grep_expr_entries.len() {
            let e = self.chunk.grep_expr_entries[i].clone();
            if let Ok(range) = self.try_compile_grep_expr_region(&e, WantarrayCtx::Scalar) {
                self.chunk.grep_expr_bytecode_ranges[i] = Some(range);
            }
        }

        // Fifth pass (b): regex flip-flop compound RHS — boolean context (same `ops` vec).
        self.chunk.regex_flip_flop_rhs_expr_bytecode_ranges =
            vec![None; self.chunk.regex_flip_flop_rhs_expr_entries.len()];
        for i in 0..self.chunk.regex_flip_flop_rhs_expr_entries.len() {
            let e = self.chunk.regex_flip_flop_rhs_expr_entries[i].clone();
            if let Ok(range) = self.try_compile_flip_flop_rhs_expr_region(&e) {
                self.chunk.regex_flip_flop_rhs_expr_bytecode_ranges[i] = Some(range);
            }
        }

        // Sixth pass: `eval_timeout EXPR { ... }` — timeout expression only (body stays interpreter).
        self.chunk.eval_timeout_expr_bytecode_ranges =
            vec![None; self.chunk.eval_timeout_entries.len()];
        for i in 0..self.chunk.eval_timeout_entries.len() {
            let timeout_expr = self.chunk.eval_timeout_entries[i].0.clone();
            if let Ok(range) =
                self.try_compile_grep_expr_region(&timeout_expr, WantarrayCtx::Scalar)
            {
                self.chunk.eval_timeout_expr_bytecode_ranges[i] = Some(range);
            }
        }

        // Seventh pass: `keys EXPR` / `values EXPR` — operand expression only.
        self.chunk.keys_expr_bytecode_ranges = vec![None; self.chunk.keys_expr_entries.len()];
        for i in 0..self.chunk.keys_expr_entries.len() {
            let e = self.chunk.keys_expr_entries[i].clone();
            if let Ok(range) = self.try_compile_grep_expr_region(&e, WantarrayCtx::List) {
                self.chunk.keys_expr_bytecode_ranges[i] = Some(range);
            }
        }
        self.chunk.values_expr_bytecode_ranges = vec![None; self.chunk.values_expr_entries.len()];
        for i in 0..self.chunk.values_expr_entries.len() {
            let e = self.chunk.values_expr_entries[i].clone();
            if let Ok(range) = self.try_compile_grep_expr_region(&e, WantarrayCtx::List) {
                self.chunk.values_expr_bytecode_ranges[i] = Some(range);
            }
        }

        // Eighth pass: `given (TOPIC) { ... }` — topic expression only.
        self.chunk.given_topic_bytecode_ranges = vec![None; self.chunk.given_entries.len()];
        for i in 0..self.chunk.given_entries.len() {
            let topic = self.chunk.given_entries[i].0.clone();
            if let Ok(range) = self.try_compile_grep_expr_region(&topic, WantarrayCtx::Scalar) {
                self.chunk.given_topic_bytecode_ranges[i] = Some(range);
            }
        }

        // Ninth pass: algebraic `match (SUBJECT) { ... }` — subject expression only.
        self.chunk.algebraic_match_subject_bytecode_ranges =
            vec![None; self.chunk.algebraic_match_entries.len()];
        for i in 0..self.chunk.algebraic_match_entries.len() {
            let subject = self.chunk.algebraic_match_entries[i].0.clone();
            if let Ok(range) = self.try_compile_grep_expr_region(&subject, WantarrayCtx::Scalar) {
                self.chunk.algebraic_match_subject_bytecode_ranges[i] = Some(range);
            }
        }

        Self::patch_static_sub_calls(&mut self.chunk);
        self.chunk.peephole_fuse();

        Ok(self.chunk)
    }

    /// Lower a block body to `ops` ending in [`Op::BlockReturnValue`] when possible.
    ///
    /// Matches `Interpreter::exec_block_no_scope` for blocks **without** `return`: last statement
    /// must be [`StmtKind::Expression`] (the value is that expression). Earlier statements use
    /// [`Self::compile_statement`] (void context). Any `CompileError` keeps AST fallback.
    fn try_compile_block_region(&mut self, block: &Block) -> Result<(usize, usize), CompileError> {
        let line0 = block.first().map(|s| s.line).unwrap_or(0);
        let start = self.chunk.len();
        if block.is_empty() {
            self.chunk.emit(Op::LoadUndef, line0);
            self.chunk.emit(Op::BlockReturnValue, line0);
            return Ok((start, self.chunk.len()));
        }
        let last = block.last().expect("non-empty block");
        let StmtKind::Expression(expr) = &last.kind else {
            return Err(CompileError::Unsupported(
                "block last statement must be an expression for bytecode lowering".into(),
            ));
        };
        for stmt in &block[..block.len() - 1] {
            self.compile_statement(stmt)?;
        }
        let line = last.line;
        self.compile_expr(expr)?;
        self.chunk.emit(Op::BlockReturnValue, line);
        Ok((start, self.chunk.len()))
    }

    /// Lower a single expression to `ops` ending in [`Op::BlockReturnValue`].
    ///
    /// Used for `grep EXPR, LIST` (with `$_` set by the VM per item), `eval_timeout EXPR { ... }`,
    /// `keys EXPR` / `values EXPR` operands, `given (TOPIC) { ... }` topic, algebraic `match (SUBJECT)`
    /// subject, and similar one-shot regions matching [`Interpreter::eval_expr`].
    fn try_compile_grep_expr_region(
        &mut self,
        expr: &Expr,
        ctx: WantarrayCtx,
    ) -> Result<(usize, usize), CompileError> {
        let line = expr.line;
        let start = self.chunk.len();
        self.compile_expr_ctx(expr, ctx)?;
        self.chunk.emit(Op::BlockReturnValue, line);
        Ok((start, self.chunk.len()))
    }

    /// Regex flip-flop right operand: boolean rvalue (bare `m//` is `$_ =~ m//`), like `if` / `grep EXPR`.
    fn try_compile_flip_flop_rhs_expr_region(
        &mut self,
        expr: &Expr,
    ) -> Result<(usize, usize), CompileError> {
        let line = expr.line;
        let start = self.chunk.len();
        self.compile_boolean_rvalue_condition(expr)?;
        self.chunk.emit(Op::BlockReturnValue, line);
        Ok((start, self.chunk.len()))
    }

    /// Peephole optimization: if a compiled sub starts with `ShiftArray("_")`
    /// ops and `@_` is not referenced elsewhere, convert those shifts to
    /// `GetArg(n)` and mark the sub entry as `uses_stack_args = true`.
    /// This eliminates Vec allocation + string-based @_ lookup per call.
    fn peephole_stack_args(&mut self, sub_name_idx: u16, entry_ip: usize, underscore_idx: u16) {
        let ops = &self.chunk.ops;
        let end = ops.len();

        // Count leading ShiftArray("_") ops
        let mut shift_count: u8 = 0;
        let mut ip = entry_ip;
        while ip < end {
            if ops[ip] == Op::ShiftArray(underscore_idx) {
                shift_count += 1;
                ip += 1;
            } else {
                break;
            }
        }
        if shift_count == 0 {
            return;
        }

        // Check that @_ is not referenced by any other op in this sub
        let refs_underscore = |op: &Op| -> bool {
            match op {
                Op::GetArray(idx)
                | Op::SetArray(idx)
                | Op::DeclareArray(idx)
                | Op::DeclareArrayFrozen(idx)
                | Op::GetArrayElem(idx)
                | Op::SetArrayElem(idx)
                | Op::SetArrayElemKeep(idx)
                | Op::PushArray(idx)
                | Op::PopArray(idx)
                | Op::ShiftArray(idx)
                | Op::ArrayLen(idx) => *idx == underscore_idx,
                _ => false,
            }
        };

        for op in ops.iter().take(end).skip(entry_ip + shift_count as usize) {
            if refs_underscore(op) {
                return; // @_ used elsewhere, can't optimize
            }
            if matches!(op, Op::Halt | Op::ReturnValue) {
                break; // end of this sub's bytecode
            }
        }

        // Safe to convert: replace ShiftArray("_") with GetArg(n)
        for i in 0..shift_count {
            self.chunk.ops[entry_ip + i as usize] = Op::GetArg(i);
        }

        // Mark sub entry as using stack args
        for e in &mut self.chunk.sub_entries {
            if e.0 == sub_name_idx {
                e.2 = true;
            }
        }
    }

    fn emit_declare_scalar(&mut self, name_idx: u16, line: usize, frozen: bool) {
        let name = self.chunk.names[name_idx as usize].clone();
        self.register_declare(Sigil::Scalar, &name, frozen);
        if frozen {
            self.chunk.emit(Op::DeclareScalarFrozen(name_idx), line);
        } else if let Some(slot) = self.assign_scalar_slot(&name) {
            self.chunk.emit(Op::DeclareScalarSlot(slot, name_idx), line);
        } else {
            self.chunk.emit(Op::DeclareScalar(name_idx), line);
        }
    }

    fn emit_declare_array(&mut self, name_idx: u16, line: usize, frozen: bool) {
        let name = self.chunk.names[name_idx as usize].clone();
        self.register_declare(Sigil::Array, &name, frozen);
        if frozen {
            self.chunk.emit(Op::DeclareArrayFrozen(name_idx), line);
        } else {
            self.chunk.emit(Op::DeclareArray(name_idx), line);
        }
    }

    fn emit_declare_hash(&mut self, name_idx: u16, line: usize, frozen: bool) {
        let name = self.chunk.names[name_idx as usize].clone();
        self.register_declare(Sigil::Hash, &name, frozen);
        if frozen {
            self.chunk.emit(Op::DeclareHashFrozen(name_idx), line);
        } else {
            self.chunk.emit(Op::DeclareHash(name_idx), line);
        }
    }

    fn compile_var_declarations(
        &mut self,
        decls: &[VarDecl],
        line: usize,
        is_my: bool,
    ) -> Result<(), CompileError> {
        let allow_frozen = is_my;
        // List assignment: my ($a, $b) = (10, 20) — distribute elements
        if decls.len() > 1 && decls[0].initializer.is_some() {
            if decls.iter().any(|d| d.type_annotation.is_some()) {
                return Err(CompileError::Unsupported(
                    "typed my in list assignment".into(),
                ));
            }
            self.compile_expr_ctx(decls[0].initializer.as_ref().unwrap(), WantarrayCtx::List)?;
            let tmp_name = self.chunk.intern_name("__list_assign_tmp__");
            self.emit_declare_array(tmp_name, line, false);
            for (i, decl) in decls.iter().enumerate() {
                let frozen = allow_frozen && decl.frozen;
                match decl.sigil {
                    Sigil::Scalar => {
                        let name_idx = self.chunk.intern_name(&decl.name);
                        self.chunk.emit(Op::LoadInt(i as i64), line);
                        self.chunk.emit(Op::GetArrayElem(tmp_name), line);
                        self.emit_declare_scalar(name_idx, line, frozen);
                    }
                    Sigil::Array => {
                        let name_idx = self
                            .chunk
                            .intern_name(&self.qualify_stash_array_name(&decl.name));
                        self.chunk.emit(Op::GetArray(tmp_name), line);
                        self.emit_declare_array(name_idx, line, frozen);
                    }
                    Sigil::Hash => {
                        let name_idx = self.chunk.intern_name(&decl.name);
                        self.chunk.emit(Op::GetArray(tmp_name), line);
                        self.emit_declare_hash(name_idx, line, frozen);
                    }
                    Sigil::Typeglob => {
                        return Err(CompileError::Unsupported(
                            "list assignment to typeglob (my (*a, *b) = ...)".into(),
                        ));
                    }
                }
            }
        } else {
            for decl in decls {
                let frozen = allow_frozen && decl.frozen;
                match decl.sigil {
                    Sigil::Scalar => {
                        let name_idx = self.chunk.intern_name(&decl.name);
                        if let Some(init) = &decl.initializer {
                            self.compile_expr(init)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, line);
                        }
                        if let Some(ty) = decl.type_annotation {
                            if frozen {
                                return Err(CompileError::Unsupported(
                                    "typed frozen my — use `typed my` without frozen".into(),
                                ));
                            }
                            let name = self.chunk.names[name_idx as usize].clone();
                            self.register_declare(Sigil::Scalar, &name, false);
                            self.chunk
                                .emit(Op::DeclareScalarTyped(name_idx, ty.as_byte()), line);
                        } else {
                            self.emit_declare_scalar(name_idx, line, frozen);
                        }
                    }
                    Sigil::Array => {
                        let name_idx = self
                            .chunk
                            .intern_name(&self.qualify_stash_array_name(&decl.name));
                        if let Some(init) = &decl.initializer {
                            self.compile_expr_ctx(init, WantarrayCtx::List)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, line);
                        }
                        self.emit_declare_array(name_idx, line, frozen);
                    }
                    Sigil::Hash => {
                        let name_idx = self.chunk.intern_name(&decl.name);
                        if let Some(init) = &decl.initializer {
                            self.compile_expr_ctx(init, WantarrayCtx::List)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, line);
                        }
                        self.emit_declare_hash(name_idx, line, frozen);
                    }
                    Sigil::Typeglob => {
                        return Err(CompileError::Unsupported(
                            "my/our *GLOB (use tree interpreter)".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn compile_local_declarations(
        &mut self,
        decls: &[VarDecl],
        line: usize,
    ) -> Result<(), CompileError> {
        if decls.iter().any(|d| d.type_annotation.is_some()) {
            return Err(CompileError::Unsupported("typed local".into()));
        }
        if decls.len() > 1 && decls[0].initializer.is_some() {
            self.compile_expr_ctx(decls[0].initializer.as_ref().unwrap(), WantarrayCtx::List)?;
            let tmp_name = self.chunk.intern_name("__list_assign_tmp__");
            self.emit_declare_array(tmp_name, line, false);
            for (i, decl) in decls.iter().enumerate() {
                let name_idx = self.chunk.intern_name(&decl.name);
                match decl.sigil {
                    Sigil::Scalar => {
                        self.chunk.emit(Op::LoadInt(i as i64), line);
                        self.chunk.emit(Op::GetArrayElem(tmp_name), line);
                        self.chunk.emit(Op::LocalDeclareScalar(name_idx), line);
                    }
                    Sigil::Array => {
                        self.chunk.emit(Op::GetArray(tmp_name), line);
                        self.chunk.emit(Op::LocalDeclareArray(name_idx), line);
                    }
                    Sigil::Hash => {
                        self.chunk.emit(Op::GetArray(tmp_name), line);
                        self.chunk.emit(Op::LocalDeclareHash(name_idx), line);
                    }
                    Sigil::Typeglob => {
                        return Err(CompileError::Unsupported(
                            "local (*a,*b,...) with list initializer and typeglob (use tree interpreter)"
                                .into(),
                        ));
                    }
                }
            }
        } else {
            for decl in decls {
                let name_idx = self.chunk.intern_name(&decl.name);
                match decl.sigil {
                    Sigil::Scalar => {
                        if let Some(init) = &decl.initializer {
                            self.compile_expr(init)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, line);
                        }
                        self.chunk.emit(Op::LocalDeclareScalar(name_idx), line);
                    }
                    Sigil::Array => {
                        if let Some(init) = &decl.initializer {
                            self.compile_expr_ctx(init, WantarrayCtx::List)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, line);
                        }
                        self.chunk.emit(Op::LocalDeclareArray(name_idx), line);
                    }
                    Sigil::Hash => {
                        if let Some(init) = &decl.initializer {
                            self.compile_expr_ctx(init, WantarrayCtx::List)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, line);
                        }
                        self.chunk.emit(Op::LocalDeclareHash(name_idx), line);
                    }
                    Sigil::Typeglob => {
                        if let Some(init) = &decl.initializer {
                            let ExprKind::Typeglob(rhs) = &init.kind else {
                                return Err(CompileError::Unsupported(
                                    "local *GLOB = non-typeglob (use tree interpreter)".into(),
                                ));
                            };
                            let rhs_idx = self.chunk.intern_name(rhs);
                            self.chunk
                                .emit(Op::LocalDeclareTypeglob(name_idx, Some(rhs_idx)), line);
                        } else {
                            self.chunk
                                .emit(Op::LocalDeclareTypeglob(name_idx, None), line);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn compile_mysync_declarations(
        &mut self,
        decls: &[VarDecl],
        line: usize,
    ) -> Result<(), CompileError> {
        for decl in decls {
            if decl.type_annotation.is_some() {
                return Err(CompileError::Unsupported("typed mysync".into()));
            }
            match decl.sigil {
                Sigil::Typeglob => {
                    return Err(CompileError::Unsupported(
                        "`mysync` does not support typeglob variables".into(),
                    ));
                }
                Sigil::Scalar => {
                    if let Some(init) = &decl.initializer {
                        self.compile_expr(init)?;
                    } else {
                        self.chunk.emit(Op::LoadUndef, line);
                    }
                    let name_idx = self.chunk.intern_name(&decl.name);
                    self.register_declare(Sigil::Scalar, &decl.name, false);
                    self.chunk.emit(Op::DeclareMySyncScalar(name_idx), line);
                }
                Sigil::Array => {
                    let stash = self.qualify_stash_array_name(&decl.name);
                    if let Some(init) = &decl.initializer {
                        self.compile_expr_ctx(init, WantarrayCtx::List)?;
                    } else {
                        self.chunk.emit(Op::LoadUndef, line);
                    }
                    let name_idx = self.chunk.intern_name(&stash);
                    self.register_declare(Sigil::Array, &stash, false);
                    self.chunk.emit(Op::DeclareMySyncArray(name_idx), line);
                    if let Some(layer) = self.scope_stack.last_mut() {
                        layer.mysync_arrays.insert(stash);
                    }
                }
                Sigil::Hash => {
                    if let Some(init) = &decl.initializer {
                        self.compile_expr_ctx(init, WantarrayCtx::List)?;
                    } else {
                        self.chunk.emit(Op::LoadUndef, line);
                    }
                    let name_idx = self.chunk.intern_name(&decl.name);
                    self.register_declare(Sigil::Hash, &decl.name, false);
                    self.chunk.emit(Op::DeclareMySyncHash(name_idx), line);
                    if let Some(layer) = self.scope_stack.last_mut() {
                        layer.mysync_hashes.insert(decl.name.clone());
                    }
                }
            }
        }
        Ok(())
    }

    /// `local $h{k} = …` / `local $SIG{__WARN__}` — not plain [`StmtKind::Local`] declarations.
    fn compile_local_expr(
        &mut self,
        target: &Expr,
        initializer: Option<&Expr>,
        line: usize,
    ) -> Result<(), CompileError> {
        match &target.kind {
            ExprKind::HashElement { hash, key } => {
                self.check_strict_hash_access(hash, line)?;
                self.check_hash_mutable(hash, line)?;
                let hash_idx = self.chunk.intern_name(hash);
                if let Some(init) = initializer {
                    self.compile_expr(init)?;
                } else {
                    self.chunk.emit(Op::LoadUndef, line);
                }
                self.compile_expr(key)?;
                self.chunk.emit(Op::LocalDeclareHashElement(hash_idx), line);
                Ok(())
            }
            ExprKind::ArrayElement { array, index } => {
                self.check_strict_array_access(array, line)?;
                let q = self.qualify_stash_array_name(array);
                self.check_array_mutable(&q, line)?;
                let arr_idx = self.chunk.intern_name(&q);
                if let Some(init) = initializer {
                    self.compile_expr(init)?;
                } else {
                    self.chunk.emit(Op::LoadUndef, line);
                }
                self.compile_expr(index)?;
                self.chunk.emit(Op::LocalDeclareArrayElement(arr_idx), line);
                Ok(())
            }
            ExprKind::Typeglob(name) => {
                let lhs_idx = self.chunk.intern_name(name);
                if let Some(init) = initializer {
                    let ExprKind::Typeglob(rhs) = &init.kind else {
                        return Err(CompileError::Unsupported(
                            "local *GLOB = non-typeglob (use tree interpreter)".into(),
                        ));
                    };
                    let rhs_idx = self.chunk.intern_name(rhs);
                    self.chunk
                        .emit(Op::LocalDeclareTypeglob(lhs_idx, Some(rhs_idx)), line);
                } else {
                    self.chunk
                        .emit(Op::LocalDeclareTypeglob(lhs_idx, None), line);
                }
                Ok(())
            }
            ExprKind::Deref {
                expr,
                kind: Sigil::Typeglob,
            } => {
                if let Some(init) = initializer {
                    let ExprKind::Typeglob(rhs) = &init.kind else {
                        return Err(CompileError::Unsupported(
                            "local *GLOB = non-typeglob (use tree interpreter)".into(),
                        ));
                    };
                    let rhs_idx = self.chunk.intern_name(rhs);
                    self.compile_expr(expr)?;
                    self.chunk
                        .emit(Op::LocalDeclareTypeglobDynamic(Some(rhs_idx)), line);
                } else {
                    self.compile_expr(expr)?;
                    self.chunk.emit(Op::LocalDeclareTypeglobDynamic(None), line);
                }
                Ok(())
            }
            ExprKind::TypeglobExpr(expr) => {
                if let Some(init) = initializer {
                    let ExprKind::Typeglob(rhs) = &init.kind else {
                        return Err(CompileError::Unsupported(
                            "local *GLOB = non-typeglob (use tree interpreter)".into(),
                        ));
                    };
                    let rhs_idx = self.chunk.intern_name(rhs);
                    self.compile_expr(expr)?;
                    self.chunk
                        .emit(Op::LocalDeclareTypeglobDynamic(Some(rhs_idx)), line);
                } else {
                    self.compile_expr(expr)?;
                    self.chunk.emit(Op::LocalDeclareTypeglobDynamic(None), line);
                }
                Ok(())
            }
            ExprKind::ScalarVar(name) => {
                let name_idx = self.chunk.intern_name(name);
                if let Some(init) = initializer {
                    self.compile_expr(init)?;
                } else {
                    self.chunk.emit(Op::LoadUndef, line);
                }
                self.chunk.emit(Op::LocalDeclareScalar(name_idx), line);
                Ok(())
            }
            ExprKind::ArrayVar(name) => {
                self.check_strict_array_access(name, line)?;
                let q = self.qualify_stash_array_name(name);
                let name_idx = self.chunk.intern_name(&q);
                if let Some(init) = initializer {
                    self.compile_expr_ctx(init, WantarrayCtx::List)?;
                } else {
                    self.chunk.emit(Op::LoadUndef, line);
                }
                self.chunk.emit(Op::LocalDeclareArray(name_idx), line);
                Ok(())
            }
            ExprKind::HashVar(name) => {
                let name_idx = self.chunk.intern_name(name);
                if let Some(init) = initializer {
                    self.compile_expr_ctx(init, WantarrayCtx::List)?;
                } else {
                    self.chunk.emit(Op::LoadUndef, line);
                }
                self.chunk.emit(Op::LocalDeclareHash(name_idx), line);
                Ok(())
            }
            _ => Err(CompileError::Unsupported(
                "local on this lvalue (use tree interpreter)".into(),
            )),
        }
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), CompileError> {
        // A `LABEL:` on a statement binds the label to the IP of the first op emitted for that
        // statement, so that `goto LABEL` can jump to the effective start of execution.
        if let Some(lbl) = &stmt.label {
            self.record_stmt_label(lbl);
        }
        let line = stmt.line;
        match &stmt.kind {
            StmtKind::FormatDecl { name, lines } => {
                let idx = self.chunk.add_format_decl(name.clone(), lines.clone());
                self.chunk.emit(Op::FormatDecl(idx), line);
            }
            StmtKind::Expression(expr) => {
                self.compile_expr_ctx(expr, WantarrayCtx::Void)?;
                self.chunk.emit(Op::Pop, line);
            }
            StmtKind::Local(decls) => self.compile_local_declarations(decls, line)?,
            StmtKind::LocalExpr {
                target,
                initializer,
            } => {
                self.compile_local_expr(target, initializer.as_ref(), line)?;
            }
            StmtKind::MySync(decls) => self.compile_mysync_declarations(decls, line)?,
            StmtKind::My(decls) => self.compile_var_declarations(decls, line, true)?,
            StmtKind::Our(decls) => self.compile_var_declarations(decls, line, false)?,
            StmtKind::If {
                condition,
                body,
                elsifs,
                else_block,
            } => {
                self.compile_boolean_rvalue_condition(condition)?;
                let jump_else = self.chunk.emit(Op::JumpIfFalse(0), line);
                self.compile_block(body)?;
                let mut end_jumps = vec![self.chunk.emit(Op::Jump(0), line)];
                self.chunk.patch_jump_here(jump_else);

                for (cond, blk) in elsifs {
                    self.compile_boolean_rvalue_condition(cond)?;
                    let j = self.chunk.emit(Op::JumpIfFalse(0), cond.line);
                    self.compile_block(blk)?;
                    end_jumps.push(self.chunk.emit(Op::Jump(0), cond.line));
                    self.chunk.patch_jump_here(j);
                }

                if let Some(eb) = else_block {
                    self.compile_block(eb)?;
                }
                for j in end_jumps {
                    self.chunk.patch_jump_here(j);
                }
            }
            StmtKind::Unless {
                condition,
                body,
                else_block,
            } => {
                self.compile_boolean_rvalue_condition(condition)?;
                let jump_else = self.chunk.emit(Op::JumpIfTrue(0), line);
                self.compile_block(body)?;
                if let Some(eb) = else_block {
                    let end_j = self.chunk.emit(Op::Jump(0), line);
                    self.chunk.patch_jump_here(jump_else);
                    self.compile_block(eb)?;
                    self.chunk.patch_jump_here(end_j);
                } else {
                    self.chunk.patch_jump_here(jump_else);
                }
            }
            StmtKind::While {
                condition,
                body,
                label,
                continue_block,
            } => {
                let loop_start = self.chunk.len();
                self.compile_boolean_rvalue_condition(condition)?;
                let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);
                let body_start_ip = self.chunk.len();

                self.loop_stack.push(LoopCtx {
                    label: label.clone(),
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    body_start_ip,
                    break_jumps: vec![],
                    continue_jumps: vec![],
                });
                self.compile_block_no_frame(body)?;
                // `continue { ... }` runs both on normal fall-through from the body and on
                // `next` (continue_jumps). `last` still bypasses it via break_jumps.
                let continue_entry = self.chunk.len();
                let cont_jumps =
                    std::mem::take(&mut self.loop_stack.last_mut().expect("loop").continue_jumps);
                for j in cont_jumps {
                    self.chunk.patch_jump_to(j, continue_entry);
                }
                if let Some(cb) = continue_block {
                    self.compile_block_no_frame(cb)?;
                }
                self.chunk.emit(Op::Jump(loop_start), line);
                self.chunk.patch_jump_here(exit_jump);
                let ctx = self.loop_stack.pop().expect("loop");
                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
            }
            StmtKind::Until {
                condition,
                body,
                label,
                continue_block,
            } => {
                let loop_start = self.chunk.len();
                self.compile_boolean_rvalue_condition(condition)?;
                let exit_jump = self.chunk.emit(Op::JumpIfTrue(0), line);
                let body_start_ip = self.chunk.len();

                self.loop_stack.push(LoopCtx {
                    label: label.clone(),
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    body_start_ip,
                    break_jumps: vec![],
                    continue_jumps: vec![],
                });
                self.compile_block_no_frame(body)?;
                let continue_entry = self.chunk.len();
                let cont_jumps =
                    std::mem::take(&mut self.loop_stack.last_mut().expect("loop").continue_jumps);
                for j in cont_jumps {
                    self.chunk.patch_jump_to(j, continue_entry);
                }
                if let Some(cb) = continue_block {
                    self.compile_block_no_frame(cb)?;
                }
                self.chunk.emit(Op::Jump(loop_start), line);
                self.chunk.patch_jump_here(exit_jump);
                let ctx = self.loop_stack.pop().expect("loop");
                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
                label,
                continue_block,
            } => {
                // When the enclosing scope uses scalar slots, skip PushFrame/PopFrame for the
                // C-style `for` so loop variables (`$i`) and outer variables (`$sum`) share the
                // same runtime frame and are both accessible via O(1) slot ops.  The compiler's
                // scope layer still tracks `my` declarations for name resolution; only the runtime
                // frame push is elided.
                let outer_has_slots = self.scope_stack.last().is_some_and(|l| l.use_slots);
                if !outer_has_slots {
                    self.emit_push_frame(line);
                }
                if let Some(init) = init {
                    self.compile_statement(init)?;
                }
                let loop_start = self.chunk.len();
                let cond_exit = if let Some(cond) = condition {
                    self.compile_boolean_rvalue_condition(cond)?;
                    Some(self.chunk.emit(Op::JumpIfFalse(0), line))
                } else {
                    None
                };
                let body_start_ip = self.chunk.len();

                self.loop_stack.push(LoopCtx {
                    label: label.clone(),
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    body_start_ip,
                    break_jumps: cond_exit.into_iter().collect(),
                    continue_jumps: vec![],
                });
                self.compile_block_no_frame(body)?;

                let continue_entry = self.chunk.len();
                let cont_jumps =
                    std::mem::take(&mut self.loop_stack.last_mut().expect("loop").continue_jumps);
                for j in cont_jumps {
                    self.chunk.patch_jump_to(j, continue_entry);
                }
                if let Some(cb) = continue_block {
                    self.compile_block_no_frame(cb)?;
                }
                if let Some(step) = step {
                    self.compile_expr(step)?;
                    self.chunk.emit(Op::Pop, line);
                }
                self.chunk.emit(Op::Jump(loop_start), line);

                let ctx = self.loop_stack.pop().expect("loop");
                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
                if !outer_has_slots {
                    self.emit_pop_frame(line);
                }
            }
            StmtKind::Foreach {
                var,
                list,
                body,
                label,
                continue_block,
            } => {
                // PushFrame isolates __foreach_list__ / __foreach_i__ from outer/nested loops.
                self.emit_push_frame(line);
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let list_name = self.chunk.intern_name("__foreach_list__");
                self.chunk.emit(Op::DeclareArray(list_name), line);

                // Counter and loop variable go in slots so the hot per-iteration ops
                // (`GetScalarSlot` / `PreIncSlot`) skip the linear frame-scalar scan.
                // We cache the slot indices before compiling the body so that any
                // nested foreach / inner `my` that reallocates the same name in the
                // shared scope layer cannot poison our post-body increment op.
                let counter_name = self.chunk.intern_name("__foreach_i__");
                self.chunk.emit(Op::LoadInt(0), line);
                let counter_slot_opt = self.assign_scalar_slot("__foreach_i__");
                if let Some(slot) = counter_slot_opt {
                    self.chunk
                        .emit(Op::DeclareScalarSlot(slot, counter_name), line);
                } else {
                    self.chunk.emit(Op::DeclareScalar(counter_name), line);
                }

                let var_name = self.chunk.intern_name(var);
                self.register_declare(Sigil::Scalar, var, false);
                self.chunk.emit(Op::LoadUndef, line);
                // `$_` is the global topic — keep it in the frame scalars so bareword calls
                // and `print`/`printf` arg-defaulting still see it via the usual special-var
                // path. Slotting it breaks callees that read `$_` across the call boundary.
                let var_slot_opt = if var == "_" {
                    None
                } else {
                    self.assign_scalar_slot(var)
                };
                if let Some(slot) = var_slot_opt {
                    self.chunk.emit(Op::DeclareScalarSlot(slot, var_name), line);
                } else {
                    self.chunk.emit(Op::DeclareScalar(var_name), line);
                }

                let loop_start = self.chunk.len();
                // Check: $i < scalar @list
                if let Some(s) = counter_slot_opt {
                    self.chunk.emit(Op::GetScalarSlot(s), line);
                } else {
                    self.emit_get_scalar(counter_name, line, None);
                }
                self.chunk.emit(Op::ArrayLen(list_name), line);
                self.chunk.emit(Op::NumLt, line);
                let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);

                // $var = $list[$i]
                if let Some(s) = counter_slot_opt {
                    self.chunk.emit(Op::GetScalarSlot(s), line);
                } else {
                    self.emit_get_scalar(counter_name, line, None);
                }
                self.chunk.emit(Op::GetArrayElem(list_name), line);
                if let Some(s) = var_slot_opt {
                    self.chunk.emit(Op::SetScalarSlot(s), line);
                } else {
                    self.emit_set_scalar(var_name, line, None);
                }
                let body_start_ip = self.chunk.len();

                self.loop_stack.push(LoopCtx {
                    label: label.clone(),
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    body_start_ip,
                    break_jumps: vec![],
                    continue_jumps: vec![],
                });
                self.compile_block_no_frame(body)?;
                // `continue { ... }` on foreach runs after each iteration body (and on `next`),
                // before the iterator increment.
                let step_ip = self.chunk.len();
                let cont_jumps =
                    std::mem::take(&mut self.loop_stack.last_mut().expect("loop").continue_jumps);
                for j in cont_jumps {
                    self.chunk.patch_jump_to(j, step_ip);
                }
                if let Some(cb) = continue_block {
                    self.compile_block_no_frame(cb)?;
                }

                // $i++ — use the cached slot directly. The scope layer's scalar_slots
                // map may now point `__foreach_i__` at a nested foreach's slot (if any),
                // so we must NOT re-resolve through `emit_pre_inc(counter_name)`.
                if let Some(s) = counter_slot_opt {
                    self.chunk.emit(Op::PreIncSlot(s), line);
                } else {
                    self.emit_pre_inc(counter_name, line, None);
                }
                self.chunk.emit(Op::Pop, line);
                self.chunk.emit(Op::Jump(loop_start), line);

                self.chunk.patch_jump_here(exit_jump);
                let ctx = self.loop_stack.pop().expect("loop");
                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
                self.emit_pop_frame(line);
            }
            StmtKind::DoWhile { body, condition } => {
                let loop_start = self.chunk.len();
                self.loop_stack.push(LoopCtx {
                    label: None,
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    body_start_ip: loop_start,
                    break_jumps: vec![],
                    continue_jumps: vec![],
                });
                self.compile_block_no_frame(body)?;
                let cont_jumps =
                    std::mem::take(&mut self.loop_stack.last_mut().expect("loop").continue_jumps);
                for j in cont_jumps {
                    self.chunk.patch_jump_to(j, loop_start);
                }
                self.compile_boolean_rvalue_condition(condition)?;
                let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);
                self.chunk.emit(Op::Jump(loop_start), line);
                self.chunk.patch_jump_here(exit_jump);
                let ctx = self.loop_stack.pop().expect("loop");
                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
            }
            StmtKind::Goto { target } => {
                // `goto LABEL` where LABEL is a compile-time-known bareword/string: emit a
                // forward `Jump(0)` and record it for patching when the current goto-scope
                // exits. `goto &sub` and `goto $expr` (dynamic target) stay Unsupported.
                if !self.try_emit_goto_label(target, line) {
                    return Err(CompileError::Unsupported(
                        "goto with dynamic or sub-ref target".into(),
                    ));
                }
            }
            StmtKind::Continue(block) => {
                // A bare `continue { ... }` statement (no attached loop) is a parser edge case:
                // the tree interpreter just runs the block (`Interpreter::exec_block_smart`).
                // Match that in the VM path so the fallback is unneeded.
                for stmt in block {
                    self.compile_statement(stmt)?;
                }
            }
            StmtKind::Return(val) => {
                if let Some(expr) = val {
                    self.compile_expr(expr)?;
                    self.chunk.emit(Op::ReturnValue, line);
                } else {
                    self.chunk.emit(Op::Return, line);
                }
            }
            StmtKind::Last(label) | StmtKind::Next(label) => {
                // Resolve the target loop via `self.loop_stack` — walk from the innermost loop
                // outward, picking the first one that matches the label (or the innermost if
                // `last`/`next` has no label). Emit `(frame_depth - entry_frame_depth)`
                // `PopFrame` ops first so any intervening block / if-body frames are torn down
                // before the jump. `try { }` crossings still bail to tree (see `entry_try_depth`).
                let is_last = matches!(&stmt.kind, StmtKind::Last(_));
                // Search the loop stack (innermost → outermost) for a matching label.
                let (target_idx, entry_frame_depth, entry_try_depth) = {
                    let mut found: Option<(usize, usize, usize)> = None;
                    for (i, lc) in self.loop_stack.iter().enumerate().rev() {
                        let matches = match (label.as_deref(), lc.label.as_deref()) {
                            (None, _) => true, // unlabeled `last`/`next` targets innermost loop
                            (Some(l), Some(lcl)) => l == lcl,
                            (Some(_), None) => false,
                        };
                        if matches {
                            found = Some((i, lc.entry_frame_depth, lc.entry_try_depth));
                            break;
                        }
                    }
                    found.ok_or_else(|| {
                        CompileError::Unsupported(if label.is_some() {
                            format!(
                                "last/next with label `{}` — no matching loop in compile scope",
                                label.as_deref().unwrap_or("")
                            )
                        } else {
                            "last/next outside any loop (tree interpreter)".into()
                        })
                    })?
                };
                // Cross-try-frame flow control is not modeled in bytecode.
                if self.try_depth != entry_try_depth {
                    return Err(CompileError::Unsupported(
                        "last/next across try { } frame (tree interpreter)".into(),
                    ));
                }
                // Tear down any scope frames pushed since the loop was entered.
                let frames_to_pop = self.frame_depth.saturating_sub(entry_frame_depth);
                for _ in 0..frames_to_pop {
                    // Emit the `PopFrame` op without decrementing `self.frame_depth` — the
                    // compiler is still emitting code for the enclosing block which will later
                    // emit its own `PopFrame`; we only need the runtime pop here for the
                    // `last`/`next` control path.
                    self.chunk.emit(Op::PopFrame, line);
                }
                let j = self.chunk.emit(Op::Jump(0), line);
                let slot = &mut self.loop_stack[target_idx];
                if is_last {
                    slot.break_jumps.push(j);
                } else {
                    slot.continue_jumps.push(j);
                }
            }
            StmtKind::Redo(label) => {
                let (target_idx, entry_frame_depth, entry_try_depth) = {
                    let mut found: Option<(usize, usize, usize)> = None;
                    for (i, lc) in self.loop_stack.iter().enumerate().rev() {
                        let matches = match (label.as_deref(), lc.label.as_deref()) {
                            (None, _) => true,
                            (Some(l), Some(lcl)) => l == lcl,
                            (Some(_), None) => false,
                        };
                        if matches {
                            found = Some((i, lc.entry_frame_depth, lc.entry_try_depth));
                            break;
                        }
                    }
                    found.ok_or_else(|| {
                        CompileError::Unsupported(if label.is_some() {
                            format!(
                                "redo with label `{}` — no matching loop in compile scope",
                                label.as_deref().unwrap_or("")
                            )
                        } else {
                            "redo outside any loop (tree interpreter)".into()
                        })
                    })?
                };
                if self.try_depth != entry_try_depth {
                    return Err(CompileError::Unsupported(
                        "redo across try { } frame (tree interpreter)".into(),
                    ));
                }
                let frames_to_pop = self.frame_depth.saturating_sub(entry_frame_depth);
                for _ in 0..frames_to_pop {
                    self.chunk.emit(Op::PopFrame, line);
                }
                let body_start = self.loop_stack[target_idx].body_start_ip;
                let j = self.chunk.emit(Op::Jump(0), line);
                self.chunk.patch_jump_to(j, body_start);
            }
            StmtKind::Block(block) => {
                self.chunk.emit(Op::PushFrame, line);
                self.compile_block_inner(block)?;
                self.chunk.emit(Op::PopFrame, line);
            }
            StmtKind::Package { name } => {
                self.current_package = name.clone();
                let val_idx = self.chunk.add_constant(PerlValue::string(name.clone()));
                let name_idx = self.chunk.intern_name("__PACKAGE__");
                self.chunk.emit(Op::LoadConst(val_idx), line);
                self.emit_set_scalar(name_idx, line, None);
            }
            StmtKind::SubDecl {
                name,
                params,
                body,
                prototype,
            } => {
                let idx = self.chunk.runtime_sub_decls.len();
                if idx > u16::MAX as usize {
                    return Err(CompileError::Unsupported(
                        "too many runtime sub declarations in one chunk".into(),
                    ));
                }
                self.chunk.runtime_sub_decls.push(RuntimeSubDecl {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    prototype: prototype.clone(),
                });
                self.chunk.emit(Op::RuntimeSubDecl(idx as u16), line);
            }
            StmtKind::StructDecl { def } => {
                if self.chunk.struct_defs.iter().any(|d| d.name == def.name) {
                    return Err(CompileError::Unsupported(format!(
                        "duplicate struct `{}`",
                        def.name
                    )));
                }
                self.chunk.struct_defs.push(def.clone());
            }
            StmtKind::TryCatch {
                try_block,
                catch_var,
                catch_block,
                finally_block,
            } => {
                let catch_var_idx = self.chunk.intern_name(catch_var);
                let try_push_idx = self.chunk.emit(
                    Op::TryPush {
                        catch_ip: 0,
                        finally_ip: None,
                        after_ip: 0,
                        catch_var_idx,
                    },
                    line,
                );
                self.chunk.emit(Op::PushFrame, line);
                if self.program_last_stmt_takes_value {
                    self.emit_block_value(try_block, line)?;
                } else {
                    self.compile_block_inner(try_block)?;
                }
                self.chunk.emit(Op::PopFrame, line);
                self.chunk.emit(Op::TryContinueNormal, line);

                let catch_start = self.chunk.len();
                self.chunk.patch_try_push_catch(try_push_idx, catch_start);

                self.chunk.emit(Op::CatchReceive(catch_var_idx), line);
                if self.program_last_stmt_takes_value {
                    self.emit_block_value(catch_block, line)?;
                } else {
                    self.compile_block_inner(catch_block)?;
                }
                self.chunk.emit(Op::PopFrame, line);
                self.chunk.emit(Op::TryContinueNormal, line);

                if let Some(fin) = finally_block {
                    let finally_start = self.chunk.len();
                    self.chunk
                        .patch_try_push_finally(try_push_idx, Some(finally_start));
                    self.chunk.emit(Op::PushFrame, line);
                    self.compile_block_inner(fin)?;
                    self.chunk.emit(Op::PopFrame, line);
                    self.chunk.emit(Op::TryFinallyEnd, line);
                }
                let merge = self.chunk.len();
                self.chunk.patch_try_push_after(try_push_idx, merge);
            }
            StmtKind::EvalTimeout { timeout, body } => {
                let idx = self
                    .chunk
                    .add_eval_timeout_entry(timeout.clone(), body.clone());
                self.chunk.emit(Op::EvalTimeout(idx), line);
            }
            StmtKind::Given { topic, body } => {
                let idx = self.chunk.add_given_entry(topic.clone(), body.clone());
                self.chunk.emit(Op::Given(idx), line);
            }
            StmtKind::When { .. } | StmtKind::DefaultCase { .. } => {
                return Err(CompileError::Unsupported(
                    "`when` / `default` only valid inside `given`".into(),
                ));
            }
            StmtKind::Tie {
                target,
                class,
                args,
            } => {
                self.compile_expr(class)?;
                for a in args {
                    self.compile_expr(a)?;
                }
                let (kind, name_idx) = match target {
                    TieTarget::Scalar(s) => (0u8, self.chunk.intern_name(s)),
                    TieTarget::Array(a) => (1u8, self.chunk.intern_name(a)),
                    TieTarget::Hash(h) => (2u8, self.chunk.intern_name(h)),
                };
                let argc = (1 + args.len()) as u8;
                self.chunk.emit(
                    Op::Tie {
                        target_kind: kind,
                        name_idx,
                        argc,
                    },
                    line,
                );
            }
            StmtKind::UseOverload { pairs } => {
                let idx = self.chunk.add_use_overload(pairs.clone());
                self.chunk.emit(Op::UseOverload(idx), line);
            }
            StmtKind::UsePerlVersion { .. }
            | StmtKind::Use { .. }
            | StmtKind::No { .. }
            | StmtKind::Begin(_)
            | StmtKind::UnitCheck(_)
            | StmtKind::Check(_)
            | StmtKind::Init(_)
            | StmtKind::End(_)
            | StmtKind::Empty => {
                // No-ops or handled elsewhere
            }
        }
        Ok(())
    }

    /// Returns true if the block contains a Return statement (directly, not in nested subs).
    fn block_has_return(block: &Block) -> bool {
        for stmt in block {
            match &stmt.kind {
                StmtKind::Return(_) => return true,
                StmtKind::If {
                    body,
                    elsifs,
                    else_block,
                    ..
                } => {
                    if Self::block_has_return(body) {
                        return true;
                    }
                    for (_, blk) in elsifs {
                        if Self::block_has_return(blk) {
                            return true;
                        }
                    }
                    if let Some(eb) = else_block {
                        if Self::block_has_return(eb) {
                            return true;
                        }
                    }
                }
                StmtKind::Unless {
                    body, else_block, ..
                } => {
                    if Self::block_has_return(body) {
                        return true;
                    }
                    if let Some(eb) = else_block {
                        if Self::block_has_return(eb) {
                            return true;
                        }
                    }
                }
                StmtKind::While { body, .. }
                | StmtKind::Until { body, .. }
                | StmtKind::Foreach { body, .. } => {
                    if Self::block_has_return(body) {
                        return true;
                    }
                }
                StmtKind::For { body, .. } => {
                    if Self::block_has_return(body) {
                        return true;
                    }
                }
                StmtKind::Block(blk) => {
                    if Self::block_has_return(blk) {
                        return true;
                    }
                }
                StmtKind::DoWhile { body, .. } => {
                    if Self::block_has_return(body) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn compile_block(&mut self, block: &Block) -> Result<(), CompileError> {
        if Self::block_has_return(block) {
            self.compile_block_inner(block)?;
        } else if self.scope_stack.last().is_some_and(|l| l.use_slots) {
            // When scalar slots are active, skip PushFrame/PopFrame so slot indices keep
            // addressing the same runtime frame. New `my` decls still get fresh slot indices.
            self.compile_block_inner(block)?;
        } else {
            self.push_scope_layer();
            self.chunk.emit(Op::PushFrame, 0);
            self.compile_block_inner(block)?;
            self.chunk.emit(Op::PopFrame, 0);
            self.pop_scope_layer();
        }
        Ok(())
    }

    fn compile_block_inner(&mut self, block: &Block) -> Result<(), CompileError> {
        for stmt in block {
            self.compile_statement(stmt)?;
        }
        Ok(())
    }

    /// Compile a block that leaves its last expression's value on the stack.
    /// Used for if/unless as the last statement (implicit return).
    fn emit_block_value(&mut self, block: &Block, line: usize) -> Result<(), CompileError> {
        if block.is_empty() {
            self.chunk.emit(Op::LoadUndef, line);
            return Ok(());
        }
        let last = &block[block.len() - 1];
        if let StmtKind::Expression(expr) = &last.kind {
            if block.len() == 1 {
                self.compile_expr(expr)?;
                return Ok(());
            }
        }
        for stmt in block {
            self.compile_statement(stmt)?;
        }
        self.chunk.emit(Op::LoadUndef, line);
        Ok(())
    }

    /// Compile a subroutine body so the return value matches Perl: the last statement's value is
    /// returned when it is an expression or a trailing `if`/`unless` (same shape as the main
    /// program's last-statement value rule). Otherwise falls through with `undef` after the last
    /// statement unless it already executed `return`.
    fn emit_subroutine_body_return(&mut self, body: &Block) -> Result<(), CompileError> {
        if body.is_empty() {
            self.chunk.emit(Op::LoadUndef, 0);
            self.chunk.emit(Op::ReturnValue, 0);
            return Ok(());
        }
        let last_idx = body.len() - 1;
        let last = &body[last_idx];
        match &last.kind {
            StmtKind::Return(_) => {
                for stmt in body {
                    self.compile_statement(stmt)?;
                }
            }
            StmtKind::Expression(expr) => {
                for stmt in &body[..last_idx] {
                    self.compile_statement(stmt)?;
                }
                self.compile_expr(expr)?;
                self.chunk.emit(Op::ReturnValue, last.line);
            }
            StmtKind::If {
                condition,
                body: if_body,
                elsifs,
                else_block,
            } => {
                for stmt in &body[..last_idx] {
                    self.compile_statement(stmt)?;
                }
                self.compile_boolean_rvalue_condition(condition)?;
                let j0 = self.chunk.emit(Op::JumpIfFalse(0), last.line);
                self.emit_block_value(if_body, last.line)?;
                let mut ends = vec![self.chunk.emit(Op::Jump(0), last.line)];
                self.chunk.patch_jump_here(j0);
                for (c, blk) in elsifs {
                    self.compile_boolean_rvalue_condition(c)?;
                    let j = self.chunk.emit(Op::JumpIfFalse(0), c.line);
                    self.emit_block_value(blk, c.line)?;
                    ends.push(self.chunk.emit(Op::Jump(0), c.line));
                    self.chunk.patch_jump_here(j);
                }
                if let Some(eb) = else_block {
                    self.emit_block_value(eb, last.line)?;
                } else {
                    self.chunk.emit(Op::LoadUndef, last.line);
                }
                for j in ends {
                    self.chunk.patch_jump_here(j);
                }
                self.chunk.emit(Op::ReturnValue, last.line);
            }
            StmtKind::Unless {
                condition,
                body: unless_body,
                else_block,
            } => {
                for stmt in &body[..last_idx] {
                    self.compile_statement(stmt)?;
                }
                self.compile_boolean_rvalue_condition(condition)?;
                let j0 = self.chunk.emit(Op::JumpIfFalse(0), last.line);
                if let Some(eb) = else_block {
                    self.emit_block_value(eb, last.line)?;
                } else {
                    self.chunk.emit(Op::LoadUndef, last.line);
                }
                let end = self.chunk.emit(Op::Jump(0), last.line);
                self.chunk.patch_jump_here(j0);
                self.emit_block_value(unless_body, last.line)?;
                self.chunk.patch_jump_here(end);
                self.chunk.emit(Op::ReturnValue, last.line);
            }
            _ => {
                for stmt in body {
                    self.compile_statement(stmt)?;
                }
                self.chunk.emit(Op::LoadUndef, 0);
                self.chunk.emit(Op::ReturnValue, 0);
            }
        }
        Ok(())
    }

    /// Compile a loop body as a sequence of statements. `last`/`next` (including those nested
    /// inside `if`/`unless`/block statements) are handled by `compile_statement` via the
    /// [`Compiler::loop_stack`] — the innermost loop frame owns their break/continue patches.
    fn compile_block_no_frame(&mut self, block: &Block) -> Result<(), CompileError> {
        for stmt in block {
            self.compile_statement(stmt)?;
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        self.compile_expr_ctx(expr, WantarrayCtx::Scalar)
    }

    fn compile_expr_ctx(&mut self, root: &Expr, ctx: WantarrayCtx) -> Result<(), CompileError> {
        let line = root.line;
        match &root.kind {
            ExprKind::Integer(n) => {
                self.emit_op(Op::LoadInt(*n), line, Some(root));
            }
            ExprKind::Float(f) => {
                self.emit_op(Op::LoadFloat(*f), line, Some(root));
            }
            ExprKind::String(s) => {
                let idx = self.chunk.add_constant(PerlValue::string(s.clone()));
                self.emit_op(Op::LoadConst(idx), line, Some(root));
            }
            ExprKind::Bareword(name) => {
                // `BAREWORD` as an rvalue: run-time lookup via `Op::BarewordRvalue` — if a sub
                // with this name exists at run time, call it nullary; otherwise push the name
                // as a string. Mirrors the tree-walker's `ExprKind::Bareword` eval path.
                let idx = self.chunk.intern_name(name);
                self.emit_op(Op::BarewordRvalue(idx), line, Some(root));
            }
            ExprKind::Undef => {
                self.emit_op(Op::LoadUndef, line, Some(root));
            }
            ExprKind::MagicConst(crate::ast::MagicConstKind::File) => {
                let idx = self
                    .chunk
                    .add_constant(PerlValue::string(self.source_file.clone()));
                self.emit_op(Op::LoadConst(idx), line, Some(root));
            }
            ExprKind::MagicConst(crate::ast::MagicConstKind::Line) => {
                let idx = self
                    .chunk
                    .add_constant(PerlValue::integer(root.line as i64));
                self.emit_op(Op::LoadConst(idx), line, Some(root));
            }
            ExprKind::ScalarVar(name) => {
                self.check_strict_scalar_access(name, line)?;
                let idx = self.chunk.intern_name(name);
                self.emit_get_scalar(idx, line, Some(root));
            }
            ExprKind::ArrayVar(name) => {
                self.check_strict_array_access(name, line)?;
                let idx = self.chunk.intern_name(&self.qualify_stash_array_name(name));
                if ctx == WantarrayCtx::List {
                    self.emit_op(Op::GetArray(idx), line, Some(root));
                } else {
                    self.emit_op(Op::ArrayLen(idx), line, Some(root));
                }
            }
            ExprKind::HashVar(name) => {
                self.check_strict_hash_access(name, line)?;
                let idx = self.chunk.intern_name(name);
                self.emit_op(Op::GetHash(idx), line, Some(root));
                if ctx != WantarrayCtx::List {
                    self.emit_op(Op::ValueScalarContext, line, Some(root));
                }
            }
            ExprKind::Typeglob(name) => {
                let idx = self.chunk.add_constant(PerlValue::string(name.clone()));
                self.emit_op(Op::LoadConst(idx), line, Some(root));
            }
            ExprKind::TypeglobExpr(expr) => {
                self.compile_expr(expr)?;
                self.emit_op(Op::LoadDynamicTypeglob, line, Some(root));
            }
            ExprKind::ArrayElement { array, index } => {
                self.check_strict_array_access(array, line)?;
                let idx = self
                    .chunk
                    .intern_name(&self.qualify_stash_array_name(array));
                self.compile_expr(index)?;
                self.emit_op(Op::GetArrayElem(idx), line, Some(root));
            }
            ExprKind::HashElement { hash, key } => {
                self.check_strict_hash_access(hash, line)?;
                let idx = self.chunk.intern_name(hash);
                self.compile_expr(key)?;
                self.emit_op(Op::GetHashElem(idx), line, Some(root));
            }
            ExprKind::ArraySlice { array, indices } => {
                let arr_idx = self
                    .chunk
                    .intern_name(&self.qualify_stash_array_name(array));
                if indices.is_empty() {
                    self.emit_op(Op::MakeArray(0), line, Some(root));
                } else {
                    for (ix, index_expr) in indices.iter().enumerate() {
                        self.compile_array_slice_index_expr(index_expr)?;
                        self.emit_op(Op::ArraySlicePart(arr_idx), line, Some(root));
                        if ix > 0 {
                            self.emit_op(Op::ArrayConcatTwo, line, Some(root));
                        }
                    }
                }
            }
            ExprKind::HashSlice { hash, keys } => {
                let hash_idx = self.chunk.intern_name(hash);
                for key_expr in keys {
                    self.compile_expr(key_expr)?;
                    self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                }
                self.emit_op(Op::MakeArray(keys.len() as u16), line, Some(root));
            }
            ExprKind::HashSliceDeref { container, keys } => {
                self.compile_expr(container)?;
                for key_expr in keys {
                    self.compile_expr(key_expr)?;
                }
                self.emit_op(Op::HashSliceDeref(keys.len() as u16), line, Some(root));
            }
            ExprKind::AnonymousListSlice { source, indices } => {
                if indices.is_empty() {
                    self.compile_expr_ctx(source, WantarrayCtx::List)?;
                    self.emit_op(Op::MakeArray(0), line, Some(root));
                } else {
                    self.compile_expr_ctx(source, WantarrayCtx::List)?;
                    for index_expr in indices {
                        self.compile_array_slice_index_expr(index_expr)?;
                    }
                    self.emit_op(Op::ArrowArraySlice(indices.len() as u16), line, Some(root));
                }
                if ctx != WantarrayCtx::List {
                    self.emit_op(Op::ListSliceToScalar, line, Some(root));
                }
            }

            // ── Operators ──
            ExprKind::BinOp { left, op, right } => {
                // Short-circuit operators
                match op {
                    BinOp::LogAnd | BinOp::LogAndWord => {
                        if matches!(left.kind, ExprKind::Regex(..)) {
                            self.compile_boolean_rvalue_condition(left)?;
                            self.emit_op(Op::RegexBoolToScalar, line, Some(root));
                        } else {
                            self.compile_expr(left)?;
                        }
                        let j = self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root));
                        self.emit_op(Op::Pop, line, Some(root));
                        if matches!(right.kind, ExprKind::Regex(..)) {
                            self.compile_boolean_rvalue_condition(right)?;
                            self.emit_op(Op::RegexBoolToScalar, line, Some(root));
                        } else {
                            self.compile_expr(right)?;
                        }
                        self.chunk.patch_jump_here(j);
                        return Ok(());
                    }
                    BinOp::LogOr | BinOp::LogOrWord => {
                        if matches!(left.kind, ExprKind::Regex(..)) {
                            self.compile_boolean_rvalue_condition(left)?;
                            self.emit_op(Op::RegexBoolToScalar, line, Some(root));
                        } else {
                            self.compile_expr(left)?;
                        }
                        let j = self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root));
                        self.emit_op(Op::Pop, line, Some(root));
                        if matches!(right.kind, ExprKind::Regex(..)) {
                            self.compile_boolean_rvalue_condition(right)?;
                            self.emit_op(Op::RegexBoolToScalar, line, Some(root));
                        } else {
                            self.compile_expr(right)?;
                        }
                        self.chunk.patch_jump_here(j);
                        return Ok(());
                    }
                    BinOp::DefinedOr => {
                        self.compile_expr(left)?;
                        let j = self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root));
                        self.emit_op(Op::Pop, line, Some(root));
                        self.compile_expr(right)?;
                        self.chunk.patch_jump_here(j);
                        return Ok(());
                    }
                    BinOp::BindMatch => {
                        self.compile_expr(left)?;
                        self.compile_expr(right)?;
                        self.emit_op(Op::RegexMatchDyn(false), line, Some(root));
                        return Ok(());
                    }
                    BinOp::BindNotMatch => {
                        self.compile_expr(left)?;
                        self.compile_expr(right)?;
                        self.emit_op(Op::RegexMatchDyn(true), line, Some(root));
                        return Ok(());
                    }
                    _ => {}
                }

                self.compile_expr(left)?;
                self.compile_expr(right)?;
                let op_code = match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Mod => Op::Mod,
                    BinOp::Pow => Op::Pow,
                    BinOp::Concat => Op::Concat,
                    BinOp::NumEq => Op::NumEq,
                    BinOp::NumNe => Op::NumNe,
                    BinOp::NumLt => Op::NumLt,
                    BinOp::NumGt => Op::NumGt,
                    BinOp::NumLe => Op::NumLe,
                    BinOp::NumGe => Op::NumGe,
                    BinOp::Spaceship => Op::Spaceship,
                    BinOp::StrEq => Op::StrEq,
                    BinOp::StrNe => Op::StrNe,
                    BinOp::StrLt => Op::StrLt,
                    BinOp::StrGt => Op::StrGt,
                    BinOp::StrLe => Op::StrLe,
                    BinOp::StrGe => Op::StrGe,
                    BinOp::StrCmp => Op::StrCmp,
                    BinOp::BitAnd => Op::BitAnd,
                    BinOp::BitOr => Op::BitOr,
                    BinOp::BitXor => Op::BitXor,
                    BinOp::ShiftLeft => Op::Shl,
                    BinOp::ShiftRight => Op::Shr,
                    // Short-circuit and regex bind handled above
                    BinOp::LogAnd
                    | BinOp::LogOr
                    | BinOp::DefinedOr
                    | BinOp::LogAndWord
                    | BinOp::LogOrWord
                    | BinOp::BindMatch
                    | BinOp::BindNotMatch => unreachable!(),
                };
                self.emit_op(op_code, line, Some(root));
            }

            ExprKind::UnaryOp { op, expr } => match op {
                UnaryOp::PreIncrement => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
                        self.check_scalar_mutable(name, line)?;
                        let idx = self.chunk.intern_name(name);
                        self.emit_pre_inc(idx, line, Some(root));
                    } else if let ExprKind::ArrayElement { array, index } = &expr.kind {
                        if self.is_mysync_array(array) {
                            return Err(CompileError::Unsupported(
                                "mysync array element update (tree interpreter)".into(),
                            ));
                        }
                        let q = self.qualify_stash_array_name(array);
                        self.check_array_mutable(&q, line)?;
                        let arr_idx = self.chunk.intern_name(&q);
                        self.compile_expr(index)?;
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::GetArrayElem(arr_idx), line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Add, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::SetArrayElem(arr_idx), line, Some(root));
                    } else if let ExprKind::ArraySlice { array, indices } = &expr.kind {
                        if self.is_mysync_array(array) {
                            return Err(CompileError::Unsupported(
                                "mysync array element update (tree interpreter)".into(),
                            ));
                        }
                        self.check_strict_array_access(array, line)?;
                        let q = self.qualify_stash_array_name(array);
                        self.check_array_mutable(&q, line)?;
                        let arr_idx = self.chunk.intern_name(&q);
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(
                            Op::NamedArraySliceIncDec(0, arr_idx, indices.len() as u16),
                            line,
                            Some(root),
                        );
                    } else if let ExprKind::HashElement { hash, key } = &expr.kind {
                        if self.is_mysync_hash(hash) {
                            return Err(CompileError::Unsupported(
                                "mysync hash element update (tree interpreter)".into(),
                            ));
                        }
                        self.check_hash_mutable(hash, line)?;
                        let hash_idx = self.chunk.intern_name(hash);
                        self.compile_expr(key)?;
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Add, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::SetHashElem(hash_idx), line, Some(root));
                    } else if let ExprKind::HashSlice { hash, keys } = &expr.kind {
                        if self.is_mysync_hash(hash) {
                            return Err(CompileError::Unsupported(
                                "mysync hash element update (tree interpreter)".into(),
                            ));
                        }
                        self.check_hash_mutable(hash, line)?;
                        let hash_idx = self.chunk.intern_name(hash);
                        if hash_slice_needs_slice_ops(keys) {
                            for hk in keys {
                                self.compile_expr(hk)?;
                            }
                            self.emit_op(
                                Op::NamedHashSliceIncDec(0, hash_idx, keys.len() as u16),
                                line,
                                Some(root),
                            );
                            return Ok(());
                        }
                        let hk = &keys[0];
                        self.compile_expr(hk)?;
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Add, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::SetHashElem(hash_idx), line, Some(root));
                    } else if let ExprKind::ArrowDeref {
                        expr,
                        index,
                        kind: DerefKind::Array,
                    } = &expr.kind
                    {
                        if let ExprKind::List(indices) = &index.kind {
                            // Multi-index `++@$aref[i1,i2,...]` — delegates to VM slice inc-dec.
                            self.compile_arrow_array_base_expr(expr)?;
                            for ix in indices {
                                self.compile_array_slice_index_expr(ix)?;
                            }
                            self.emit_op(
                                Op::ArrowArraySliceIncDec(0, indices.len() as u16),
                                line,
                                Some(root),
                            );
                            return Ok(());
                        }
                        self.compile_arrow_array_base_expr(expr)?;
                        self.compile_array_slice_index_expr(index)?;
                        self.emit_op(Op::ArrowArraySliceIncDec(0, 1), line, Some(root));
                    } else if let ExprKind::AnonymousListSlice { source, indices } = &expr.kind {
                        if let ExprKind::Deref {
                            expr: inner,
                            kind: Sigil::Array,
                        } = &source.kind
                        {
                            self.compile_arrow_array_base_expr(inner)?;
                            for ix in indices {
                                self.compile_array_slice_index_expr(ix)?;
                            }
                            self.emit_op(
                                Op::ArrowArraySliceIncDec(0, indices.len() as u16),
                                line,
                                Some(root),
                            );
                            return Ok(());
                        }
                    } else if let ExprKind::ArrowDeref {
                        expr,
                        index,
                        kind: DerefKind::Hash,
                    } = &expr.kind
                    {
                        self.compile_arrow_hash_base_expr(expr)?;
                        self.compile_expr(index)?;
                        self.emit_op(Op::Dup2, line, Some(root));
                        self.emit_op(Op::ArrowHash, line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Add, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Pop, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::SetArrowHashKeep, line, Some(root));
                    } else if let ExprKind::HashSliceDeref { container, keys } = &expr.kind {
                        if hash_slice_needs_slice_ops(keys) {
                            // Multi-key: matches tree-walker's generic PreIncrement fallback
                            // (list → int → ±1 → slice assign). Dedicated op in VM delegates to
                            // Interpreter::hash_slice_deref_inc_dec.
                            self.compile_expr(container)?;
                            for hk in keys {
                                self.compile_expr(hk)?;
                            }
                            self.emit_op(
                                Op::HashSliceDerefIncDec(0, keys.len() as u16),
                                line,
                                Some(root),
                            );
                            return Ok(());
                        }
                        let hk = &keys[0];
                        self.compile_expr(container)?;
                        self.compile_expr(hk)?;
                        self.emit_op(Op::Dup2, line, Some(root));
                        self.emit_op(Op::ArrowHash, line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Add, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Pop, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::SetArrowHashKeep, line, Some(root));
                    } else if let ExprKind::Deref {
                        expr,
                        kind: Sigil::Scalar,
                    } = &expr.kind
                    {
                        self.compile_expr(expr)?;
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::SymbolicDeref(0), line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Add, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::SetSymbolicScalarRefKeep, line, Some(root));
                    } else if let ExprKind::Deref { kind, .. } = &expr.kind {
                        // `++@{…}` / `++%{…}` (and `++@$r` / `++%$r`) are invalid in Perl 5.
                        // Emit a runtime error directly so `try_vm_execute` doesn't fall back to
                        // the tree interpreter just to produce the same error.
                        self.emit_aggregate_symbolic_inc_dec_error(*kind, true, true, line, root)?;
                    } else {
                        return Err(CompileError::Unsupported("PreInc on non-scalar".into()));
                    }
                }
                UnaryOp::PreDecrement => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
                        self.check_scalar_mutable(name, line)?;
                        let idx = self.chunk.intern_name(name);
                        self.emit_pre_dec(idx, line, Some(root));
                    } else if let ExprKind::ArrayElement { array, index } = &expr.kind {
                        if self.is_mysync_array(array) {
                            return Err(CompileError::Unsupported(
                                "mysync array element update (tree interpreter)".into(),
                            ));
                        }
                        let q = self.qualify_stash_array_name(array);
                        self.check_array_mutable(&q, line)?;
                        let arr_idx = self.chunk.intern_name(&q);
                        self.compile_expr(index)?;
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::GetArrayElem(arr_idx), line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Sub, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::SetArrayElem(arr_idx), line, Some(root));
                    } else if let ExprKind::ArraySlice { array, indices } = &expr.kind {
                        if self.is_mysync_array(array) {
                            return Err(CompileError::Unsupported(
                                "mysync array element update (tree interpreter)".into(),
                            ));
                        }
                        self.check_strict_array_access(array, line)?;
                        let q = self.qualify_stash_array_name(array);
                        self.check_array_mutable(&q, line)?;
                        let arr_idx = self.chunk.intern_name(&q);
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(
                            Op::NamedArraySliceIncDec(1, arr_idx, indices.len() as u16),
                            line,
                            Some(root),
                        );
                    } else if let ExprKind::HashElement { hash, key } = &expr.kind {
                        if self.is_mysync_hash(hash) {
                            return Err(CompileError::Unsupported(
                                "mysync hash element update (tree interpreter)".into(),
                            ));
                        }
                        self.check_hash_mutable(hash, line)?;
                        let hash_idx = self.chunk.intern_name(hash);
                        self.compile_expr(key)?;
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Sub, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::SetHashElem(hash_idx), line, Some(root));
                    } else if let ExprKind::HashSlice { hash, keys } = &expr.kind {
                        if self.is_mysync_hash(hash) {
                            return Err(CompileError::Unsupported(
                                "mysync hash element update (tree interpreter)".into(),
                            ));
                        }
                        self.check_hash_mutable(hash, line)?;
                        let hash_idx = self.chunk.intern_name(hash);
                        if hash_slice_needs_slice_ops(keys) {
                            for hk in keys {
                                self.compile_expr(hk)?;
                            }
                            self.emit_op(
                                Op::NamedHashSliceIncDec(1, hash_idx, keys.len() as u16),
                                line,
                                Some(root),
                            );
                            return Ok(());
                        }
                        let hk = &keys[0];
                        self.compile_expr(hk)?;
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Sub, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::SetHashElem(hash_idx), line, Some(root));
                    } else if let ExprKind::ArrowDeref {
                        expr,
                        index,
                        kind: DerefKind::Array,
                    } = &expr.kind
                    {
                        if let ExprKind::List(indices) = &index.kind {
                            self.compile_arrow_array_base_expr(expr)?;
                            for ix in indices {
                                self.compile_array_slice_index_expr(ix)?;
                            }
                            self.emit_op(
                                Op::ArrowArraySliceIncDec(1, indices.len() as u16),
                                line,
                                Some(root),
                            );
                            return Ok(());
                        }
                        self.compile_arrow_array_base_expr(expr)?;
                        self.compile_array_slice_index_expr(index)?;
                        self.emit_op(Op::ArrowArraySliceIncDec(1, 1), line, Some(root));
                    } else if let ExprKind::AnonymousListSlice { source, indices } = &expr.kind {
                        if let ExprKind::Deref {
                            expr: inner,
                            kind: Sigil::Array,
                        } = &source.kind
                        {
                            self.compile_arrow_array_base_expr(inner)?;
                            for ix in indices {
                                self.compile_array_slice_index_expr(ix)?;
                            }
                            self.emit_op(
                                Op::ArrowArraySliceIncDec(1, indices.len() as u16),
                                line,
                                Some(root),
                            );
                            return Ok(());
                        }
                    } else if let ExprKind::ArrowDeref {
                        expr,
                        index,
                        kind: DerefKind::Hash,
                    } = &expr.kind
                    {
                        self.compile_arrow_hash_base_expr(expr)?;
                        self.compile_expr(index)?;
                        self.emit_op(Op::Dup2, line, Some(root));
                        self.emit_op(Op::ArrowHash, line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Sub, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Pop, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::SetArrowHashKeep, line, Some(root));
                    } else if let ExprKind::HashSliceDeref { container, keys } = &expr.kind {
                        if hash_slice_needs_slice_ops(keys) {
                            self.compile_expr(container)?;
                            for hk in keys {
                                self.compile_expr(hk)?;
                            }
                            self.emit_op(
                                Op::HashSliceDerefIncDec(1, keys.len() as u16),
                                line,
                                Some(root),
                            );
                            return Ok(());
                        }
                        let hk = &keys[0];
                        self.compile_expr(container)?;
                        self.compile_expr(hk)?;
                        self.emit_op(Op::Dup2, line, Some(root));
                        self.emit_op(Op::ArrowHash, line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Sub, line, Some(root));
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::Pop, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::Rot, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::SetArrowHashKeep, line, Some(root));
                    } else if let ExprKind::Deref {
                        expr,
                        kind: Sigil::Scalar,
                    } = &expr.kind
                    {
                        self.compile_expr(expr)?;
                        self.emit_op(Op::Dup, line, Some(root));
                        self.emit_op(Op::SymbolicDeref(0), line, Some(root));
                        self.emit_op(Op::LoadInt(1), line, Some(root));
                        self.emit_op(Op::Sub, line, Some(root));
                        self.emit_op(Op::Swap, line, Some(root));
                        self.emit_op(Op::SetSymbolicScalarRefKeep, line, Some(root));
                    } else if let ExprKind::Deref { kind, .. } = &expr.kind {
                        self.emit_aggregate_symbolic_inc_dec_error(*kind, true, false, line, root)?;
                    } else {
                        return Err(CompileError::Unsupported("PreDec on non-scalar".into()));
                    }
                }
                UnaryOp::Ref => {
                    self.compile_expr(expr)?;
                    self.emit_op(Op::MakeScalarRef, line, Some(root));
                }
                _ => match op {
                    UnaryOp::LogNot | UnaryOp::LogNotWord => {
                        if matches!(expr.kind, ExprKind::Regex(..)) {
                            self.compile_boolean_rvalue_condition(expr)?;
                        } else {
                            self.compile_expr(expr)?;
                        }
                        self.emit_op(Op::LogNot, line, Some(root));
                    }
                    UnaryOp::Negate => {
                        self.compile_expr(expr)?;
                        self.emit_op(Op::Negate, line, Some(root));
                    }
                    UnaryOp::BitNot => {
                        self.compile_expr(expr)?;
                        self.emit_op(Op::BitNot, line, Some(root));
                    }
                    _ => unreachable!(),
                },
            },
            ExprKind::PostfixOp { expr, op } => {
                if let ExprKind::ScalarVar(name) = &expr.kind {
                    self.check_scalar_mutable(name, line)?;
                    let idx = self.chunk.intern_name(name);
                    match op {
                        PostfixOp::Increment => {
                            self.emit_post_inc(idx, line, Some(root));
                        }
                        PostfixOp::Decrement => {
                            self.emit_post_dec(idx, line, Some(root));
                        }
                    }
                } else if let ExprKind::ArrayElement { array, index } = &expr.kind {
                    if self.is_mysync_array(array) {
                        return Err(CompileError::Unsupported(
                            "mysync array element update (tree interpreter)".into(),
                        ));
                    }
                    let q = self.qualify_stash_array_name(array);
                    self.check_array_mutable(&q, line)?;
                    let arr_idx = self.chunk.intern_name(&q);
                    self.compile_expr(index)?;
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::GetArrayElem(arr_idx), line, Some(root));
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::LoadInt(1), line, Some(root));
                    match op {
                        PostfixOp::Increment => {
                            self.emit_op(Op::Add, line, Some(root));
                        }
                        PostfixOp::Decrement => {
                            self.emit_op(Op::Sub, line, Some(root));
                        }
                    }
                    self.emit_op(Op::Rot, line, Some(root));
                    self.emit_op(Op::SetArrayElem(arr_idx), line, Some(root));
                } else if let ExprKind::ArraySlice { array, indices } = &expr.kind {
                    if self.is_mysync_array(array) {
                        return Err(CompileError::Unsupported(
                            "mysync array element update (tree interpreter)".into(),
                        ));
                    }
                    self.check_strict_array_access(array, line)?;
                    let q = self.qualify_stash_array_name(array);
                    self.check_array_mutable(&q, line)?;
                    let arr_idx = self.chunk.intern_name(&q);
                    let kind_byte: u8 = match op {
                        PostfixOp::Increment => 2,
                        PostfixOp::Decrement => 3,
                    };
                    for ix in indices {
                        self.compile_array_slice_index_expr(ix)?;
                    }
                    self.emit_op(
                        Op::NamedArraySliceIncDec(kind_byte, arr_idx, indices.len() as u16),
                        line,
                        Some(root),
                    );
                } else if let ExprKind::HashElement { hash, key } = &expr.kind {
                    if self.is_mysync_hash(hash) {
                        return Err(CompileError::Unsupported(
                            "mysync hash element update (tree interpreter)".into(),
                        ));
                    }
                    self.check_hash_mutable(hash, line)?;
                    let hash_idx = self.chunk.intern_name(hash);
                    self.compile_expr(key)?;
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::LoadInt(1), line, Some(root));
                    match op {
                        PostfixOp::Increment => {
                            self.emit_op(Op::Add, line, Some(root));
                        }
                        PostfixOp::Decrement => {
                            self.emit_op(Op::Sub, line, Some(root));
                        }
                    }
                    self.emit_op(Op::Rot, line, Some(root));
                    self.emit_op(Op::SetHashElem(hash_idx), line, Some(root));
                } else if let ExprKind::HashSlice { hash, keys } = &expr.kind {
                    if self.is_mysync_hash(hash) {
                        return Err(CompileError::Unsupported(
                            "mysync hash element update (tree interpreter)".into(),
                        ));
                    }
                    self.check_hash_mutable(hash, line)?;
                    let hash_idx = self.chunk.intern_name(hash);
                    if hash_slice_needs_slice_ops(keys) {
                        let kind_byte: u8 = match op {
                            PostfixOp::Increment => 2,
                            PostfixOp::Decrement => 3,
                        };
                        for hk in keys {
                            self.compile_expr(hk)?;
                        }
                        self.emit_op(
                            Op::NamedHashSliceIncDec(kind_byte, hash_idx, keys.len() as u16),
                            line,
                            Some(root),
                        );
                        return Ok(());
                    }
                    let hk = &keys[0];
                    self.compile_expr(hk)?;
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::LoadInt(1), line, Some(root));
                    match op {
                        PostfixOp::Increment => {
                            self.emit_op(Op::Add, line, Some(root));
                        }
                        PostfixOp::Decrement => {
                            self.emit_op(Op::Sub, line, Some(root));
                        }
                    }
                    self.emit_op(Op::Rot, line, Some(root));
                    self.emit_op(Op::SetHashElem(hash_idx), line, Some(root));
                } else if let ExprKind::ArrowDeref {
                    expr: inner,
                    index,
                    kind: DerefKind::Array,
                } = &expr.kind
                {
                    if let ExprKind::List(indices) = &index.kind {
                        let kind_byte: u8 = match op {
                            PostfixOp::Increment => 2,
                            PostfixOp::Decrement => 3,
                        };
                        self.compile_arrow_array_base_expr(inner)?;
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(
                            Op::ArrowArraySliceIncDec(kind_byte, indices.len() as u16),
                            line,
                            Some(root),
                        );
                        return Ok(());
                    }
                    self.compile_arrow_array_base_expr(inner)?;
                    self.compile_array_slice_index_expr(index)?;
                    let kind_byte: u8 = match op {
                        PostfixOp::Increment => 2,
                        PostfixOp::Decrement => 3,
                    };
                    self.emit_op(Op::ArrowArraySliceIncDec(kind_byte, 1), line, Some(root));
                } else if let ExprKind::AnonymousListSlice { source, indices } = &expr.kind {
                    let ExprKind::Deref {
                        expr: inner,
                        kind: Sigil::Array,
                    } = &source.kind
                    else {
                        return Err(CompileError::Unsupported(
                            "PostfixOp on list slice (non-array deref)".into(),
                        ));
                    };
                    if indices.is_empty() {
                        return Err(CompileError::Unsupported(
                            "postfix ++/-- on empty list slice (internal)".into(),
                        ));
                    }
                    let kind_byte: u8 = match op {
                        PostfixOp::Increment => 2,
                        PostfixOp::Decrement => 3,
                    };
                    self.compile_arrow_array_base_expr(inner)?;
                    if indices.len() > 1 {
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(
                            Op::ArrowArraySliceIncDec(kind_byte, indices.len() as u16),
                            line,
                            Some(root),
                        );
                    } else {
                        self.compile_array_slice_index_expr(&indices[0])?;
                        self.emit_op(Op::ArrowArraySliceIncDec(kind_byte, 1), line, Some(root));
                    }
                } else if let ExprKind::ArrowDeref {
                    expr: inner,
                    index,
                    kind: DerefKind::Hash,
                } = &expr.kind
                {
                    self.compile_arrow_hash_base_expr(inner)?;
                    self.compile_expr(index)?;
                    let b = match op {
                        PostfixOp::Increment => 0u8,
                        PostfixOp::Decrement => 1u8,
                    };
                    self.emit_op(Op::ArrowHashPostfix(b), line, Some(root));
                } else if let ExprKind::HashSliceDeref { container, keys } = &expr.kind {
                    if hash_slice_needs_slice_ops(keys) {
                        // Multi-key postfix ++/--: matches tree-walker's generic PostfixOp fallback
                        // (reads slice list, assigns scalar back, returns old list).
                        let kind_byte: u8 = match op {
                            PostfixOp::Increment => 2,
                            PostfixOp::Decrement => 3,
                        };
                        self.compile_expr(container)?;
                        for hk in keys {
                            self.compile_expr(hk)?;
                        }
                        self.emit_op(
                            Op::HashSliceDerefIncDec(kind_byte, keys.len() as u16),
                            line,
                            Some(root),
                        );
                        return Ok(());
                    }
                    let hk = &keys[0];
                    self.compile_expr(container)?;
                    self.compile_expr(hk)?;
                    let b = match op {
                        PostfixOp::Increment => 0u8,
                        PostfixOp::Decrement => 1u8,
                    };
                    self.emit_op(Op::ArrowHashPostfix(b), line, Some(root));
                } else if let ExprKind::Deref {
                    expr,
                    kind: Sigil::Scalar,
                } = &expr.kind
                {
                    self.compile_expr(expr)?;
                    let b = match op {
                        PostfixOp::Increment => 0u8,
                        PostfixOp::Decrement => 1u8,
                    };
                    self.emit_op(Op::SymbolicScalarRefPostfix(b), line, Some(root));
                } else if let ExprKind::Deref { kind, .. } = &expr.kind {
                    let is_inc = matches!(op, PostfixOp::Increment);
                    self.emit_aggregate_symbolic_inc_dec_error(*kind, false, is_inc, line, root)?;
                } else {
                    return Err(CompileError::Unsupported("PostfixOp on non-scalar".into()));
                }
            }

            ExprKind::Assign { target, value } => {
                if let (ExprKind::Typeglob(lhs), ExprKind::Typeglob(rhs)) =
                    (&target.kind, &value.kind)
                {
                    let lhs_idx = self.chunk.intern_name(lhs);
                    let rhs_idx = self.chunk.intern_name(rhs);
                    self.emit_op(Op::CopyTypeglobSlots(lhs_idx, rhs_idx), line, Some(root));
                    self.compile_expr(value)?;
                    return Ok(());
                }
                if let ExprKind::TypeglobExpr(expr) = &target.kind {
                    if let ExprKind::Typeglob(rhs) = &value.kind {
                        self.compile_expr(expr)?;
                        let rhs_idx = self.chunk.intern_name(rhs);
                        self.emit_op(Op::CopyTypeglobSlotsDynamicLhs(rhs_idx), line, Some(root));
                        self.compile_expr(value)?;
                        return Ok(());
                    }
                    self.compile_expr(expr)?;
                    self.compile_expr(value)?;
                    self.emit_op(Op::TypeglobAssignFromValueDynamic, line, Some(root));
                    return Ok(());
                }
                // Braced `*{EXPR}` parses as `Deref { kind: Typeglob }` (same VM lowering as `TypeglobExpr`).
                if let ExprKind::Deref {
                    expr,
                    kind: Sigil::Typeglob,
                } = &target.kind
                {
                    if let ExprKind::Typeglob(rhs) = &value.kind {
                        self.compile_expr(expr)?;
                        let rhs_idx = self.chunk.intern_name(rhs);
                        self.emit_op(Op::CopyTypeglobSlotsDynamicLhs(rhs_idx), line, Some(root));
                        self.compile_expr(value)?;
                        return Ok(());
                    }
                    self.compile_expr(expr)?;
                    self.compile_expr(value)?;
                    self.emit_op(Op::TypeglobAssignFromValueDynamic, line, Some(root));
                    return Ok(());
                }
                if let ExprKind::ArrowDeref {
                    expr,
                    index,
                    kind: DerefKind::Array,
                } = &target.kind
                {
                    if let ExprKind::List(indices) = &index.kind {
                        if let ExprKind::Deref {
                            expr: inner,
                            kind: Sigil::Array,
                        } = &expr.kind
                        {
                            if let ExprKind::List(vals) = &value.kind {
                                if !indices.is_empty() && indices.len() == vals.len() {
                                    for (idx_e, val_e) in indices.iter().zip(vals.iter()) {
                                        self.compile_expr(val_e)?;
                                        self.compile_expr(inner)?;
                                        self.compile_expr(idx_e)?;
                                        self.emit_op(Op::SetArrowArray, line, Some(root));
                                    }
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
                // Fuse `$x = $x OP $y` / `$x = $x + 1` into slot ops when possible.
                if let ExprKind::ScalarVar(tgt_name) = &target.kind {
                    if let Some(dst_slot) = self.scalar_slot(tgt_name) {
                        if let ExprKind::BinOp { left, op, right } = &value.kind {
                            if let ExprKind::ScalarVar(lv) = &left.kind {
                                if lv == tgt_name {
                                    // $x = $x + SCALAR_VAR → AddAssignSlotSlot etc.
                                    if let ExprKind::ScalarVar(rv) = &right.kind {
                                        if let Some(src_slot) = self.scalar_slot(rv) {
                                            let fused = match op {
                                                BinOp::Add => {
                                                    Some(Op::AddAssignSlotSlot(dst_slot, src_slot))
                                                }
                                                BinOp::Sub => {
                                                    Some(Op::SubAssignSlotSlot(dst_slot, src_slot))
                                                }
                                                BinOp::Mul => {
                                                    Some(Op::MulAssignSlotSlot(dst_slot, src_slot))
                                                }
                                                _ => None,
                                            };
                                            if let Some(fop) = fused {
                                                self.emit_op(fop, line, Some(root));
                                                return Ok(());
                                            }
                                        }
                                    }
                                    // $x = $x + 1 → PreIncSlot, $x = $x - 1 → PreDecSlot
                                    if let ExprKind::Integer(1) = &right.kind {
                                        match op {
                                            BinOp::Add => {
                                                self.emit_op(
                                                    Op::PreIncSlot(dst_slot),
                                                    line,
                                                    Some(root),
                                                );
                                                return Ok(());
                                            }
                                            BinOp::Sub => {
                                                self.emit_op(
                                                    Op::PreDecSlot(dst_slot),
                                                    line,
                                                    Some(root),
                                                );
                                                return Ok(());
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                self.compile_expr_ctx(value, assign_rhs_wantarray(target))?;
                self.compile_assign(target, line, true, Some(root))?;
            }
            ExprKind::CompoundAssign { target, op, value } => {
                if let ExprKind::ScalarVar(name) = &target.kind {
                    self.check_scalar_mutable(name, line)?;
                    let idx = self.chunk.intern_name(name);
                    // Fast path: `.=` on scalar → in-place append (no clone)
                    if *op == BinOp::Concat {
                        self.compile_expr(value)?;
                        if let Some(slot) = self.scalar_slot(name) {
                            self.emit_op(Op::ConcatAppendSlot(slot), line, Some(root));
                        } else {
                            self.emit_op(Op::ConcatAppend(idx), line, Some(root));
                        }
                        return Ok(());
                    }
                    // Fused slot+slot arithmetic: $slot_a += $slot_b (no stack traffic)
                    if let Some(dst_slot) = self.scalar_slot(name) {
                        if let ExprKind::ScalarVar(rhs_name) = &value.kind {
                            if let Some(src_slot) = self.scalar_slot(rhs_name) {
                                let fused = match op {
                                    BinOp::Add => Some(Op::AddAssignSlotSlot(dst_slot, src_slot)),
                                    BinOp::Sub => Some(Op::SubAssignSlotSlot(dst_slot, src_slot)),
                                    BinOp::Mul => Some(Op::MulAssignSlotSlot(dst_slot, src_slot)),
                                    _ => None,
                                };
                                if let Some(fop) = fused {
                                    self.emit_op(fop, line, Some(root));
                                    return Ok(());
                                }
                            }
                        }
                    }
                    if *op == BinOp::DefinedOr {
                        // `$x //=` — short-circuit when LHS is defined (see `ExprKind::CompoundAssign` in interpreter).
                        self.emit_get_scalar(idx, line, Some(root));
                        let j_def = self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root));
                        self.compile_expr(value)?;
                        self.emit_set_scalar_keep(idx, line, Some(root));
                        let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                        self.chunk.patch_jump_here(j_def);
                        self.chunk.patch_jump_here(j_end);
                        return Ok(());
                    }
                    if *op == BinOp::LogOr {
                        // `$x ||=` — short-circuit when LHS is true.
                        self.emit_get_scalar(idx, line, Some(root));
                        let j_true = self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root));
                        self.compile_expr(value)?;
                        self.emit_set_scalar_keep(idx, line, Some(root));
                        let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                        self.chunk.patch_jump_here(j_true);
                        self.chunk.patch_jump_here(j_end);
                        return Ok(());
                    }
                    if *op == BinOp::LogAnd {
                        // `$x &&=` — short-circuit when LHS is false.
                        self.emit_get_scalar(idx, line, Some(root));
                        let j = self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root));
                        self.compile_expr(value)?;
                        self.emit_set_scalar_keep(idx, line, Some(root));
                        let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                        self.chunk.patch_jump_here(j);
                        self.chunk.patch_jump_here(j_end);
                        return Ok(());
                    }
                    if let Some(op_b) = scalar_compound_op_to_byte(*op) {
                        // Slot-aware path: `my $x` inside a sub body lives in a local slot.
                        // `Op::ScalarCompoundAssign` is name-based and routes through
                        // `scope.atomic_mutate(name)`, which bypasses slots — so `$s += 5`
                        // inside a sub silently updates a different (name-based) slot and
                        // leaves the real `$s` untouched (issue surfaces when strict_vars was
                        // previously masking this via tree fallback). For slot lexicals, emit
                        // the read-modify-write sequence against the slot instead.
                        if let Some(slot) = self.scalar_slot(name) {
                            let vm_op = binop_to_vm_op(*op).ok_or_else(|| {
                                CompileError::Unsupported("CompoundAssign op (slot)".into())
                            })?;
                            self.emit_op(Op::GetScalarSlot(slot), line, Some(root));
                            self.compile_expr(value)?;
                            self.emit_op(vm_op, line, Some(root));
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::SetScalarSlot(slot), line, Some(root));
                            return Ok(());
                        }
                        self.compile_expr(value)?;
                        self.emit_op(
                            Op::ScalarCompoundAssign {
                                name_idx: idx,
                                op: op_b,
                            },
                            line,
                            Some(root),
                        );
                    } else {
                        return Err(CompileError::Unsupported("CompoundAssign op".into()));
                    }
                } else if let ExprKind::ArrayElement { array, index } = &target.kind {
                    if self.is_mysync_array(array) {
                        return Err(CompileError::Unsupported(
                            "mysync array element update (tree interpreter)".into(),
                        ));
                    }
                    let q = self.qualify_stash_array_name(array);
                    self.check_array_mutable(&q, line)?;
                    let arr_idx = self.chunk.intern_name(&q);
                    match op {
                        BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd => {
                            self.compile_expr(index)?;
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::GetArrayElem(arr_idx), line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetArrayElemKeep(arr_idx), line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        _ => {
                            let vm_op = binop_to_vm_op(*op).ok_or_else(|| {
                                CompileError::Unsupported("CompoundAssign op".into())
                            })?;
                            self.compile_expr(index)?;
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::GetArrayElem(arr_idx), line, Some(root));
                            self.compile_expr(value)?;
                            self.emit_op(vm_op, line, Some(root));
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::Rot, line, Some(root));
                            self.emit_op(Op::SetArrayElem(arr_idx), line, Some(root));
                        }
                    }
                } else if let ExprKind::HashElement { hash, key } = &target.kind {
                    if self.is_mysync_hash(hash) {
                        return Err(CompileError::Unsupported(
                            "mysync hash element update (tree interpreter)".into(),
                        ));
                    }
                    self.check_hash_mutable(hash, line)?;
                    let hash_idx = self.chunk.intern_name(hash);
                    match op {
                        BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd => {
                            self.compile_expr(key)?;
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetHashElemKeep(hash_idx), line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        _ => {
                            let vm_op = binop_to_vm_op(*op).ok_or_else(|| {
                                CompileError::Unsupported("CompoundAssign op".into())
                            })?;
                            self.compile_expr(key)?;
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                            self.compile_expr(value)?;
                            self.emit_op(vm_op, line, Some(root));
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::Rot, line, Some(root));
                            self.emit_op(Op::SetHashElem(hash_idx), line, Some(root));
                        }
                    }
                } else if let ExprKind::Deref {
                    expr,
                    kind: Sigil::Scalar,
                } = &target.kind
                {
                    match op {
                        BinOp::DefinedOr => {
                            // `$$r //=` — unlike binary `//`, no `Pop` after `JumpIfDefinedKeep`
                            // (the ref must stay under the deref); `Swap` before set (ref on TOS).
                            self.compile_expr(expr)?;
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::SymbolicDeref(0), line, Some(root));
                            let j_def = self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root));
                            self.compile_expr(value)?;
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetSymbolicScalarRefKeep, line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j_def);
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        BinOp::LogOr => {
                            // `$$r ||=` — same idea as `//=`: no `Pop` after `JumpIfTrueKeep`.
                            self.compile_expr(expr)?;
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::SymbolicDeref(0), line, Some(root));
                            let j_true = self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root));
                            self.compile_expr(value)?;
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetSymbolicScalarRefKeep, line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j_true);
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        BinOp::LogAnd => {
                            // `$$r &&=` — no `Pop` after `JumpIfFalseKeep` (ref under LHS).
                            self.compile_expr(expr)?;
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::SymbolicDeref(0), line, Some(root));
                            let j = self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root));
                            self.compile_expr(value)?;
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetSymbolicScalarRefKeep, line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        _ => {
                            let vm_op = binop_to_vm_op(*op).ok_or_else(|| {
                                CompileError::Unsupported("CompoundAssign op".into())
                            })?;
                            self.compile_expr(expr)?;
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::SymbolicDeref(0), line, Some(root));
                            self.compile_expr(value)?;
                            self.emit_op(vm_op, line, Some(root));
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetSymbolicScalarRef, line, Some(root));
                        }
                    }
                } else if let ExprKind::ArrowDeref {
                    expr,
                    index,
                    kind: DerefKind::Hash,
                } = &target.kind
                {
                    match op {
                        BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd => {
                            self.compile_arrow_hash_base_expr(expr)?;
                            self.compile_expr(index)?;
                            self.emit_op(Op::Dup2, line, Some(root));
                            self.emit_op(Op::ArrowHash, line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Rot, line, Some(root));
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetArrowHashKeep, line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            // Stack: ref, key, cur — leave `cur` as the expression value.
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        _ => {
                            let vm_op = binop_to_vm_op(*op).ok_or_else(|| {
                                CompileError::Unsupported("CompoundAssign op".into())
                            })?;
                            self.compile_arrow_hash_base_expr(expr)?;
                            self.compile_expr(index)?;
                            self.emit_op(Op::Dup2, line, Some(root));
                            self.emit_op(Op::ArrowHash, line, Some(root));
                            self.compile_expr(value)?;
                            self.emit_op(vm_op, line, Some(root));
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Rot, line, Some(root));
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetArrowHash, line, Some(root));
                        }
                    }
                } else if let ExprKind::ArrowDeref {
                    expr,
                    index,
                    kind: DerefKind::Array,
                } = &target.kind
                {
                    if let ExprKind::List(indices) = &index.kind {
                        if matches!(op, BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd) {
                            let k = indices.len() as u16;
                            self.compile_arrow_array_base_expr(expr)?;
                            for ix in indices {
                                self.compile_array_slice_index_expr(ix)?;
                            }
                            self.emit_op(Op::ArrowArraySlicePeekLast(k), line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::ArrowArraySliceRollValUnderSpecs(k), line, Some(root));
                            self.emit_op(Op::SetArrowArraySliceLastKeep(k), line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::ArrowArraySliceDropKeysKeepCur(k), line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                            return Ok(());
                        }
                        // Multi-index `@$aref[i1,i2,...] OP= EXPR` — Perl applies the op only to the
                        // last index (see `Interpreter::compound_assign_arrow_array_slice`).
                        let op_byte = scalar_compound_op_to_byte(*op).ok_or_else(|| {
                            CompileError::Unsupported(
                                "CompoundAssign op on multi-index array slice".into(),
                            )
                        })?;
                        self.compile_expr(value)?;
                        self.compile_arrow_array_base_expr(expr)?;
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(
                            Op::ArrowArraySliceCompound(op_byte, indices.len() as u16),
                            line,
                            Some(root),
                        );
                        return Ok(());
                    }
                    match op {
                        BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd => {
                            // Same last-slot short-circuit semantics as `@$r[i,j] //=` but with one
                            // subscript slot (`..` / list / `qw` flatten to multiple indices).
                            self.compile_arrow_array_base_expr(expr)?;
                            self.compile_array_slice_index_expr(index)?;
                            self.emit_op(Op::ArrowArraySlicePeekLast(1), line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::ArrowArraySliceRollValUnderSpecs(1), line, Some(root));
                            self.emit_op(Op::SetArrowArraySliceLastKeep(1), line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::ArrowArraySliceDropKeysKeepCur(1), line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        _ => {
                            let op_byte = scalar_compound_op_to_byte(*op).ok_or_else(|| {
                                CompileError::Unsupported("CompoundAssign op".into())
                            })?;
                            self.compile_expr(value)?;
                            self.compile_arrow_array_base_expr(expr)?;
                            self.compile_array_slice_index_expr(index)?;
                            self.emit_op(Op::ArrowArraySliceCompound(op_byte, 1), line, Some(root));
                        }
                    }
                } else if let ExprKind::HashSliceDeref { container, keys } = &target.kind {
                    // Single-key `@$href{"k"} OP= EXPR` matches `$href->{"k"} OP= EXPR` (ArrowHash).
                    // Multi-key `@$href{k1,k2} OP= EXPR` — Perl applies the op only to the last key.
                    if keys.is_empty() {
                        // Mirror `@h{} OP= EXPR`: evaluate invocant and RHS, then error (matches
                        // [`ExprKind::HashSlice`] empty `keys` compound path).
                        self.compile_expr(container)?;
                        self.emit_op(Op::Pop, line, Some(root));
                        self.compile_expr(value)?;
                        self.emit_op(Op::Pop, line, Some(root));
                        let idx = self
                            .chunk
                            .add_constant(PerlValue::string("assign to empty hash slice".into()));
                        self.emit_op(Op::RuntimeErrorConst(idx), line, Some(root));
                        self.emit_op(Op::LoadUndef, line, Some(root));
                        return Ok(());
                    }
                    if hash_slice_needs_slice_ops(keys) {
                        if matches!(op, BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd) {
                            let k = keys.len() as u16;
                            self.compile_expr(container)?;
                            for hk in keys {
                                self.compile_expr(hk)?;
                            }
                            self.emit_op(Op::HashSliceDerefPeekLast(k), line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::HashSliceDerefRollValUnderKeys(k), line, Some(root));
                            self.emit_op(Op::HashSliceDerefSetLastKeep(k), line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::HashSliceDerefDropKeysKeepCur(k), line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                            return Ok(());
                        }
                        let op_byte = scalar_compound_op_to_byte(*op).ok_or_else(|| {
                            CompileError::Unsupported(
                                "CompoundAssign op on multi-key hash slice".into(),
                            )
                        })?;
                        self.compile_expr(value)?;
                        self.compile_expr(container)?;
                        for hk in keys {
                            self.compile_expr(hk)?;
                        }
                        self.emit_op(
                            Op::HashSliceDerefCompound(op_byte, keys.len() as u16),
                            line,
                            Some(root),
                        );
                        return Ok(());
                    }
                    let hk = &keys[0];
                    match op {
                        BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd => {
                            self.compile_expr(container)?;
                            self.compile_expr(hk)?;
                            self.emit_op(Op::Dup2, line, Some(root));
                            self.emit_op(Op::ArrowHash, line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Rot, line, Some(root));
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetArrowHashKeep, line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        _ => {
                            let vm_op = binop_to_vm_op(*op).ok_or_else(|| {
                                CompileError::Unsupported("CompoundAssign op".into())
                            })?;
                            self.compile_expr(container)?;
                            self.compile_expr(hk)?;
                            self.emit_op(Op::Dup2, line, Some(root));
                            self.emit_op(Op::ArrowHash, line, Some(root));
                            self.compile_expr(value)?;
                            self.emit_op(vm_op, line, Some(root));
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Rot, line, Some(root));
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetArrowHash, line, Some(root));
                        }
                    }
                } else if let ExprKind::HashSlice { hash, keys } = &target.kind {
                    if keys.is_empty() {
                        if self.is_mysync_hash(hash) {
                            return Err(CompileError::Unsupported(
                                "mysync hash slice update (tree interpreter)".into(),
                            ));
                        }
                        self.check_strict_hash_access(hash, line)?;
                        self.check_hash_mutable(hash, line)?;
                        self.compile_expr(value)?;
                        self.emit_op(Op::Pop, line, Some(root));
                        let idx = self
                            .chunk
                            .add_constant(PerlValue::string("assign to empty hash slice".into()));
                        self.emit_op(Op::RuntimeErrorConst(idx), line, Some(root));
                        self.emit_op(Op::LoadUndef, line, Some(root));
                        return Ok(());
                    }
                    if self.is_mysync_hash(hash) {
                        return Err(CompileError::Unsupported(
                            "mysync hash slice update (tree interpreter)".into(),
                        ));
                    }
                    self.check_strict_hash_access(hash, line)?;
                    self.check_hash_mutable(hash, line)?;
                    let hash_idx = self.chunk.intern_name(hash);
                    if hash_slice_needs_slice_ops(keys) {
                        if matches!(op, BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd) {
                            let k = keys.len() as u16;
                            for hk in keys {
                                self.compile_expr(hk)?;
                            }
                            self.emit_op(Op::NamedHashSlicePeekLast(hash_idx, k), line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::NamedArraySliceRollValUnderSpecs(k), line, Some(root));
                            self.emit_op(
                                Op::SetNamedHashSliceLastKeep(hash_idx, k),
                                line,
                                Some(root),
                            );
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::NamedHashSliceDropKeysKeepCur(k), line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                            return Ok(());
                        }
                        let op_byte = scalar_compound_op_to_byte(*op).ok_or_else(|| {
                            CompileError::Unsupported(
                                "CompoundAssign op on multi-key hash slice".into(),
                            )
                        })?;
                        self.compile_expr(value)?;
                        for hk in keys {
                            self.compile_expr(hk)?;
                        }
                        self.emit_op(
                            Op::NamedHashSliceCompound(op_byte, hash_idx, keys.len() as u16),
                            line,
                            Some(root),
                        );
                        return Ok(());
                    }
                    let hk = &keys[0];
                    match op {
                        BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd => {
                            self.compile_expr(hk)?;
                            self.emit_op(Op::Dup, line, Some(root));
                            self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::SetHashElemKeep(hash_idx), line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::Swap, line, Some(root));
                            self.emit_op(Op::Pop, line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        _ => {
                            let op_byte = scalar_compound_op_to_byte(*op).ok_or_else(|| {
                                CompileError::Unsupported("CompoundAssign op".into())
                            })?;
                            self.compile_expr(value)?;
                            self.compile_expr(hk)?;
                            self.emit_op(
                                Op::NamedHashSliceCompound(op_byte, hash_idx, 1),
                                line,
                                Some(root),
                            );
                        }
                    }
                } else if let ExprKind::ArraySlice { array, indices } = &target.kind {
                    if indices.is_empty() {
                        if self.is_mysync_array(array) {
                            return Err(CompileError::Unsupported(
                                "mysync array slice update (tree interpreter)".into(),
                            ));
                        }
                        let q = self.qualify_stash_array_name(array);
                        self.check_array_mutable(&q, line)?;
                        let arr_idx = self.chunk.intern_name(&q);
                        if matches!(op, BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd) {
                            self.compile_expr(value)?;
                            self.emit_op(Op::Pop, line, Some(root));
                            let idx = self.chunk.add_constant(PerlValue::string(
                                "assign to empty array slice".into(),
                            ));
                            self.emit_op(Op::RuntimeErrorConst(idx), line, Some(root));
                            self.emit_op(Op::LoadUndef, line, Some(root));
                            return Ok(());
                        }
                        let op_byte = scalar_compound_op_to_byte(*op).ok_or_else(|| {
                            CompileError::Unsupported(
                                "CompoundAssign op on named array slice".into(),
                            )
                        })?;
                        self.compile_expr(value)?;
                        self.emit_op(
                            Op::NamedArraySliceCompound(op_byte, arr_idx, 0),
                            line,
                            Some(root),
                        );
                        return Ok(());
                    }
                    if self.is_mysync_array(array) {
                        return Err(CompileError::Unsupported(
                            "mysync array slice update (tree interpreter)".into(),
                        ));
                    }
                    let q = self.qualify_stash_array_name(array);
                    self.check_array_mutable(&q, line)?;
                    let arr_idx = self.chunk.intern_name(&q);
                    if matches!(op, BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd) {
                        let k = indices.len() as u16;
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(Op::NamedArraySlicePeekLast(arr_idx, k), line, Some(root));
                        let j = match *op {
                            BinOp::DefinedOr => {
                                self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                            }
                            BinOp::LogOr => self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root)),
                            BinOp::LogAnd => self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root)),
                            _ => unreachable!(),
                        };
                        self.compile_expr(value)?;
                        self.emit_op(Op::NamedArraySliceRollValUnderSpecs(k), line, Some(root));
                        self.emit_op(Op::SetNamedArraySliceLastKeep(arr_idx, k), line, Some(root));
                        let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                        self.chunk.patch_jump_here(j);
                        self.emit_op(Op::NamedArraySliceDropKeysKeepCur(k), line, Some(root));
                        self.chunk.patch_jump_here(j_end);
                        return Ok(());
                    }
                    let op_byte = scalar_compound_op_to_byte(*op).ok_or_else(|| {
                        CompileError::Unsupported("CompoundAssign op on named array slice".into())
                    })?;
                    self.compile_expr(value)?;
                    for ix in indices {
                        self.compile_array_slice_index_expr(ix)?;
                    }
                    self.emit_op(
                        Op::NamedArraySliceCompound(op_byte, arr_idx, indices.len() as u16),
                        line,
                        Some(root),
                    );
                    return Ok(());
                } else if let ExprKind::AnonymousListSlice { source, indices } = &target.kind {
                    let ExprKind::Deref {
                        expr: inner,
                        kind: Sigil::Array,
                    } = &source.kind
                    else {
                        return Err(CompileError::Unsupported(
                            "CompoundAssign on AnonymousListSlice (non-array deref)".into(),
                        ));
                    };
                    if indices.is_empty() {
                        self.compile_arrow_array_base_expr(inner)?;
                        self.emit_op(Op::Pop, line, Some(root));
                        self.compile_expr(value)?;
                        self.emit_op(Op::Pop, line, Some(root));
                        let idx = self
                            .chunk
                            .add_constant(PerlValue::string("assign to empty array slice".into()));
                        self.emit_op(Op::RuntimeErrorConst(idx), line, Some(root));
                        self.emit_op(Op::LoadUndef, line, Some(root));
                        return Ok(());
                    }
                    if indices.len() > 1 {
                        if matches!(op, BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd) {
                            let k = indices.len() as u16;
                            self.compile_arrow_array_base_expr(inner)?;
                            for ix in indices {
                                self.compile_array_slice_index_expr(ix)?;
                            }
                            self.emit_op(Op::ArrowArraySlicePeekLast(k), line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::ArrowArraySliceRollValUnderSpecs(k), line, Some(root));
                            self.emit_op(Op::SetArrowArraySliceLastKeep(k), line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::ArrowArraySliceDropKeysKeepCur(k), line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                            return Ok(());
                        }
                        let op_byte = scalar_compound_op_to_byte(*op).ok_or_else(|| {
                            CompileError::Unsupported(
                                "CompoundAssign op on multi-index array slice".into(),
                            )
                        })?;
                        self.compile_expr(value)?;
                        self.compile_arrow_array_base_expr(inner)?;
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(
                            Op::ArrowArraySliceCompound(op_byte, indices.len() as u16),
                            line,
                            Some(root),
                        );
                        return Ok(());
                    }
                    let ix0 = &indices[0];
                    match op {
                        BinOp::DefinedOr | BinOp::LogOr | BinOp::LogAnd => {
                            self.compile_arrow_array_base_expr(inner)?;
                            self.compile_array_slice_index_expr(ix0)?;
                            self.emit_op(Op::ArrowArraySlicePeekLast(1), line, Some(root));
                            let j = match *op {
                                BinOp::DefinedOr => {
                                    self.emit_op(Op::JumpIfDefinedKeep(0), line, Some(root))
                                }
                                BinOp::LogOr => {
                                    self.emit_op(Op::JumpIfTrueKeep(0), line, Some(root))
                                }
                                BinOp::LogAnd => {
                                    self.emit_op(Op::JumpIfFalseKeep(0), line, Some(root))
                                }
                                _ => unreachable!(),
                            };
                            self.compile_expr(value)?;
                            self.emit_op(Op::ArrowArraySliceRollValUnderSpecs(1), line, Some(root));
                            self.emit_op(Op::SetArrowArraySliceLastKeep(1), line, Some(root));
                            let j_end = self.emit_op(Op::Jump(0), line, Some(root));
                            self.chunk.patch_jump_here(j);
                            self.emit_op(Op::ArrowArraySliceDropKeysKeepCur(1), line, Some(root));
                            self.chunk.patch_jump_here(j_end);
                        }
                        _ => {
                            let op_byte = scalar_compound_op_to_byte(*op).ok_or_else(|| {
                                CompileError::Unsupported("CompoundAssign op".into())
                            })?;
                            self.compile_expr(value)?;
                            self.compile_arrow_array_base_expr(inner)?;
                            self.compile_array_slice_index_expr(ix0)?;
                            self.emit_op(Op::ArrowArraySliceCompound(op_byte, 1), line, Some(root));
                        }
                    }
                } else {
                    return Err(CompileError::Unsupported(
                        "CompoundAssign on non-scalar".into(),
                    ));
                }
            }

            ExprKind::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                self.compile_boolean_rvalue_condition(condition)?;
                let jump_else = self.emit_op(Op::JumpIfFalse(0), line, Some(root));
                self.compile_expr(then_expr)?;
                let jump_end = self.emit_op(Op::Jump(0), line, Some(root));
                self.chunk.patch_jump_here(jump_else);
                self.compile_expr(else_expr)?;
                self.chunk.patch_jump_here(jump_end);
            }

            ExprKind::Range {
                from,
                to,
                exclusive,
            } => {
                if ctx == WantarrayCtx::List {
                    self.compile_expr_ctx(from, WantarrayCtx::Scalar)?;
                    self.compile_expr_ctx(to, WantarrayCtx::Scalar)?;
                    self.emit_op(Op::Range, line, Some(root));
                } else if let (ExprKind::Regex(lp, lf), ExprKind::Regex(rp, rf)) =
                    (&from.kind, &to.kind)
                {
                    let slot = self.chunk.alloc_flip_flop_slot();
                    let lp_idx = self.chunk.add_constant(PerlValue::string(lp.clone()));
                    let lf_idx = self.chunk.add_constant(PerlValue::string(lf.clone()));
                    let rp_idx = self.chunk.add_constant(PerlValue::string(rp.clone()));
                    let rf_idx = self.chunk.add_constant(PerlValue::string(rf.clone()));
                    self.emit_op(
                        Op::RegexFlipFlop(
                            slot,
                            u8::from(*exclusive),
                            lp_idx,
                            lf_idx,
                            rp_idx,
                            rf_idx,
                        ),
                        line,
                        Some(root),
                    );
                } else if let (ExprKind::Regex(lp, lf), ExprKind::Eof(None)) =
                    (&from.kind, &to.kind)
                {
                    let slot = self.chunk.alloc_flip_flop_slot();
                    let lp_idx = self.chunk.add_constant(PerlValue::string(lp.clone()));
                    let lf_idx = self.chunk.add_constant(PerlValue::string(lf.clone()));
                    self.emit_op(
                        Op::RegexEofFlipFlop(slot, u8::from(*exclusive), lp_idx, lf_idx),
                        line,
                        Some(root),
                    );
                } else if matches!(
                    (&from.kind, &to.kind),
                    (ExprKind::Regex(_, _), ExprKind::Eof(Some(_)))
                ) {
                    return Err(CompileError::Unsupported(
                        "regex flip-flop with eof(HANDLE) is not supported".into(),
                    ));
                } else if let ExprKind::Regex(lp, lf) = &from.kind {
                    let slot = self.chunk.alloc_flip_flop_slot();
                    let lp_idx = self.chunk.add_constant(PerlValue::string(lp.clone()));
                    let lf_idx = self.chunk.add_constant(PerlValue::string(lf.clone()));
                    if matches!(to.kind, ExprKind::Integer(_) | ExprKind::Float(_)) {
                        let line_target = match &to.kind {
                            ExprKind::Integer(n) => *n,
                            ExprKind::Float(f) => *f as i64,
                            _ => unreachable!(),
                        };
                        let line_cidx = self.chunk.add_constant(PerlValue::integer(line_target));
                        self.emit_op(
                            Op::RegexFlipFlopDotLineRhs(
                                slot,
                                u8::from(*exclusive),
                                lp_idx,
                                lf_idx,
                                line_cidx,
                            ),
                            line,
                            Some(root),
                        );
                    } else {
                        let rhs_idx = self
                            .chunk
                            .add_regex_flip_flop_rhs_expr_entry((**to).clone());
                        self.emit_op(
                            Op::RegexFlipFlopExprRhs(
                                slot,
                                u8::from(*exclusive),
                                lp_idx,
                                lf_idx,
                                rhs_idx,
                            ),
                            line,
                            Some(root),
                        );
                    }
                } else {
                    self.compile_expr(from)?;
                    self.compile_expr(to)?;
                    let slot = self.chunk.alloc_flip_flop_slot();
                    self.emit_op(
                        Op::ScalarFlipFlop(slot, u8::from(*exclusive)),
                        line,
                        Some(root),
                    );
                }
            }

            ExprKind::Repeat { expr, count } => {
                self.compile_expr(expr)?;
                self.compile_expr(count)?;
                self.emit_op(Op::StringRepeat, line, Some(root));
            }

            // ── Function calls ──
            ExprKind::FuncCall { name, args } => match name.as_str() {
                "deque" => {
                    if !args.is_empty() {
                        return Err(CompileError::Unsupported(
                            "deque() takes no arguments".into(),
                        ));
                    }
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::DequeNew as u16, 0),
                        line,
                        Some(root),
                    );
                }
                "heap" => {
                    if args.len() != 1 {
                        return Err(CompileError::Unsupported(
                            "heap() expects one comparator sub".into(),
                        ));
                    }
                    self.compile_expr(&args[0])?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::HeapNew as u16, 1),
                        line,
                        Some(root),
                    );
                }
                "pipeline" => {
                    for arg in args {
                        self.compile_expr_ctx(arg, WantarrayCtx::List)?;
                    }
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Pipeline as u16, args.len() as u8),
                        line,
                        Some(root),
                    );
                }
                "par_pipeline" => {
                    for arg in args {
                        self.compile_expr_ctx(arg, WantarrayCtx::List)?;
                    }
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::ParPipeline as u16, args.len() as u8),
                        line,
                        Some(root),
                    );
                }
                "par_pipeline_stream" => {
                    for arg in args {
                        self.compile_expr_ctx(arg, WantarrayCtx::List)?;
                    }
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::ParPipelineStream as u16, args.len() as u8),
                        line,
                        Some(root),
                    );
                }
                "ppool" => {
                    if args.len() != 1 {
                        return Err(CompileError::Unsupported(
                            "ppool() expects one argument (worker count)".into(),
                        ));
                    }
                    self.compile_expr(&args[0])?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Ppool as u16, 1),
                        line,
                        Some(root),
                    );
                }
                "barrier" => {
                    if args.len() != 1 {
                        return Err(CompileError::Unsupported(
                            "barrier() expects one argument (party count)".into(),
                        ));
                    }
                    self.compile_expr(&args[0])?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::BarrierNew as u16, 1),
                        line,
                        Some(root),
                    );
                }
                "pselect" => {
                    if args.is_empty() {
                        return Err(CompileError::Unsupported(
                            "pselect() expects at least one pchannel receiver".into(),
                        ));
                    }
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Pselect as u16, args.len() as u8),
                        line,
                        Some(root),
                    );
                }
                _ => {
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    let q = self.qualify_sub_key(name);
                    let name_idx = self.chunk.intern_name(&q);
                    self.emit_op(
                        Op::Call(name_idx, args.len() as u8, ctx.as_byte()),
                        line,
                        Some(root),
                    );
                }
            },

            // ── Method calls ──
            ExprKind::MethodCall {
                object,
                method,
                args,
                super_call,
            } => {
                self.compile_expr(object)?;
                for arg in args {
                    self.compile_expr(arg)?;
                }
                let name_idx = self.chunk.intern_name(method);
                if *super_call {
                    self.emit_op(
                        Op::MethodCallSuper(name_idx, args.len() as u8, ctx.as_byte()),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::MethodCall(name_idx, args.len() as u8, ctx.as_byte()),
                        line,
                        Some(root),
                    );
                }
            }
            ExprKind::IndirectCall {
                target,
                args,
                ampersand: _,
                pass_caller_arglist,
            } => {
                self.compile_expr(target)?;
                if !pass_caller_arglist {
                    for a in args {
                        self.compile_expr(a)?;
                    }
                }
                let argc = if *pass_caller_arglist {
                    0
                } else {
                    args.len() as u8
                };
                self.emit_op(
                    Op::IndirectCall(
                        argc,
                        ctx.as_byte(),
                        if *pass_caller_arglist { 1 } else { 0 },
                    ),
                    line,
                    Some(root),
                );
            }

            // ── Print / Say / Printf ──
            ExprKind::Print { handle, args } => {
                for arg in args {
                    self.compile_expr_ctx(arg, WantarrayCtx::List)?;
                }
                let h = handle.as_ref().map(|s| self.chunk.intern_name(s));
                self.emit_op(Op::Print(h, args.len() as u8), line, Some(root));
            }
            ExprKind::Say { handle, args } => {
                for arg in args {
                    self.compile_expr_ctx(arg, WantarrayCtx::List)?;
                }
                let h = handle.as_ref().map(|s| self.chunk.intern_name(s));
                self.emit_op(Op::Say(h, args.len() as u8), line, Some(root));
            }
            ExprKind::Printf { args, .. } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Printf as u16, args.len() as u8),
                    line,
                    Some(root),
                );
            }

            // ── Die / Warn ──
            ExprKind::Die(args) => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Die as u16, args.len() as u8),
                    line,
                    Some(root),
                );
            }
            ExprKind::Warn(args) => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Warn as u16, args.len() as u8),
                    line,
                    Some(root),
                );
            }
            ExprKind::Exit(code) => {
                if let Some(c) = code {
                    self.compile_expr(c)?;
                    self.emit_op(Op::CallBuiltin(BuiltinId::Exit as u16, 1), line, Some(root));
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                    self.emit_op(Op::CallBuiltin(BuiltinId::Exit as u16, 1), line, Some(root));
                }
            }

            // ── Array ops ──
            ExprKind::Push { array, values } => {
                if let ExprKind::ArrayVar(name) = &array.kind {
                    let idx = self.chunk.intern_name(&self.qualify_stash_array_name(name));
                    for v in values {
                        self.compile_expr_ctx(v, WantarrayCtx::List)?;
                        self.emit_op(Op::PushArray(idx), line, Some(root));
                    }
                    self.emit_op(Op::ArrayLen(idx), line, Some(root));
                } else if let ExprKind::Deref {
                    expr: aref_expr,
                    kind: Sigil::Array,
                } = &array.kind
                {
                    self.compile_expr(aref_expr)?;
                    for v in values {
                        self.emit_op(Op::Dup, line, Some(root));
                        self.compile_expr_ctx(v, WantarrayCtx::List)?;
                        self.emit_op(Op::PushArrayDeref, line, Some(root));
                    }
                    self.emit_op(Op::ArrayDerefLen, line, Some(root));
                } else {
                    let pool = self
                        .chunk
                        .add_push_expr_entry(array.as_ref().clone(), values.clone());
                    self.emit_op(Op::PushExpr(pool), line, Some(root));
                }
            }
            ExprKind::Pop(array) => {
                if let ExprKind::ArrayVar(name) = &array.kind {
                    let idx = self.chunk.intern_name(&self.qualify_stash_array_name(name));
                    self.emit_op(Op::PopArray(idx), line, Some(root));
                } else if let ExprKind::Deref {
                    expr: aref_expr,
                    kind: Sigil::Array,
                } = &array.kind
                {
                    self.compile_expr(aref_expr)?;
                    self.emit_op(Op::PopArrayDeref, line, Some(root));
                } else {
                    let pool = self.chunk.add_pop_expr_entry(array.as_ref().clone());
                    self.emit_op(Op::PopExpr(pool), line, Some(root));
                }
            }
            ExprKind::Shift(array) => {
                if let ExprKind::ArrayVar(name) = &array.kind {
                    let idx = self.chunk.intern_name(&self.qualify_stash_array_name(name));
                    self.emit_op(Op::ShiftArray(idx), line, Some(root));
                } else if let ExprKind::Deref {
                    expr: aref_expr,
                    kind: Sigil::Array,
                } = &array.kind
                {
                    self.compile_expr(aref_expr)?;
                    self.emit_op(Op::ShiftArrayDeref, line, Some(root));
                } else {
                    let pool = self.chunk.add_shift_expr_entry(array.as_ref().clone());
                    self.emit_op(Op::ShiftExpr(pool), line, Some(root));
                }
            }
            ExprKind::Unshift { array, values } => {
                if let ExprKind::ArrayVar(name) = &array.kind {
                    let q = self.qualify_stash_array_name(name);
                    let name_const = self.chunk.add_constant(PerlValue::string(q));
                    self.emit_op(Op::LoadConst(name_const), line, Some(root));
                    for v in values {
                        self.compile_expr_ctx(v, WantarrayCtx::List)?;
                    }
                    let nargs = (1 + values.len()) as u8;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Unshift as u16, nargs),
                        line,
                        Some(root),
                    );
                } else if let ExprKind::Deref {
                    expr: aref_expr,
                    kind: Sigil::Array,
                } = &array.kind
                {
                    if values.len() > u8::MAX as usize {
                        let pool = self
                            .chunk
                            .add_unshift_expr_entry(array.as_ref().clone(), values.clone());
                        self.emit_op(Op::UnshiftExpr(pool), line, Some(root));
                    } else {
                        self.compile_expr(aref_expr)?;
                        for v in values {
                            self.compile_expr_ctx(v, WantarrayCtx::List)?;
                        }
                        self.emit_op(Op::UnshiftArrayDeref(values.len() as u8), line, Some(root));
                    }
                } else {
                    let pool = self
                        .chunk
                        .add_unshift_expr_entry(array.as_ref().clone(), values.clone());
                    self.emit_op(Op::UnshiftExpr(pool), line, Some(root));
                }
            }
            ExprKind::Splice {
                array,
                offset,
                length,
                replacement,
            } => {
                self.emit_op(Op::WantarrayPush(ctx.as_byte()), line, Some(root));
                if let ExprKind::ArrayVar(name) = &array.kind {
                    let q = self.qualify_stash_array_name(name);
                    let name_const = self.chunk.add_constant(PerlValue::string(q));
                    self.emit_op(Op::LoadConst(name_const), line, Some(root));
                    if let Some(o) = offset {
                        self.compile_expr(o)?;
                    } else {
                        self.emit_op(Op::LoadInt(0), line, Some(root));
                    }
                    if let Some(l) = length {
                        self.compile_expr(l)?;
                    } else {
                        self.emit_op(Op::LoadUndef, line, Some(root));
                    }
                    for r in replacement {
                        self.compile_expr(r)?;
                    }
                    let nargs = (3 + replacement.len()) as u8;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Splice as u16, nargs),
                        line,
                        Some(root),
                    );
                } else if let ExprKind::Deref {
                    expr: aref_expr,
                    kind: Sigil::Array,
                } = &array.kind
                {
                    if replacement.len() > u8::MAX as usize {
                        let pool = self.chunk.add_splice_expr_entry(
                            array.as_ref().clone(),
                            offset.as_deref().cloned(),
                            length.as_deref().cloned(),
                            replacement.clone(),
                        );
                        self.emit_op(Op::SpliceExpr(pool), line, Some(root));
                    } else {
                        self.compile_expr(aref_expr)?;
                        if let Some(o) = offset {
                            self.compile_expr(o)?;
                        } else {
                            self.emit_op(Op::LoadInt(0), line, Some(root));
                        }
                        if let Some(l) = length {
                            self.compile_expr(l)?;
                        } else {
                            self.emit_op(Op::LoadUndef, line, Some(root));
                        }
                        for r in replacement {
                            self.compile_expr(r)?;
                        }
                        self.emit_op(
                            Op::SpliceArrayDeref(replacement.len() as u8),
                            line,
                            Some(root),
                        );
                    }
                } else {
                    let pool = self.chunk.add_splice_expr_entry(
                        array.as_ref().clone(),
                        offset.as_deref().cloned(),
                        length.as_deref().cloned(),
                        replacement.clone(),
                    );
                    self.emit_op(Op::SpliceExpr(pool), line, Some(root));
                }
                self.emit_op(Op::WantarrayPop, line, Some(root));
            }
            ExprKind::ScalarContext(inner) => {
                // `scalar EXPR` forces scalar context on EXPR regardless of the outer context
                // (e.g. `print scalar grep { } @x` — grep's result is a count, not a list).
                self.compile_expr_ctx(inner, WantarrayCtx::Scalar)?;
            }

            // ── Hash ops ──
            ExprKind::Delete(inner) => {
                if let ExprKind::HashElement { hash, key } = &inner.kind {
                    self.check_hash_mutable(hash, line)?;
                    let idx = self.chunk.intern_name(hash);
                    self.compile_expr(key)?;
                    self.emit_op(Op::DeleteHashElem(idx), line, Some(root));
                } else if let ExprKind::ArrayElement { array, index } = &inner.kind {
                    self.check_strict_array_access(array, line)?;
                    let q = self.qualify_stash_array_name(array);
                    self.check_array_mutable(&q, line)?;
                    let arr_idx = self.chunk.intern_name(&q);
                    self.compile_expr(index)?;
                    self.emit_op(Op::DeleteArrayElem(arr_idx), line, Some(root));
                } else if let ExprKind::ArrowDeref {
                    expr: container,
                    index,
                    kind: DerefKind::Hash,
                } = &inner.kind
                {
                    self.compile_arrow_hash_base_expr(container)?;
                    self.compile_expr(index)?;
                    self.emit_op(Op::DeleteArrowHashElem, line, Some(root));
                } else if let ExprKind::ArrowDeref {
                    expr: container,
                    index,
                    kind: DerefKind::Array,
                } = &inner.kind
                {
                    if arrow_deref_arrow_subscript_is_plain_scalar_index(index) {
                        self.compile_expr(container)?;
                        self.compile_expr(index)?;
                        self.emit_op(Op::DeleteArrowArrayElem, line, Some(root));
                    } else {
                        let pool = self.chunk.add_delete_expr_entry(inner.as_ref().clone());
                        self.emit_op(Op::DeleteExpr(pool), line, Some(root));
                    }
                } else {
                    let pool = self.chunk.add_delete_expr_entry(inner.as_ref().clone());
                    self.emit_op(Op::DeleteExpr(pool), line, Some(root));
                }
            }
            ExprKind::Exists(inner) => {
                if let ExprKind::HashElement { hash, key } = &inner.kind {
                    let idx = self.chunk.intern_name(hash);
                    self.compile_expr(key)?;
                    self.emit_op(Op::ExistsHashElem(idx), line, Some(root));
                } else if let ExprKind::ArrayElement { array, index } = &inner.kind {
                    self.check_strict_array_access(array, line)?;
                    let arr_idx = self
                        .chunk
                        .intern_name(&self.qualify_stash_array_name(array));
                    self.compile_expr(index)?;
                    self.emit_op(Op::ExistsArrayElem(arr_idx), line, Some(root));
                } else if let ExprKind::ArrowDeref {
                    expr: container,
                    index,
                    kind: DerefKind::Hash,
                } = &inner.kind
                {
                    self.compile_arrow_hash_base_expr(container)?;
                    self.compile_expr(index)?;
                    self.emit_op(Op::ExistsArrowHashElem, line, Some(root));
                } else if let ExprKind::ArrowDeref {
                    expr: container,
                    index,
                    kind: DerefKind::Array,
                } = &inner.kind
                {
                    if arrow_deref_arrow_subscript_is_plain_scalar_index(index) {
                        self.compile_expr(container)?;
                        self.compile_expr(index)?;
                        self.emit_op(Op::ExistsArrowArrayElem, line, Some(root));
                    } else {
                        let pool = self.chunk.add_exists_expr_entry(inner.as_ref().clone());
                        self.emit_op(Op::ExistsExpr(pool), line, Some(root));
                    }
                } else {
                    let pool = self.chunk.add_exists_expr_entry(inner.as_ref().clone());
                    self.emit_op(Op::ExistsExpr(pool), line, Some(root));
                }
            }
            ExprKind::Keys(inner) => {
                if let ExprKind::HashVar(name) = &inner.kind {
                    let idx = self.chunk.intern_name(name);
                    if ctx == WantarrayCtx::List {
                        self.emit_op(Op::HashKeys(idx), line, Some(root));
                    } else {
                        self.emit_op(Op::HashKeysScalar(idx), line, Some(root));
                    }
                } else {
                    self.compile_expr_ctx(inner, WantarrayCtx::List)?;
                    if ctx == WantarrayCtx::List {
                        self.emit_op(Op::KeysFromValue, line, Some(root));
                    } else {
                        self.emit_op(Op::KeysFromValueScalar, line, Some(root));
                    }
                }
            }
            ExprKind::Values(inner) => {
                if let ExprKind::HashVar(name) = &inner.kind {
                    let idx = self.chunk.intern_name(name);
                    if ctx == WantarrayCtx::List {
                        self.emit_op(Op::HashValues(idx), line, Some(root));
                    } else {
                        self.emit_op(Op::HashValuesScalar(idx), line, Some(root));
                    }
                } else {
                    self.compile_expr_ctx(inner, WantarrayCtx::List)?;
                    if ctx == WantarrayCtx::List {
                        self.emit_op(Op::ValuesFromValue, line, Some(root));
                    } else {
                        self.emit_op(Op::ValuesFromValueScalar, line, Some(root));
                    }
                }
            }
            ExprKind::Each(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Each as u16, 1), line, Some(root));
            }

            // ── Builtins that map to CallBuiltin ──
            ExprKind::Length(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Length as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Chomp(e) => {
                self.compile_expr(e)?;
                let lv = self.chunk.add_lvalue_expr(e.as_ref().clone());
                self.emit_op(Op::ChompInPlace(lv), line, Some(root));
            }
            ExprKind::Chop(e) => {
                self.compile_expr(e)?;
                let lv = self.chunk.add_lvalue_expr(e.as_ref().clone());
                self.emit_op(Op::ChopInPlace(lv), line, Some(root));
            }
            ExprKind::Defined(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Defined as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Abs(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Abs as u16, 1), line, Some(root));
            }
            ExprKind::Int(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Int as u16, 1), line, Some(root));
            }
            ExprKind::Sqrt(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Sqrt as u16, 1), line, Some(root));
            }
            ExprKind::Sin(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Sin as u16, 1), line, Some(root));
            }
            ExprKind::Cos(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Cos as u16, 1), line, Some(root));
            }
            ExprKind::Atan2 { y, x } => {
                self.compile_expr(y)?;
                self.compile_expr(x)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Atan2 as u16, 2),
                    line,
                    Some(root),
                );
            }
            ExprKind::Exp(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Exp as u16, 1), line, Some(root));
            }
            ExprKind::Log(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Log as u16, 1), line, Some(root));
            }
            ExprKind::Rand(upper) => {
                if let Some(e) = upper {
                    self.compile_expr(e)?;
                    self.emit_op(Op::CallBuiltin(BuiltinId::Rand as u16, 1), line, Some(root));
                } else {
                    self.emit_op(Op::CallBuiltin(BuiltinId::Rand as u16, 0), line, Some(root));
                }
            }
            ExprKind::Srand(seed) => {
                if let Some(e) = seed {
                    self.compile_expr(e)?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Srand as u16, 1),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Srand as u16, 0),
                        line,
                        Some(root),
                    );
                }
            }
            ExprKind::Chr(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Chr as u16, 1), line, Some(root));
            }
            ExprKind::Ord(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Ord as u16, 1), line, Some(root));
            }
            ExprKind::Hex(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Hex as u16, 1), line, Some(root));
            }
            ExprKind::Oct(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Oct as u16, 1), line, Some(root));
            }
            ExprKind::Uc(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Uc as u16, 1), line, Some(root));
            }
            ExprKind::Lc(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Lc as u16, 1), line, Some(root));
            }
            ExprKind::Ucfirst(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Ucfirst as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Lcfirst(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Lcfirst as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Fc(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Fc as u16, 1), line, Some(root));
            }
            ExprKind::Crypt { plaintext, salt } => {
                self.compile_expr(plaintext)?;
                self.compile_expr(salt)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Crypt as u16, 2),
                    line,
                    Some(root),
                );
            }
            ExprKind::Pos(e) => match e {
                None => {
                    self.emit_op(Op::CallBuiltin(BuiltinId::Pos as u16, 0), line, Some(root));
                }
                Some(pos_arg) => {
                    if let ExprKind::ScalarVar(name) = &pos_arg.kind {
                        let idx = self.chunk.add_constant(PerlValue::string(name.clone()));
                        self.emit_op(Op::LoadConst(idx), line, Some(root));
                    } else {
                        self.compile_expr(pos_arg)?;
                    }
                    self.emit_op(Op::CallBuiltin(BuiltinId::Pos as u16, 1), line, Some(root));
                }
            },
            ExprKind::Study(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Study as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Ref(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Ref as u16, 1), line, Some(root));
            }
            ExprKind::ReverseExpr(e) => {
                self.compile_expr_ctx(e, WantarrayCtx::List)?;
                if ctx == WantarrayCtx::List {
                    self.emit_op(Op::ReverseListOp, line, Some(root));
                } else {
                    self.emit_op(Op::ReverseScalarOp, line, Some(root));
                }
            }
            ExprKind::System(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::System as u16, args.len() as u8),
                    line,
                    Some(root),
                );
            }
            ExprKind::Exec(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Exec as u16, args.len() as u8),
                    line,
                    Some(root),
                );
            }

            // ── String builtins ──
            ExprKind::Substr {
                string,
                offset,
                length,
                replacement,
            } => {
                if let Some(rep) = replacement {
                    let idx = self.chunk.add_substr_four_arg_entry(
                        string.as_ref().clone(),
                        offset.as_ref().clone(),
                        length.as_ref().map(|b| b.as_ref().clone()),
                        rep.as_ref().clone(),
                    );
                    self.emit_op(Op::SubstrFourArg(idx), line, Some(root));
                } else {
                    self.compile_expr(string)?;
                    self.compile_expr(offset)?;
                    let mut argc: u8 = 2;
                    if let Some(len) = length {
                        self.compile_expr(len)?;
                        argc = 3;
                    }
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Substr as u16, argc),
                        line,
                        Some(root),
                    );
                }
            }
            ExprKind::Index {
                string,
                substr,
                position,
            } => {
                self.compile_expr(string)?;
                self.compile_expr(substr)?;
                if let Some(pos) = position {
                    self.compile_expr(pos)?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Index as u16, 3),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Index as u16, 2),
                        line,
                        Some(root),
                    );
                }
            }
            ExprKind::Rindex {
                string,
                substr,
                position,
            } => {
                self.compile_expr(string)?;
                self.compile_expr(substr)?;
                if let Some(pos) = position {
                    self.compile_expr(pos)?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Rindex as u16, 3),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Rindex as u16, 2),
                        line,
                        Some(root),
                    );
                }
            }

            ExprKind::JoinExpr { separator, list } => {
                self.compile_expr(separator)?;
                // Arguments after the separator are evaluated in list context (Perl 5).
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Join as u16, 2), line, Some(root));
            }
            ExprKind::SplitExpr {
                pattern,
                string,
                limit,
            } => {
                self.compile_expr(pattern)?;
                self.compile_expr(string)?;
                if let Some(l) = limit {
                    self.compile_expr(l)?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Split as u16, 3),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Split as u16, 2),
                        line,
                        Some(root),
                    );
                }
            }
            ExprKind::Sprintf { format, args } => {
                self.compile_expr(format)?;
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Sprintf as u16, (1 + args.len()) as u8),
                    line,
                    Some(root),
                );
            }

            // ── I/O ──
            ExprKind::Open { handle, mode, file } => {
                if let ExprKind::OpenMyHandle { name } = &handle.kind {
                    let name_idx = self.chunk.intern_name(name);
                    self.emit_op(Op::LoadUndef, line, Some(root));
                    self.emit_declare_scalar(name_idx, line, false);
                    let h_idx = self.chunk.add_constant(PerlValue::string(name.clone()));
                    self.emit_op(Op::LoadConst(h_idx), line, Some(root));
                    self.compile_expr(mode)?;
                    if let Some(f) = file {
                        self.compile_expr(f)?;
                        self.emit_op(Op::CallBuiltin(BuiltinId::Open as u16, 3), line, Some(root));
                    } else {
                        self.emit_op(Op::CallBuiltin(BuiltinId::Open as u16, 2), line, Some(root));
                    }
                    self.emit_op(Op::SetScalarKeepPlain(name_idx), line, Some(root));
                    return Ok(());
                }
                self.compile_expr(handle)?;
                self.compile_expr(mode)?;
                if let Some(f) = file {
                    self.compile_expr(f)?;
                    self.emit_op(Op::CallBuiltin(BuiltinId::Open as u16, 3), line, Some(root));
                } else {
                    self.emit_op(Op::CallBuiltin(BuiltinId::Open as u16, 2), line, Some(root));
                }
            }
            ExprKind::OpenMyHandle { .. } => {
                return Err(CompileError::Unsupported(
                    "open my $fh handle expression".into(),
                ));
            }
            ExprKind::Close(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Close as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::ReadLine(handle) => {
                let bid = if ctx == WantarrayCtx::List {
                    BuiltinId::ReadLineList
                } else {
                    BuiltinId::ReadLine
                };
                if let Some(h) = handle {
                    let idx = self.chunk.add_constant(PerlValue::string(h.clone()));
                    self.emit_op(Op::LoadConst(idx), line, Some(root));
                    self.emit_op(Op::CallBuiltin(bid as u16, 1), line, Some(root));
                } else {
                    self.emit_op(Op::CallBuiltin(bid as u16, 0), line, Some(root));
                }
            }
            ExprKind::Eof(e) => {
                if let Some(inner) = e {
                    self.compile_expr(inner)?;
                    self.emit_op(Op::CallBuiltin(BuiltinId::Eof as u16, 1), line, Some(root));
                } else {
                    self.emit_op(Op::CallBuiltin(BuiltinId::Eof as u16, 0), line, Some(root));
                }
            }
            ExprKind::Opendir { handle, path } => {
                self.compile_expr(handle)?;
                self.compile_expr(path)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Opendir as u16, 2),
                    line,
                    Some(root),
                );
            }
            ExprKind::Readdir(e) => {
                let bid = if ctx == WantarrayCtx::List {
                    BuiltinId::ReaddirList
                } else {
                    BuiltinId::Readdir
                };
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(bid as u16, 1), line, Some(root));
            }
            ExprKind::Closedir(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Closedir as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Rewinddir(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Rewinddir as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Telldir(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Telldir as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Seekdir { handle, position } => {
                self.compile_expr(handle)?;
                self.compile_expr(position)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Seekdir as u16, 2),
                    line,
                    Some(root),
                );
            }

            // ── File tests ──
            ExprKind::FileTest { op, expr } => {
                self.compile_expr(expr)?;
                self.emit_op(Op::FileTestOp(*op as u8), line, Some(root));
            }

            // ── Eval / Do / Require ──
            ExprKind::Eval(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Eval as u16, 1), line, Some(root));
            }
            ExprKind::Do(e) => {
                // do { BLOCK } executes the block; do "file" loads a file
                if let ExprKind::CodeRef { body, .. } = &e.kind {
                    let block_idx = self.chunk.add_block(body.clone());
                    self.emit_op(Op::EvalBlock(block_idx, ctx.as_byte()), line, Some(root));
                } else {
                    self.compile_expr(e)?;
                    self.emit_op(Op::CallBuiltin(BuiltinId::Do as u16, 1), line, Some(root));
                }
            }
            ExprKind::Require(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Require as u16, 1),
                    line,
                    Some(root),
                );
            }

            // ── Filesystem ──
            ExprKind::Chdir(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Chdir as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Mkdir { path, mode } => {
                self.compile_expr(path)?;
                if let Some(m) = mode {
                    self.compile_expr(m)?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Mkdir as u16, 2),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Mkdir as u16, 1),
                        line,
                        Some(root),
                    );
                }
            }
            ExprKind::Unlink(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Unlink as u16, args.len() as u8),
                    line,
                    Some(root),
                );
            }
            ExprKind::Rename { old, new } => {
                self.compile_expr(old)?;
                self.compile_expr(new)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Rename as u16, 2),
                    line,
                    Some(root),
                );
            }
            ExprKind::Chmod(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Chmod as u16, args.len() as u8),
                    line,
                    Some(root),
                );
            }
            ExprKind::Chown(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Chown as u16, args.len() as u8),
                    line,
                    Some(root),
                );
            }
            ExprKind::Stat(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Stat as u16, 1), line, Some(root));
            }
            ExprKind::Lstat(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Lstat as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Link { old, new } => {
                self.compile_expr(old)?;
                self.compile_expr(new)?;
                self.emit_op(Op::CallBuiltin(BuiltinId::Link as u16, 2), line, Some(root));
            }
            ExprKind::Symlink { old, new } => {
                self.compile_expr(old)?;
                self.compile_expr(new)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Symlink as u16, 2),
                    line,
                    Some(root),
                );
            }
            ExprKind::Readlink(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Readlink as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Glob(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Glob as u16, args.len() as u8),
                    line,
                    Some(root),
                );
            }
            ExprKind::GlobPar { args, progress } => {
                for a in args {
                    self.compile_expr(a)?;
                }
                match progress {
                    None => {
                        self.emit_op(
                            Op::CallBuiltin(BuiltinId::GlobPar as u16, args.len() as u8),
                            line,
                            Some(root),
                        );
                    }
                    Some(p) => {
                        self.compile_expr(p)?;
                        self.emit_op(
                            Op::CallBuiltin(
                                BuiltinId::GlobParProgress as u16,
                                (args.len() + 1) as u8,
                            ),
                            line,
                            Some(root),
                        );
                    }
                }
            }
            ExprKind::ParSed { args, progress } => {
                for a in args {
                    self.compile_expr(a)?;
                }
                match progress {
                    None => {
                        self.emit_op(
                            Op::CallBuiltin(BuiltinId::ParSed as u16, args.len() as u8),
                            line,
                            Some(root),
                        );
                    }
                    Some(p) => {
                        self.compile_expr(p)?;
                        self.emit_op(
                            Op::CallBuiltin(
                                BuiltinId::ParSedProgress as u16,
                                (args.len() + 1) as u8,
                            ),
                            line,
                            Some(root),
                        );
                    }
                }
            }

            // ── OOP ──
            ExprKind::Bless { ref_expr, class } => {
                self.compile_expr(ref_expr)?;
                if let Some(c) = class {
                    self.compile_expr(c)?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Bless as u16, 2),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Bless as u16, 1),
                        line,
                        Some(root),
                    );
                }
            }
            ExprKind::Caller(e) => {
                if let Some(inner) = e {
                    self.compile_expr(inner)?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Caller as u16, 1),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Caller as u16, 0),
                        line,
                        Some(root),
                    );
                }
            }
            ExprKind::Wantarray => {
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Wantarray as u16, 0),
                    line,
                    Some(root),
                );
            }

            // ── References ──
            ExprKind::ScalarRef(e) => match &e.kind {
                ExprKind::ScalarVar(name) => {
                    let idx = self.chunk.intern_name(name);
                    self.emit_op(Op::MakeScalarBindingRef(idx), line, Some(root));
                }
                ExprKind::ArrayVar(name) => {
                    self.check_strict_array_access(name, line)?;
                    let idx = self.chunk.intern_name(&self.qualify_stash_array_name(name));
                    self.emit_op(Op::MakeArrayBindingRef(idx), line, Some(root));
                }
                ExprKind::HashVar(name) => {
                    self.check_strict_hash_access(name, line)?;
                    let idx = self.chunk.intern_name(name);
                    self.emit_op(Op::MakeHashBindingRef(idx), line, Some(root));
                }
                ExprKind::Deref {
                    expr: inner,
                    kind: Sigil::Array,
                } => {
                    self.compile_expr(inner)?;
                    self.emit_op(Op::MakeArrayRefAlias, line, Some(root));
                }
                ExprKind::Deref {
                    expr: inner,
                    kind: Sigil::Hash,
                } => {
                    self.compile_expr(inner)?;
                    self.emit_op(Op::MakeHashRefAlias, line, Some(root));
                }
                ExprKind::ArraySlice { .. } | ExprKind::HashSlice { .. } => {
                    self.compile_expr_ctx(e, WantarrayCtx::List)?;
                    self.emit_op(Op::MakeArrayRef, line, Some(root));
                }
                ExprKind::AnonymousListSlice { .. } | ExprKind::HashSliceDeref { .. } => {
                    self.compile_expr_ctx(e, WantarrayCtx::List)?;
                    self.emit_op(Op::MakeArrayRef, line, Some(root));
                }
                _ => {
                    self.compile_expr(e)?;
                    self.emit_op(Op::MakeScalarRef, line, Some(root));
                }
            },
            ExprKind::ArrayRef(elems) => {
                for e in elems {
                    self.compile_expr(e)?;
                }
                self.emit_op(Op::MakeArray(elems.len() as u16), line, Some(root));
                self.emit_op(Op::MakeArrayRef, line, Some(root));
            }
            ExprKind::HashRef(pairs) => {
                for (k, v) in pairs {
                    self.compile_expr(k)?;
                    self.compile_expr(v)?;
                }
                self.emit_op(Op::MakeHash((pairs.len() * 2) as u16), line, Some(root));
                self.emit_op(Op::MakeHashRef, line, Some(root));
            }
            ExprKind::CodeRef { body, .. } => {
                let block_idx = self.chunk.add_block(body.clone());
                self.emit_op(Op::MakeCodeRef(block_idx), line, Some(root));
            }
            ExprKind::SubroutineRef(name) => {
                // Unary `&name` — invoke subroutine with no explicit args (same as tree `call_named_sub`).
                let q = self.qualify_sub_key(name);
                let name_idx = self.chunk.intern_name(&q);
                self.emit_op(Op::Call(name_idx, 0, ctx.as_byte()), line, Some(root));
            }
            ExprKind::SubroutineCodeRef(name) => {
                // `\&name` — coderef (must exist at run time).
                let name_idx = self.chunk.intern_name(name);
                self.emit_op(Op::LoadNamedSubRef(name_idx), line, Some(root));
            }
            ExprKind::DynamicSubCodeRef(expr) => {
                self.compile_expr(expr)?;
                self.emit_op(Op::LoadDynamicSubRef, line, Some(root));
            }

            // ── Derefs ──
            ExprKind::ArrowDeref { expr, index, kind } => match kind {
                DerefKind::Array => {
                    self.compile_arrow_array_base_expr(expr)?;
                    let mut used_arrow_slice = false;
                    if let ExprKind::List(indices) = &index.kind {
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(Op::ArrowArraySlice(indices.len() as u16), line, Some(root));
                        used_arrow_slice = true;
                    } else if arrow_deref_arrow_subscript_is_plain_scalar_index(index) {
                        self.compile_expr(index)?;
                        self.emit_op(Op::ArrowArray, line, Some(root));
                    } else {
                        // One subscript expr may expand to multiple indices (`$r->[0..1]`, `[(0,1)]`).
                        self.compile_array_slice_index_expr(index)?;
                        self.emit_op(Op::ArrowArraySlice(1), line, Some(root));
                        used_arrow_slice = true;
                    }
                    if used_arrow_slice && ctx != WantarrayCtx::List {
                        self.emit_op(Op::ListSliceToScalar, line, Some(root));
                    }
                }
                DerefKind::Hash => {
                    self.compile_arrow_hash_base_expr(expr)?;
                    self.compile_expr(index)?;
                    self.emit_op(Op::ArrowHash, line, Some(root));
                }
                DerefKind::Call => {
                    self.compile_expr(expr)?;
                    self.compile_expr(index)?;
                    self.emit_op(Op::ArrowCall(ctx.as_byte()), line, Some(root));
                }
            },
            ExprKind::Deref { expr, kind } => {
                // Perl: `scalar @{EXPR}` / `scalar @$r` is the array length (not a copy of the list).
                // `scalar %{EXPR}` uses hash fill metrics like `%h` in scalar context.
                if ctx != WantarrayCtx::List && matches!(kind, Sigil::Array) {
                    self.compile_expr(expr)?;
                    self.emit_op(Op::ArrayDerefLen, line, Some(root));
                } else if ctx != WantarrayCtx::List && matches!(kind, Sigil::Hash) {
                    self.compile_expr(expr)?;
                    self.emit_op(Op::SymbolicDeref(2), line, Some(root));
                    self.emit_op(Op::ValueScalarContext, line, Some(root));
                } else {
                    self.compile_expr(expr)?;
                    let b = match kind {
                        Sigil::Scalar => 0u8,
                        Sigil::Array => 1,
                        Sigil::Hash => 2,
                        Sigil::Typeglob => 3,
                    };
                    self.emit_op(Op::SymbolicDeref(b), line, Some(root));
                }
            }

            // ── Interpolated strings ──
            ExprKind::InterpolatedString(parts) => {
                if parts.is_empty() {
                    let idx = self.chunk.add_constant(PerlValue::string(String::new()));
                    self.emit_op(Op::LoadConst(idx), line, Some(root));
                } else {
                    // `"$x"` is a single [`StringPart`] — still string context; must go through
                    // [`Op::Concat`] so operands are stringified (`use overload '""'`, etc.).
                    if !matches!(&parts[0], StringPart::Literal(_)) {
                        let idx = self.chunk.add_constant(PerlValue::string(String::new()));
                        self.emit_op(Op::LoadConst(idx), line, Some(root));
                    }
                    self.compile_string_part(&parts[0], line, Some(root))?;
                    for part in &parts[1..] {
                        self.compile_string_part(part, line, Some(root))?;
                        self.emit_op(Op::Concat, line, Some(root));
                    }
                    if !matches!(&parts[0], StringPart::Literal(_)) {
                        self.emit_op(Op::Concat, line, Some(root));
                    }
                }
            }

            // ── List ──
            ExprKind::List(exprs) => {
                if ctx == WantarrayCtx::Scalar {
                    // Perl: comma-list in scalar context evaluates to the **last** element (`(1,2)` → 2).
                    if let Some(last) = exprs.last() {
                        self.compile_expr_ctx(last, WantarrayCtx::Scalar)?;
                    } else {
                        self.emit_op(Op::LoadUndef, line, Some(root));
                    }
                } else {
                    for e in exprs {
                        self.compile_expr_ctx(e, ctx)?;
                    }
                    if exprs.len() != 1 {
                        self.emit_op(Op::MakeArray(exprs.len() as u16), line, Some(root));
                    }
                }
            }

            // ── QW ──
            ExprKind::QW(words) => {
                for w in words {
                    let idx = self.chunk.add_constant(PerlValue::string(w.clone()));
                    self.emit_op(Op::LoadConst(idx), line, Some(root));
                }
                self.emit_op(Op::MakeArray(words.len() as u16), line, Some(root));
            }

            // ── Postfix if/unless ──
            ExprKind::PostfixIf { expr, condition } => {
                self.compile_boolean_rvalue_condition(condition)?;
                let j = self.emit_op(Op::JumpIfFalse(0), line, Some(root));
                self.compile_expr(expr)?;
                let end = self.emit_op(Op::Jump(0), line, Some(root));
                self.chunk.patch_jump_here(j);
                self.emit_op(Op::LoadUndef, line, Some(root));
                self.chunk.patch_jump_here(end);
            }
            ExprKind::PostfixUnless { expr, condition } => {
                self.compile_boolean_rvalue_condition(condition)?;
                let j = self.emit_op(Op::JumpIfTrue(0), line, Some(root));
                self.compile_expr(expr)?;
                let end = self.emit_op(Op::Jump(0), line, Some(root));
                self.chunk.patch_jump_here(j);
                self.emit_op(Op::LoadUndef, line, Some(root));
                self.chunk.patch_jump_here(end);
            }

            // ── Postfix while/until/foreach ──
            ExprKind::PostfixWhile { expr, condition } => {
                // Detect `do { BLOCK } while (COND)` pattern
                let is_do_block = matches!(
                    &expr.kind,
                    ExprKind::Do(inner) if matches!(inner.kind, ExprKind::CodeRef { .. })
                );
                if is_do_block {
                    // do-while: body executes before first condition check
                    let loop_start = self.chunk.len();
                    self.compile_expr(expr)?;
                    self.emit_op(Op::Pop, line, Some(root));
                    self.compile_boolean_rvalue_condition(condition)?;
                    self.emit_op(Op::JumpIfTrue(loop_start), line, Some(root));
                    self.emit_op(Op::LoadUndef, line, Some(root));
                } else {
                    // Regular postfix while: condition checked first
                    let loop_start = self.chunk.len();
                    self.compile_boolean_rvalue_condition(condition)?;
                    let exit_jump = self.emit_op(Op::JumpIfFalse(0), line, Some(root));
                    self.compile_expr(expr)?;
                    self.emit_op(Op::Pop, line, Some(root));
                    self.emit_op(Op::Jump(loop_start), line, Some(root));
                    self.chunk.patch_jump_here(exit_jump);
                    self.emit_op(Op::LoadUndef, line, Some(root));
                }
            }
            ExprKind::PostfixUntil { expr, condition } => {
                let is_do_block = matches!(
                    &expr.kind,
                    ExprKind::Do(inner) if matches!(inner.kind, ExprKind::CodeRef { .. })
                );
                if is_do_block {
                    let loop_start = self.chunk.len();
                    self.compile_expr(expr)?;
                    self.emit_op(Op::Pop, line, Some(root));
                    self.compile_boolean_rvalue_condition(condition)?;
                    self.emit_op(Op::JumpIfFalse(loop_start), line, Some(root));
                    self.emit_op(Op::LoadUndef, line, Some(root));
                } else {
                    let loop_start = self.chunk.len();
                    self.compile_boolean_rvalue_condition(condition)?;
                    let exit_jump = self.emit_op(Op::JumpIfTrue(0), line, Some(root));
                    self.compile_expr(expr)?;
                    self.emit_op(Op::Pop, line, Some(root));
                    self.emit_op(Op::Jump(loop_start), line, Some(root));
                    self.chunk.patch_jump_here(exit_jump);
                    self.emit_op(Op::LoadUndef, line, Some(root));
                }
            }
            ExprKind::PostfixForeach { expr, list } => {
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let list_name = self.chunk.intern_name("__pf_foreach_list__");
                self.emit_op(Op::DeclareArray(list_name), line, Some(root));
                let counter = self.chunk.intern_name("__pf_foreach_i__");
                self.emit_op(Op::LoadInt(0), line, Some(root));
                self.emit_op(Op::DeclareScalar(counter), line, Some(root));
                let underscore = self.chunk.intern_name("_");

                let loop_start = self.chunk.len();
                self.emit_get_scalar(counter, line, Some(root));
                self.emit_op(Op::ArrayLen(list_name), line, Some(root));
                self.emit_op(Op::NumLt, line, Some(root));
                let exit_jump = self.emit_op(Op::JumpIfFalse(0), line, Some(root));

                self.emit_get_scalar(counter, line, Some(root));
                self.emit_op(Op::GetArrayElem(list_name), line, Some(root));
                self.emit_set_scalar(underscore, line, Some(root));

                self.compile_expr(expr)?;
                self.emit_op(Op::Pop, line, Some(root));

                self.emit_pre_inc(counter, line, Some(root));
                self.emit_op(Op::Pop, line, Some(root));
                self.emit_op(Op::Jump(loop_start), line, Some(root));
                self.chunk.patch_jump_here(exit_jump);
                self.emit_op(Op::LoadUndef, line, Some(root));
            }

            ExprKind::AlgebraicMatch { subject, arms } => {
                let idx = self
                    .chunk
                    .add_algebraic_match_entry(subject.as_ref().clone(), arms.clone());
                self.emit_op(Op::AlgebraicMatch(idx), line, Some(root));
            }

            // ── Match (regex) ──
            ExprKind::Match {
                expr,
                pattern,
                flags,
                scalar_g,
            } => {
                self.compile_expr(expr)?;
                let pat_idx = self.chunk.add_constant(PerlValue::string(pattern.clone()));
                let flags_idx = self.chunk.add_constant(PerlValue::string(flags.clone()));
                let pos_key_idx = if *scalar_g && flags.contains('g') {
                    if let ExprKind::ScalarVar(n) = &expr.kind {
                        self.chunk.add_constant(PerlValue::string(n.clone()))
                    } else {
                        u16::MAX
                    }
                } else {
                    u16::MAX
                };
                self.emit_op(
                    Op::RegexMatch(pat_idx, flags_idx, *scalar_g, pos_key_idx),
                    line,
                    Some(root),
                );
            }

            ExprKind::Substitution {
                expr,
                pattern,
                replacement,
                flags,
            } => {
                self.compile_expr(expr)?;
                let pat_idx = self.chunk.add_constant(PerlValue::string(pattern.clone()));
                let repl_idx = self
                    .chunk
                    .add_constant(PerlValue::string(replacement.clone()));
                let flags_idx = self.chunk.add_constant(PerlValue::string(flags.clone()));
                let lv_idx = self.chunk.add_lvalue_expr(expr.as_ref().clone());
                self.emit_op(
                    Op::RegexSubst(pat_idx, repl_idx, flags_idx, lv_idx),
                    line,
                    Some(root),
                );
            }
            ExprKind::Transliterate {
                expr,
                from,
                to,
                flags,
            } => {
                self.compile_expr(expr)?;
                let from_idx = self.chunk.add_constant(PerlValue::string(from.clone()));
                let to_idx = self.chunk.add_constant(PerlValue::string(to.clone()));
                let flags_idx = self.chunk.add_constant(PerlValue::string(flags.clone()));
                let lv_idx = self.chunk.add_lvalue_expr(expr.as_ref().clone());
                self.emit_op(
                    Op::RegexTransliterate(from_idx, to_idx, flags_idx, lv_idx),
                    line,
                    Some(root),
                );
            }

            // ── Regex literal ──
            ExprKind::Regex(pattern, flags) => {
                if ctx == WantarrayCtx::Void {
                    // Statement context: bare `/pat/;` is `$_ =~ /pat/` (Perl), not a discarded regex object.
                    self.compile_boolean_rvalue_condition(root)?;
                } else {
                    let pat_idx = self.chunk.add_constant(PerlValue::string(pattern.clone()));
                    let flags_idx = self.chunk.add_constant(PerlValue::string(flags.clone()));
                    self.emit_op(Op::LoadRegex(pat_idx, flags_idx), line, Some(root));
                }
            }

            // ── Map/Grep/Sort with blocks ──
            ExprKind::MapExpr { block, list } => {
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                if let Some(k) = crate::map_grep_fast::detect_map_int_mul(block) {
                    self.emit_op(Op::MapIntMul(k), line, Some(root));
                } else {
                    let block_idx = self.chunk.add_block(block.clone());
                    self.emit_op(Op::MapWithBlock(block_idx), line, Some(root));
                }
                if ctx != WantarrayCtx::List {
                    self.emit_op(Op::StackArrayLen, line, Some(root));
                }
            }
            ExprKind::MapExprComma { expr, list } => {
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let idx = self.chunk.add_map_expr_entry(*expr.clone());
                self.emit_op(Op::MapWithExpr(idx), line, Some(root));
                if ctx != WantarrayCtx::List {
                    self.emit_op(Op::StackArrayLen, line, Some(root));
                }
            }
            ExprKind::GrepExpr { block, list } => {
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                if let Some((m, r)) = crate::map_grep_fast::detect_grep_int_mod_eq(block) {
                    self.emit_op(Op::GrepIntModEq(m, r), line, Some(root));
                } else {
                    let block_idx = self.chunk.add_block(block.clone());
                    self.emit_op(Op::GrepWithBlock(block_idx), line, Some(root));
                }
                if ctx != WantarrayCtx::List {
                    self.emit_op(Op::StackArrayLen, line, Some(root));
                }
            }
            ExprKind::GrepExprComma { expr, list } => {
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let idx = self.chunk.add_grep_expr_entry(*expr.clone());
                self.emit_op(Op::GrepWithExpr(idx), line, Some(root));
                if ctx != WantarrayCtx::List {
                    self.emit_op(Op::StackArrayLen, line, Some(root));
                }
            }
            ExprKind::SortExpr { cmp, list } => {
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                match cmp {
                    Some(crate::ast::SortComparator::Block(block)) => {
                        if let Some(mode) = detect_sort_block_fast(block) {
                            let tag = match mode {
                                crate::sort_fast::SortBlockFast::Numeric => 0u8,
                                crate::sort_fast::SortBlockFast::String => 1u8,
                                crate::sort_fast::SortBlockFast::NumericRev => 2u8,
                                crate::sort_fast::SortBlockFast::StringRev => 3u8,
                            };
                            self.emit_op(Op::SortWithBlockFast(tag), line, Some(root));
                        } else {
                            let block_idx = self.chunk.add_block(block.clone());
                            self.emit_op(Op::SortWithBlock(block_idx), line, Some(root));
                        }
                    }
                    Some(crate::ast::SortComparator::Code(code_expr)) => {
                        self.compile_expr(code_expr)?;
                        self.emit_op(Op::SortWithCodeComparator(ctx.as_byte()), line, Some(root));
                    }
                    None => {
                        self.emit_op(Op::SortNoBlock, line, Some(root));
                    }
                }
            }

            // ── Parallel extensions ──
            ExprKind::PMapExpr {
                block,
                list,
                progress,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.emit_op(Op::PMapWithBlock(block_idx), line, Some(root));
            }
            ExprKind::PMapChunkedExpr {
                chunk_size,
                block,
                list,
                progress,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                self.compile_expr(chunk_size)?;
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.emit_op(Op::PMapChunkedWithBlock(block_idx), line, Some(root));
            }
            ExprKind::PGrepExpr {
                block,
                list,
                progress,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.emit_op(Op::PGrepWithBlock(block_idx), line, Some(root));
            }
            ExprKind::PForExpr {
                block,
                list,
                progress,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.emit_op(Op::PForWithBlock(block_idx), line, Some(root));
            }
            ExprKind::ParLinesExpr {
                path,
                callback,
                progress,
            } => {
                let idx = self.chunk.add_par_lines_entry(
                    path.as_ref().clone(),
                    callback.as_ref().clone(),
                    progress.as_ref().map(|p| p.as_ref().clone()),
                );
                self.emit_op(Op::ParLines(idx), line, Some(root));
            }
            ExprKind::ParWalkExpr {
                path,
                callback,
                progress,
            } => {
                let idx = self.chunk.add_par_walk_entry(
                    path.as_ref().clone(),
                    callback.as_ref().clone(),
                    progress.as_ref().map(|p| p.as_ref().clone()),
                );
                self.emit_op(Op::ParWalk(idx), line, Some(root));
            }
            ExprKind::PwatchExpr { path, callback } => {
                let idx = self
                    .chunk
                    .add_pwatch_entry(path.as_ref().clone(), callback.as_ref().clone());
                self.emit_op(Op::Pwatch(idx), line, Some(root));
            }
            ExprKind::PSortExpr {
                cmp,
                list,
                progress,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                if let Some(block) = cmp {
                    if let Some(mode) = detect_sort_block_fast(block) {
                        let tag = match mode {
                            crate::sort_fast::SortBlockFast::Numeric => 0u8,
                            crate::sort_fast::SortBlockFast::String => 1u8,
                            crate::sort_fast::SortBlockFast::NumericRev => 2u8,
                            crate::sort_fast::SortBlockFast::StringRev => 3u8,
                        };
                        self.emit_op(Op::PSortWithBlockFast(tag), line, Some(root));
                    } else {
                        let block_idx = self.chunk.add_block(block.clone());
                        self.emit_op(Op::PSortWithBlock(block_idx), line, Some(root));
                    }
                } else {
                    self.emit_op(Op::PSortNoBlockParallel, line, Some(root));
                }
            }
            ExprKind::ReduceExpr { block, list } => {
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.emit_op(Op::ReduceWithBlock(block_idx), line, Some(root));
            }
            ExprKind::PReduceExpr {
                block,
                list,
                progress,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.emit_op(Op::PReduceWithBlock(block_idx), line, Some(root));
            }
            ExprKind::PReduceInitExpr {
                init,
                block,
                list,
                progress,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                self.compile_expr(init)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.emit_op(Op::PReduceInitWithBlock(block_idx), line, Some(root));
            }
            ExprKind::PMapReduceExpr {
                map_block,
                reduce_block,
                list,
                progress,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let map_idx = self.chunk.add_block(map_block.clone());
                let reduce_idx = self.chunk.add_block(reduce_block.clone());
                self.emit_op(
                    Op::PMapReduceWithBlocks(map_idx, reduce_idx),
                    line,
                    Some(root),
                );
            }
            ExprKind::PcacheExpr {
                block,
                list,
                progress,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                self.compile_expr_ctx(list, WantarrayCtx::List)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.emit_op(Op::PcacheWithBlock(block_idx), line, Some(root));
            }
            ExprKind::PselectExpr { receivers, timeout } => {
                let n = receivers.len();
                if n > u8::MAX as usize {
                    return Err(CompileError::Unsupported(
                        "pselect: too many receivers".into(),
                    ));
                }
                for r in receivers {
                    self.compile_expr(r)?;
                }
                let has_timeout = timeout.is_some();
                if let Some(t) = timeout {
                    self.compile_expr(t)?;
                }
                self.emit_op(
                    Op::Pselect {
                        n_rx: n as u8,
                        has_timeout,
                    },
                    line,
                    Some(root),
                );
            }
            ExprKind::FanExpr {
                count,
                block,
                progress,
                capture,
            } => {
                if let Some(p) = progress {
                    self.compile_expr(p)?;
                } else {
                    self.emit_op(Op::LoadInt(0), line, Some(root));
                }
                let block_idx = self.chunk.add_block(block.clone());
                match (count, capture) {
                    (Some(c), false) => {
                        self.compile_expr(c)?;
                        self.emit_op(Op::FanWithBlock(block_idx), line, Some(root));
                    }
                    (None, false) => {
                        self.emit_op(Op::FanWithBlockAuto(block_idx), line, Some(root));
                    }
                    (Some(c), true) => {
                        self.compile_expr(c)?;
                        self.emit_op(Op::FanCapWithBlock(block_idx), line, Some(root));
                    }
                    (None, true) => {
                        self.emit_op(Op::FanCapWithBlockAuto(block_idx), line, Some(root));
                    }
                }
            }
            ExprKind::AsyncBlock { body } | ExprKind::SpawnBlock { body } => {
                let block_idx = self.chunk.add_block(body.clone());
                self.emit_op(Op::AsyncBlock(block_idx), line, Some(root));
            }
            ExprKind::Trace { body } => {
                let block_idx = self.chunk.add_block(body.clone());
                self.emit_op(Op::TraceBlock(block_idx), line, Some(root));
            }
            ExprKind::Timer { body } => {
                let block_idx = self.chunk.add_block(body.clone());
                self.emit_op(Op::TimerBlock(block_idx), line, Some(root));
            }
            ExprKind::Bench { body, times } => {
                self.compile_expr(times)?;
                let block_idx = self.chunk.add_block(body.clone());
                self.emit_op(Op::BenchBlock(block_idx), line, Some(root));
            }
            ExprKind::Await(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::Await, line, Some(root));
            }
            ExprKind::Slurp(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Slurp as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Capture(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Capture as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Qx(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Readpipe as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::FetchUrl(e) => {
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::FetchUrl as u16, 1),
                    line,
                    Some(root),
                );
            }
            ExprKind::Pchannel { capacity } => {
                if let Some(c) = capacity {
                    self.compile_expr(c)?;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Pchannel as u16, 1),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Pchannel as u16, 0),
                        line,
                        Some(root),
                    );
                }
            }
            ExprKind::RetryBlock { .. }
            | ExprKind::RateLimitBlock { .. }
            | ExprKind::EveryBlock { .. }
            | ExprKind::GenBlock { .. }
            | ExprKind::Yield(_) => {
                return Err(CompileError::Unsupported(
                    "retry/rate_limit/every/gen/yield (tree interpreter only)".into(),
                ));
            }
        }
        Ok(())
    }

    fn compile_string_part(
        &mut self,
        part: &StringPart,
        line: usize,
        parent: Option<&Expr>,
    ) -> Result<(), CompileError> {
        match part {
            StringPart::Literal(s) => {
                let idx = self.chunk.add_constant(PerlValue::string(s.clone()));
                self.emit_op(Op::LoadConst(idx), line, parent);
            }
            StringPart::ScalarVar(name) => {
                let idx = self.chunk.intern_name(name);
                self.emit_get_scalar(idx, line, parent);
            }
            StringPart::ArrayVar(name) => {
                let idx = self.chunk.intern_name(&self.qualify_stash_array_name(name));
                self.emit_op(Op::GetArray(idx), line, parent);
                self.emit_op(Op::ArrayStringifyListSep, line, parent);
            }
            StringPart::Expr(e) => {
                // Interpolation uses list/array values (`$"`), not Perl scalar(@arr) length.
                if matches!(&e.kind, ExprKind::ArraySlice { .. })
                    || matches!(
                        &e.kind,
                        ExprKind::Deref {
                            kind: Sigil::Array,
                            ..
                        }
                    )
                {
                    self.compile_expr_ctx(e, WantarrayCtx::List)?;
                    self.emit_op(Op::ArrayStringifyListSep, line, parent);
                } else {
                    self.compile_expr(e)?;
                }
            }
        }
        Ok(())
    }

    fn compile_assign(
        &mut self,
        target: &Expr,
        line: usize,
        keep: bool,
        ast: Option<&Expr>,
    ) -> Result<(), CompileError> {
        match &target.kind {
            ExprKind::ScalarVar(name) => {
                self.check_strict_scalar_access(name, line)?;
                self.check_scalar_mutable(name, line)?;
                let idx = self.chunk.intern_name(name);
                if keep {
                    self.emit_set_scalar_keep(idx, line, ast);
                } else {
                    self.emit_set_scalar(idx, line, ast);
                }
            }
            ExprKind::ArrayVar(name) => {
                self.check_strict_array_access(name, line)?;
                let q = self.qualify_stash_array_name(name);
                self.check_array_mutable(&q, line)?;
                let idx = self.chunk.intern_name(&q);
                self.emit_op(Op::SetArray(idx), line, ast);
                if keep {
                    self.emit_op(Op::GetArray(idx), line, ast);
                }
            }
            ExprKind::HashVar(name) => {
                self.check_strict_hash_access(name, line)?;
                self.check_hash_mutable(name, line)?;
                let idx = self.chunk.intern_name(name);
                self.emit_op(Op::SetHash(idx), line, ast);
                if keep {
                    self.emit_op(Op::GetHash(idx), line, ast);
                }
            }
            ExprKind::ArrayElement { array, index } => {
                self.check_strict_array_access(array, line)?;
                let q = self.qualify_stash_array_name(array);
                self.check_array_mutable(&q, line)?;
                let idx = self.chunk.intern_name(&q);
                self.compile_expr(index)?;
                self.emit_op(Op::SetArrayElem(idx), line, ast);
            }
            ExprKind::ArraySlice { array, indices } => {
                if indices.is_empty() {
                    if self.is_mysync_array(array) {
                        return Err(CompileError::Unsupported(
                            "mysync array slice assign (tree interpreter)".into(),
                        ));
                    }
                    self.check_strict_array_access(array, line)?;
                    let q = self.qualify_stash_array_name(array);
                    self.check_array_mutable(&q, line)?;
                    let arr_idx = self.chunk.intern_name(&q);
                    self.emit_op(Op::SetNamedArraySlice(arr_idx, 0), line, ast);
                    if keep {
                        self.emit_op(Op::MakeArray(0), line, ast);
                    }
                    return Ok(());
                }
                if self.is_mysync_array(array) {
                    return Err(CompileError::Unsupported(
                        "mysync array slice assign (tree interpreter)".into(),
                    ));
                }
                self.check_strict_array_access(array, line)?;
                let q = self.qualify_stash_array_name(array);
                self.check_array_mutable(&q, line)?;
                let arr_idx = self.chunk.intern_name(&q);
                for ix in indices {
                    self.compile_array_slice_index_expr(ix)?;
                }
                self.emit_op(
                    Op::SetNamedArraySlice(arr_idx, indices.len() as u16),
                    line,
                    ast,
                );
                if keep {
                    for (ix, index_expr) in indices.iter().enumerate() {
                        self.compile_array_slice_index_expr(index_expr)?;
                        self.emit_op(Op::ArraySlicePart(arr_idx), line, ast);
                        if ix > 0 {
                            self.emit_op(Op::ArrayConcatTwo, line, ast);
                        }
                    }
                }
                return Ok(());
            }
            ExprKind::HashElement { hash, key } => {
                self.check_strict_hash_access(hash, line)?;
                self.check_hash_mutable(hash, line)?;
                let idx = self.chunk.intern_name(hash);
                self.compile_expr(key)?;
                self.emit_op(Op::SetHashElem(idx), line, ast);
            }
            ExprKind::HashSlice { hash, keys } => {
                if keys.is_empty() {
                    if self.is_mysync_hash(hash) {
                        return Err(CompileError::Unsupported(
                            "mysync hash slice assign (tree interpreter)".into(),
                        ));
                    }
                    self.check_strict_hash_access(hash, line)?;
                    self.check_hash_mutable(hash, line)?;
                    let hash_idx = self.chunk.intern_name(hash);
                    self.emit_op(Op::SetHashSlice(hash_idx, 0), line, ast);
                    if keep {
                        self.emit_op(Op::MakeArray(0), line, ast);
                    }
                    return Ok(());
                }
                if self.is_mysync_hash(hash) {
                    return Err(CompileError::Unsupported(
                        "mysync hash slice assign (tree interpreter)".into(),
                    ));
                }
                self.check_strict_hash_access(hash, line)?;
                self.check_hash_mutable(hash, line)?;
                let hash_idx = self.chunk.intern_name(hash);
                for key_expr in keys {
                    self.compile_expr(key_expr)?;
                }
                self.emit_op(Op::SetHashSlice(hash_idx, keys.len() as u16), line, ast);
                if keep {
                    for key_expr in keys {
                        self.compile_expr(key_expr)?;
                        self.emit_op(Op::GetHashElem(hash_idx), line, ast);
                    }
                    self.emit_op(Op::MakeArray(keys.len() as u16), line, ast);
                }
                return Ok(());
            }
            ExprKind::Deref {
                expr,
                kind: Sigil::Scalar,
            } => {
                self.compile_expr(expr)?;
                if keep {
                    self.emit_op(Op::SetSymbolicScalarRefKeep, line, ast);
                } else {
                    self.emit_op(Op::SetSymbolicScalarRef, line, ast);
                }
            }
            ExprKind::Deref {
                expr,
                kind: Sigil::Array,
            } => {
                self.compile_expr(expr)?;
                self.emit_op(Op::SetSymbolicArrayRef, line, ast);
            }
            ExprKind::Deref {
                expr,
                kind: Sigil::Hash,
            } => {
                self.compile_expr(expr)?;
                self.emit_op(Op::SetSymbolicHashRef, line, ast);
            }
            ExprKind::Deref {
                expr,
                kind: Sigil::Typeglob,
            } => {
                self.compile_expr(expr)?;
                self.emit_op(Op::SetSymbolicTypeglobRef, line, ast);
            }
            ExprKind::Typeglob(name) => {
                let idx = self.chunk.intern_name(name);
                if keep {
                    self.emit_op(Op::TypeglobAssignFromValue(idx), line, ast);
                } else {
                    return Err(CompileError::Unsupported(
                        "typeglob assign without keep (internal)".into(),
                    ));
                }
            }
            ExprKind::AnonymousListSlice { source, indices } => {
                if let ExprKind::Deref {
                    expr: inner,
                    kind: Sigil::Array,
                } = &source.kind
                {
                    if indices.is_empty() {
                        return Err(CompileError::Unsupported(
                            "assign to empty list slice (internal)".into(),
                        ));
                    }
                    self.compile_arrow_array_base_expr(inner)?;
                    for ix in indices {
                        self.compile_array_slice_index_expr(ix)?;
                    }
                    self.emit_op(Op::SetArrowArraySlice(indices.len() as u16), line, ast);
                    if keep {
                        self.compile_arrow_array_base_expr(inner)?;
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(Op::ArrowArraySlice(indices.len() as u16), line, ast);
                    }
                    return Ok(());
                }
                return Err(CompileError::Unsupported(
                    "assign to anonymous list slice (non-@array-deref base)".into(),
                ));
            }
            ExprKind::ArrowDeref {
                expr,
                index,
                kind: DerefKind::Hash,
            } => {
                self.compile_arrow_hash_base_expr(expr)?;
                self.compile_expr(index)?;
                if keep {
                    self.emit_op(Op::SetArrowHashKeep, line, ast);
                } else {
                    self.emit_op(Op::SetArrowHash, line, ast);
                }
            }
            ExprKind::ArrowDeref {
                expr,
                index,
                kind: DerefKind::Array,
            } => {
                if let ExprKind::List(indices) = &index.kind {
                    // Multi-index slice assignment: RHS value is already on the stack (pushed
                    // by the enclosing `compile_expr(value)` before `compile_assign` was called
                    // with keep = true). `SetArrowArraySlice` delegates to
                    // `Interpreter::assign_arrow_array_slice` for element-wise write.
                    self.compile_arrow_array_base_expr(expr)?;
                    for ix in indices {
                        self.compile_array_slice_index_expr(ix)?;
                    }
                    self.emit_op(Op::SetArrowArraySlice(indices.len() as u16), line, ast);
                    if keep {
                        // The Set op pops the value; keep callers re-read via a fresh slice read.
                        self.compile_arrow_array_base_expr(expr)?;
                        for ix in indices {
                            self.compile_array_slice_index_expr(ix)?;
                        }
                        self.emit_op(Op::ArrowArraySlice(indices.len() as u16), line, ast);
                    }
                    return Ok(());
                }
                if arrow_deref_arrow_subscript_is_plain_scalar_index(index) {
                    self.compile_arrow_array_base_expr(expr)?;
                    self.compile_expr(index)?;
                    if keep {
                        self.emit_op(Op::SetArrowArrayKeep, line, ast);
                    } else {
                        self.emit_op(Op::SetArrowArray, line, ast);
                    }
                } else {
                    self.compile_arrow_array_base_expr(expr)?;
                    self.compile_array_slice_index_expr(index)?;
                    self.emit_op(Op::SetArrowArraySlice(1), line, ast);
                    if keep {
                        self.compile_arrow_array_base_expr(expr)?;
                        self.compile_array_slice_index_expr(index)?;
                        self.emit_op(Op::ArrowArraySlice(1), line, ast);
                    }
                }
            }
            ExprKind::ArrowDeref {
                kind: DerefKind::Call,
                ..
            } => {
                return Err(CompileError::Unsupported(
                    "Assign to arrow call deref (tree interpreter)".into(),
                ));
            }
            ExprKind::HashSliceDeref { container, keys } => {
                self.compile_expr(container)?;
                for key_expr in keys {
                    self.compile_expr(key_expr)?;
                }
                self.emit_op(Op::SetHashSliceDeref(keys.len() as u16), line, ast);
            }
            ExprKind::Pos(inner) => {
                let Some(inner_e) = inner.as_ref() else {
                    return Err(CompileError::Unsupported(
                        "assign to pos() without scalar".into(),
                    ));
                };
                if keep {
                    self.emit_op(Op::Dup, line, ast);
                }
                match &inner_e.kind {
                    ExprKind::ScalarVar(name) => {
                        let idx = self.chunk.add_constant(PerlValue::string(name.clone()));
                        self.emit_op(Op::LoadConst(idx), line, ast);
                    }
                    _ => {
                        self.compile_expr(inner_e)?;
                    }
                }
                self.emit_op(Op::SetRegexPos, line, ast);
            }
            _ => {
                return Err(CompileError::Unsupported("Assign to complex lvalue".into()));
            }
        }
        Ok(())
    }
}

/// Map a binary op to its stack opcode for compound assignment on aggregates (`$a[$i]`, `$h{$k}`).
pub(crate) fn binop_to_vm_op(op: BinOp) -> Option<Op> {
    Some(match op {
        BinOp::Add => Op::Add,
        BinOp::Sub => Op::Sub,
        BinOp::Mul => Op::Mul,
        BinOp::Div => Op::Div,
        BinOp::Mod => Op::Mod,
        BinOp::Pow => Op::Pow,
        BinOp::Concat => Op::Concat,
        BinOp::BitAnd => Op::BitAnd,
        BinOp::BitOr => Op::BitOr,
        BinOp::BitXor => Op::BitXor,
        BinOp::ShiftLeft => Op::Shl,
        BinOp::ShiftRight => Op::Shr,
        _ => return None,
    })
}

/// Encode/decode scalar compound ops for [`Op::ScalarCompoundAssign`].
pub(crate) fn scalar_compound_op_to_byte(op: BinOp) -> Option<u8> {
    Some(match op {
        BinOp::Add => 0,
        BinOp::Sub => 1,
        BinOp::Mul => 2,
        BinOp::Div => 3,
        BinOp::Mod => 4,
        BinOp::Pow => 5,
        BinOp::Concat => 6,
        BinOp::BitAnd => 7,
        BinOp::BitOr => 8,
        BinOp::BitXor => 9,
        BinOp::ShiftLeft => 10,
        BinOp::ShiftRight => 11,
        _ => return None,
    })
}

pub(crate) fn scalar_compound_op_from_byte(b: u8) -> Option<BinOp> {
    Some(match b {
        0 => BinOp::Add,
        1 => BinOp::Sub,
        2 => BinOp::Mul,
        3 => BinOp::Div,
        4 => BinOp::Mod,
        5 => BinOp::Pow,
        6 => BinOp::Concat,
        7 => BinOp::BitAnd,
        8 => BinOp::BitOr,
        9 => BinOp::BitXor,
        10 => BinOp::ShiftLeft,
        11 => BinOp::ShiftRight,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{BuiltinId, Op, GP_RUN};
    use crate::parse;

    fn compile_snippet(code: &str) -> Result<Chunk, CompileError> {
        let program = parse(code).expect("parse snippet");
        Compiler::new().compile_program(&program)
    }

    fn assert_last_halt(chunk: &Chunk) {
        assert!(
            matches!(chunk.ops.last(), Some(Op::Halt)),
            "expected Halt last, got {:?}",
            chunk.ops.last()
        );
    }

    #[test]
    fn compile_empty_program_emits_run_phase_then_halt() {
        let chunk = compile_snippet("").expect("compile");
        assert_eq!(chunk.ops.len(), 2);
        assert!(matches!(chunk.ops[0], Op::SetGlobalPhase(p) if p == GP_RUN));
        assert!(matches!(chunk.ops[1], Op::Halt));
    }

    #[test]
    fn compile_integer_literal_statement() {
        let chunk = compile_snippet("42;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::LoadInt(42))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_pos_assign_emits_set_regex_pos() {
        let chunk = compile_snippet(r#"$_ = ""; pos = 3;"#).expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::SetRegexPos)),
            "expected SetRegexPos in {:?}",
            chunk.ops
        );
    }

    #[test]
    fn compile_pos_deref_scalar_assign_emits_set_regex_pos() {
        let chunk = compile_snippet(
            r#"no strict 'vars';
            my $s;
            my $r = \$s;
            pos $$r = 0;"#,
        )
        .expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::SetRegexPos)),
            r"expected SetRegexPos for pos $$r =, got {:?}",
            chunk.ops
        );
    }

    #[test]
    fn compile_map_expr_comma_emits_map_with_expr() {
        let chunk = compile_snippet(
            r#"no strict 'vars';
            join(",", map $_ + 1, (4, 5));"#,
        )
        .expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::MapWithExpr(_))),
            "expected MapWithExpr, got {:?}",
            chunk.ops
        );
    }

    #[test]
    fn compile_hash_slice_deref_assign_emits_set_op() {
        let code = r#"no strict 'vars';
        my $h = { "a" => 1, "b" => 2 };
        my $r = $h;
        @$r{"a", "b"} = (10, 20);
        $r->{"a"} . "," . $r->{"b"};"#;
        let chunk = compile_snippet(code).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::SetHashSliceDeref(n) if *n == 2)),
            "expected SetHashSliceDeref(2), got {:?}",
            chunk.ops
        );
    }

    #[test]
    fn compile_bare_array_assign_diamond_uses_readline_list() {
        let chunk = compile_snippet("@a = <>;").expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(
                o,
                Op::CallBuiltin(bid, 0) if *bid == BuiltinId::ReadLineList as u16
            )),
            "expected ReadLineList for bare @a = <>, got {:?}",
            chunk.ops
        );
    }

    #[test]
    fn compile_float_literal() {
        let chunk = compile_snippet("3.25;").expect("compile");
        assert!(chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::LoadFloat(f) if (*f - 3.25).abs() < 1e-9)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_addition() {
        let chunk = compile_snippet("1 + 2;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Add)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_sub_mul_div_mod_pow() {
        for (src, op) in [
            ("10 - 3;", "Sub"),
            ("6 * 7;", "Mul"),
            ("8 / 2;", "Div"),
            ("9 % 4;", "Mod"),
            ("2 ** 8;", "Pow"),
        ] {
            let chunk = compile_snippet(src).expect(src);
            assert!(
                chunk.ops.iter().any(|o| std::mem::discriminant(o) == {
                    let dummy = match op {
                        "Sub" => Op::Sub,
                        "Mul" => Op::Mul,
                        "Div" => Op::Div,
                        "Mod" => Op::Mod,
                        "Pow" => Op::Pow,
                        _ => unreachable!(),
                    };
                    std::mem::discriminant(&dummy)
                }),
                "{} missing {:?}",
                src,
                op
            );
            assert_last_halt(&chunk);
        }
    }

    #[test]
    fn compile_string_literal_uses_constant_pool() {
        let chunk = compile_snippet(r#""hello";"#).expect("compile");
        assert!(chunk
            .constants
            .iter()
            .any(|c| c.as_str().as_deref() == Some("hello")));
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::LoadConst(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_substitution_bind_emits_regex_subst() {
        let chunk = compile_snippet(r#"my $s = "aa"; $s =~ s/a/b/g;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::RegexSubst(_, _, _, _))),
            "expected RegexSubst in {:?}",
            chunk.ops
        );
        assert!(!chunk.lvalues.is_empty());
    }

    #[test]
    fn compile_chomp_emits_chomp_in_place() {
        let chunk = compile_snippet(r#"my $s = "x\n"; chomp $s;"#).expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::ChompInPlace(_))),
            "expected ChompInPlace, got {:?}",
            chunk.ops
        );
    }

    #[test]
    fn compile_transliterate_bind_emits_regex_transliterate() {
        let chunk = compile_snippet(r#"my $u = "abc"; $u =~ tr/a-z/A-Z/;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::RegexTransliterate(_, _, _, _))),
            "expected RegexTransliterate in {:?}",
            chunk.ops
        );
        assert!(!chunk.lvalues.is_empty());
    }

    #[test]
    fn compile_negation() {
        let chunk = compile_snippet("-7;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Negate)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_my_scalar_declares() {
        let chunk = compile_snippet("my $x = 1;").expect("compile");
        assert!(chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::DeclareScalar(_) | Op::DeclareScalarSlot(_, _))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_scalar_fetch_and_assign() {
        let chunk = compile_snippet("my $a = 1; $a + 0;").expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .filter(|o| matches!(
                    o,
                    Op::GetScalar(_) | Op::GetScalarPlain(_) | Op::GetScalarSlot(_)
                ))
                .count()
                >= 1
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_plain_scalar_read_emits_get_scalar_plain() {
        let chunk = compile_snippet("my $a = 1; $a + 0;").expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::GetScalarPlain(_) | Op::GetScalarSlot(_))),
            "expected GetScalarPlain or GetScalarSlot for non-special $a, ops={:?}",
            chunk.ops
        );
    }

    #[test]
    fn compile_sub_postfix_inc_emits_post_inc_slot() {
        let chunk = compile_snippet("sub f { my $x = 0; $x++; return $x; }").expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::PostIncSlot(_))),
            "expected PostIncSlot in compiled sub body, ops={:?}",
            chunk.ops
        );
    }

    #[test]
    fn compile_comparison_ops_numeric() {
        for src in [
            "1 < 2;", "1 > 2;", "1 <= 2;", "1 >= 2;", "1 == 2;", "1 != 2;",
        ] {
            let chunk = compile_snippet(src).expect(src);
            assert!(
                chunk.ops.iter().any(|o| {
                    matches!(
                        o,
                        Op::NumLt | Op::NumGt | Op::NumLe | Op::NumGe | Op::NumEq | Op::NumNe
                    )
                }),
                "{}",
                src
            );
            assert_last_halt(&chunk);
        }
    }

    #[test]
    fn compile_string_compare_ops() {
        for src in [
            r#"'a' lt 'b';"#,
            r#"'a' gt 'b';"#,
            r#"'a' le 'b';"#,
            r#"'a' ge 'b';"#,
        ] {
            let chunk = compile_snippet(src).expect(src);
            assert!(
                chunk
                    .ops
                    .iter()
                    .any(|o| matches!(o, Op::StrLt | Op::StrGt | Op::StrLe | Op::StrGe)),
                "{}",
                src
            );
            assert_last_halt(&chunk);
        }
    }

    #[test]
    fn compile_concat() {
        let chunk = compile_snippet(r#"'a' . 'b';"#).expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Concat)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_bitwise_ops() {
        let chunk = compile_snippet("1 & 2 | 3 ^ 4;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::BitAnd)));
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::BitOr)));
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::BitXor)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_shift_right() {
        // Note: bare `<<` is tokenized as heredoc start, not binary shift — see lexer.
        let chunk = compile_snippet("8 >> 1;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Shr)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_log_not_and_bit_not() {
        let c1 = compile_snippet("!0;").expect("compile");
        assert!(c1.ops.iter().any(|o| matches!(o, Op::LogNot)));
        let c2 = compile_snippet("~0;").expect("compile");
        assert!(c2.ops.iter().any(|o| matches!(o, Op::BitNot)));
    }

    #[test]
    fn compile_sub_registers_name_and_entry() {
        let chunk = compile_snippet("sub foo { return 1; }").expect("compile");
        assert!(chunk.names.iter().any(|n| n == "foo"));
        assert!(chunk
            .sub_entries
            .iter()
            .any(|&(idx, ip, _)| chunk.names[idx as usize] == "foo" && ip > 0));
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Halt)));
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::ReturnValue)));
    }

    #[test]
    fn compile_postinc_scalar() {
        let chunk = compile_snippet("my $n = 1; $n++;").expect("compile");
        assert!(chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::PostInc(_) | Op::PostIncSlot(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_preinc_scalar() {
        let chunk = compile_snippet("my $n = 1; ++$n;").expect("compile");
        assert!(chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::PreInc(_) | Op::PreIncSlot(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_if_expression_value() {
        let chunk = compile_snippet("if (1) { 2 } else { 3 }").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::JumpIfFalse(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_unless_expression_value() {
        let chunk = compile_snippet("unless (0) { 1 } else { 2 }").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::JumpIfFalse(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_array_declare_and_push() {
        let chunk = compile_snippet("my @a; push @a, 1;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::DeclareArray(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_ternary() {
        let chunk = compile_snippet("1 ? 2 : 3;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::JumpIfFalse(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_repeat_operator() {
        let chunk = compile_snippet(r#"'ab' x 3;"#).expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::StringRepeat)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_range_to_array() {
        let chunk = compile_snippet("my @a = (1..3);").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Range)));
        assert_last_halt(&chunk);
    }

    /// Scalar `..` / `...` in a boolean condition must be the flip-flop (`$.`), not a list range.
    #[test]
    fn compile_print_if_uses_scalar_flipflop_not_range_list() {
        let chunk = compile_snippet("print if 1..2;").expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::ScalarFlipFlop(_, 0))),
            "expected ScalarFlipFlop in bytecode, got:\n{}",
            chunk.disassemble()
        );
        assert!(
            !chunk.ops.iter().any(|o| matches!(o, Op::Range)),
            "did not expect list Range op in scalar if-condition:\n{}",
            chunk.disassemble()
        );
    }

    #[test]
    fn compile_print_if_three_dot_scalar_flipflop_sets_exclusive_flag() {
        let chunk = compile_snippet("print if 1...2;").expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::ScalarFlipFlop(_, 1))),
            "expected ScalarFlipFlop(..., exclusive=1), got:\n{}",
            chunk.disassemble()
        );
    }

    #[test]
    fn compile_regex_flipflop_two_dot_emits_regex_flipflop_op() {
        let chunk = compile_snippet(r#"print if /a/../b/;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::RegexFlipFlop(_, 0, _, _, _, _))),
            "expected RegexFlipFlop(.., exclusive=0), got:\n{}",
            chunk.disassemble()
        );
        assert!(
            !chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::ScalarFlipFlop(_, _))),
            "regex flip-flop must not use ScalarFlipFlop:\n{}",
            chunk.disassemble()
        );
    }

    #[test]
    fn compile_regex_flipflop_three_dot_sets_exclusive_flag() {
        let chunk = compile_snippet(r#"print if /a/.../b/;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::RegexFlipFlop(_, 1, _, _, _, _))),
            "expected RegexFlipFlop(..., exclusive=1), got:\n{}",
            chunk.disassemble()
        );
    }

    #[test]
    fn compile_regex_eof_flipflop_emits_regex_eof_flipflop_op() {
        let chunk = compile_snippet(r#"print if /a/..eof;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::RegexEofFlipFlop(_, 0, _, _))),
            "expected RegexEofFlipFlop(.., exclusive=0), got:\n{}",
            chunk.disassemble()
        );
        assert!(
            !chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::ScalarFlipFlop(_, _))),
            "regex/eof flip-flop must not use ScalarFlipFlop:\n{}",
            chunk.disassemble()
        );
    }

    #[test]
    fn compile_regex_eof_flipflop_three_dot_sets_exclusive_flag() {
        let chunk = compile_snippet(r#"print if /a/...eof;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::RegexEofFlipFlop(_, 1, _, _))),
            "expected RegexEofFlipFlop(..., exclusive=1), got:\n{}",
            chunk.disassemble()
        );
    }

    #[test]
    fn compile_regex_flipflop_compound_rhs_emits_regex_flip_flop_expr_rhs() {
        let chunk = compile_snippet(r#"print if /a/...(/b/ or /c/);"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::RegexFlipFlopExprRhs(_, _, _, _, _))),
            "expected RegexFlipFlopExprRhs for compound RHS, got:\n{}",
            chunk.disassemble()
        );
        assert!(
            !chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::ScalarFlipFlop(_, _))),
            "compound regex flip-flop must not use ScalarFlipFlop:\n{}",
            chunk.disassemble()
        );
    }

    #[test]
    fn compile_print_statement() {
        let chunk = compile_snippet("print 1;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Print(_, _))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_say_statement() {
        let chunk = compile_snippet("say 1;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Say(_, _))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_defined_builtin() {
        let chunk = compile_snippet("defined 1;").expect("compile");
        assert!(chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::CallBuiltin(id, _) if *id == BuiltinId::Defined as u16)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_length_builtin() {
        let chunk = compile_snippet("length 'abc';").expect("compile");
        assert!(chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::CallBuiltin(id, _) if *id == BuiltinId::Length as u16)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_complex_expr_parentheses() {
        let chunk = compile_snippet("(1 + 2) * (3 + 4);").expect("compile");
        assert!(chunk.ops.iter().filter(|o| matches!(o, Op::Add)).count() >= 2);
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Mul)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_undef_literal() {
        let chunk = compile_snippet("undef;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::LoadUndef)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_empty_statement_semicolons() {
        let chunk = compile_snippet(";;;").expect("compile");
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_array_elem_preinc_uses_rot_and_set_elem() {
        let chunk = compile_snippet("my @a; $a[0] = 0; ++$a[0];").expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::Rot)),
            "expected Rot in {:?}",
            chunk.ops
        );
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::SetArrayElem(_))),
            "expected SetArrayElem in {:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_hash_elem_compound_assign_uses_rot() {
        let chunk = compile_snippet("my %h; $h{0} = 1; $h{0} += 2;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Rot)));
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::SetHashElem(_))),
            "expected SetHashElem"
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_postfix_inc_array_elem_emits_rot() {
        let chunk = compile_snippet("my @a; $a[1] = 5; $a[1]++;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Rot)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_tie_stmt_emits_op_tie() {
        let chunk = compile_snippet("tie %h, 'Pkg';").expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::Tie { .. })),
            "expected Op::Tie in {:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_format_decl_emits_format_decl_op() {
        let chunk = compile_snippet(
            r#"
format FMT =
literal line
.
1;
"#,
        )
        .expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::FormatDecl(0))),
            "expected Op::FormatDecl(0), got {:?}",
            chunk.ops
        );
        assert_eq!(chunk.format_decls.len(), 1);
        assert_eq!(chunk.format_decls[0].0, "FMT");
        assert_eq!(chunk.format_decls[0].1, vec!["literal line".to_string()]);
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_interpolated_string_scalar_only_emits_empty_prefix_and_concat() {
        let chunk = compile_snippet(r#"no strict 'vars'; my $x = 1; "$x";"#).expect("compile");
        let empty_idx = chunk
            .constants
            .iter()
            .position(|c| c.as_str().is_some_and(|s| s.is_empty()))
            .expect("empty string in pool") as u16;
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LoadConst(i) if *i == empty_idx)),
            "expected LoadConst(\"\"), ops={:?}",
            chunk.ops
        );
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::Concat)),
            "expected Op::Concat for qq with only a scalar part, ops={:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_interpolated_string_array_only_emits_stringify_and_concat() {
        let chunk = compile_snippet(r#"no strict 'vars'; my @a = (1, 2); "@a";"#).expect("compile");
        let empty_idx = chunk
            .constants
            .iter()
            .position(|c| c.as_str().is_some_and(|s| s.is_empty()))
            .expect("empty string in pool") as u16;
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LoadConst(i) if *i == empty_idx)),
            "expected LoadConst(\"\"), ops={:?}",
            chunk.ops
        );
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::ArrayStringifyListSep)),
            "expected ArrayStringifyListSep for array var in qq, ops={:?}",
            chunk.ops
        );
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::Concat)),
            "expected Op::Concat after array stringify, ops={:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_interpolated_string_hash_element_only_emits_empty_prefix_and_concat() {
        let chunk =
            compile_snippet(r#"no strict 'vars'; my %h = (k => 1); "$h{k}";"#).expect("compile");
        let empty_idx = chunk
            .constants
            .iter()
            .position(|c| c.as_str().is_some_and(|s| s.is_empty()))
            .expect("empty string in pool") as u16;
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LoadConst(i) if *i == empty_idx)),
            "expected LoadConst(\"\"), ops={:?}",
            chunk.ops
        );
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::Concat)),
            "expected Op::Concat for qq with only an expr part, ops={:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_interpolated_string_leading_literal_has_no_empty_string_prefix() {
        let chunk = compile_snippet(r#"no strict 'vars'; my $x = 1; "a$x";"#).expect("compile");
        assert!(
            !chunk
                .constants
                .iter()
                .any(|c| c.as_str().is_some_and(|s| s.is_empty())),
            "literal-first qq must not intern \"\" (only non-literal first parts need it), ops={:?}",
            chunk.ops
        );
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::Concat)),
            "expected Op::Concat after literal + scalar, ops={:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_interpolated_string_two_scalars_empty_prefix_and_two_concats() {
        let chunk =
            compile_snippet(r#"no strict 'vars'; my $a = 1; my $b = 2; "$a$b";"#).expect("compile");
        let empty_idx = chunk
            .constants
            .iter()
            .position(|c| c.as_str().is_some_and(|s| s.is_empty()))
            .expect("empty string in pool") as u16;
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LoadConst(i) if *i == empty_idx)),
            "expected LoadConst(\"\") before first scalar qq part, ops={:?}",
            chunk.ops
        );
        let n_concat = chunk.ops.iter().filter(|o| matches!(o, Op::Concat)).count();
        assert!(
            n_concat >= 2,
            "expected at least two Op::Concat for two scalar qq parts, got {} in {:?}",
            n_concat,
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_interpolated_string_literal_then_two_scalars_has_no_empty_prefix() {
        let chunk = compile_snippet(r#"no strict 'vars'; my $x = 7; my $y = 8; "p$x$y";"#)
            .expect("compile");
        assert!(
            !chunk
                .constants
                .iter()
                .any(|c| c.as_str().is_some_and(|s| s.is_empty())),
            "literal-first qq must not intern empty string, ops={:?}",
            chunk.ops
        );
        let n_concat = chunk.ops.iter().filter(|o| matches!(o, Op::Concat)).count();
        assert!(
            n_concat >= 2,
            "expected two Concats for literal + two scalars, got {} in {:?}",
            n_concat,
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_interpolated_string_braced_scalar_trailing_literal_emits_concats() {
        let chunk = compile_snippet(r#"no strict 'vars'; my $u = 1; "a${u}z";"#).expect("compile");
        let n_concat = chunk.ops.iter().filter(|o| matches!(o, Op::Concat)).count();
        assert!(
            n_concat >= 2,
            "expected braced scalar + trailing literal to use multiple Concats, got {} in {:?}",
            n_concat,
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_interpolated_string_braced_scalar_sandwiched_emits_concats() {
        let chunk = compile_snippet(r#"no strict 'vars'; my $u = 1; "L${u}R";"#).expect("compile");
        let n_concat = chunk.ops.iter().filter(|o| matches!(o, Op::Concat)).count();
        assert!(
            n_concat >= 2,
            "expected leading literal + braced scalar + trailing literal to use multiple Concats, got {} in {:?}",
            n_concat,
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_interpolated_string_mixed_braced_and_plain_scalars_emits_concats() {
        let chunk = compile_snippet(r#"no strict 'vars'; my $x = 1; my $y = 2; "a${x}b$y";"#)
            .expect("compile");
        let n_concat = chunk.ops.iter().filter(|o| matches!(o, Op::Concat)).count();
        assert!(
            n_concat >= 3,
            "expected literal/braced/plain qq mix to use at least three Concats, got {} in {:?}",
            n_concat,
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_use_overload_emits_use_overload_op() {
        let chunk = compile_snippet(r#"use overload '""' => 'as_string';"#).expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::UseOverload(0))),
            "expected Op::UseOverload(0), got {:?}",
            chunk.ops
        );
        assert_eq!(chunk.use_overload_entries.len(), 1);
        // Perl `'""'` is a single-quoted string whose contents are two `"` characters — the
        // overload table key for stringify (see [`Interpreter::overload_stringify_method`]).
        let stringify_key: String = ['"', '"'].iter().collect();
        assert_eq!(
            chunk.use_overload_entries[0],
            vec![(stringify_key, "as_string".to_string())]
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_use_overload_empty_list_emits_use_overload_with_no_pairs() {
        let chunk = compile_snippet(r#"use overload ();"#).expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::UseOverload(0))),
            "expected Op::UseOverload(0), got {:?}",
            chunk.ops
        );
        assert_eq!(chunk.use_overload_entries.len(), 1);
        assert!(chunk.use_overload_entries[0].is_empty());
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_use_overload_multiple_pairs_single_op() {
        let chunk =
            compile_snippet(r#"use overload '+' => 'p_add', '-' => 'p_sub';"#).expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::UseOverload(0))),
            "expected Op::UseOverload(0), got {:?}",
            chunk.ops
        );
        assert_eq!(chunk.use_overload_entries.len(), 1);
        assert_eq!(
            chunk.use_overload_entries[0],
            vec![
                ("+".to_string(), "p_add".to_string()),
                ("-".to_string(), "p_sub".to_string()),
            ]
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_open_my_fh_emits_declare_open_set() {
        let chunk = compile_snippet(r#"open my $fh, "<", "/dev/null";"#).expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(
                o,
                Op::CallBuiltin(b, 3) if *b == BuiltinId::Open as u16
            )),
            "expected Open builtin 3-arg, got {:?}",
            chunk.ops
        );
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::SetScalarKeepPlain(_))),
            "expected SetScalarKeepPlain after open"
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_local_hash_element_emits_local_declare_hash_element() {
        let chunk = compile_snippet(r#"local $SIG{__WARN__} = 0;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LocalDeclareHashElement(_))),
            "expected LocalDeclareHashElement in {:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_local_array_element_emits_local_declare_array_element() {
        let chunk = compile_snippet(r#"local $a[2] = 9;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LocalDeclareArrayElement(_))),
            "expected LocalDeclareArrayElement in {:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_local_typeglob_emits_local_declare_typeglob() {
        let chunk = compile_snippet(r#"local *STDOUT;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LocalDeclareTypeglob(_, None))),
            "expected LocalDeclareTypeglob(_, None) in {:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_local_typeglob_alias_emits_local_declare_typeglob_some_rhs() {
        let chunk = compile_snippet(r#"local *FOO = *STDOUT;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LocalDeclareTypeglob(_, Some(_)))),
            "expected LocalDeclareTypeglob with rhs in {:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_local_braced_typeglob_emits_local_declare_typeglob_dynamic() {
        let chunk = compile_snippet(r#"no strict 'refs'; my $g = "STDOUT"; local *{ $g };"#)
            .expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LocalDeclareTypeglobDynamic(None))),
            "expected LocalDeclareTypeglobDynamic(None) in {:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_local_star_deref_typeglob_emits_local_declare_typeglob_dynamic() {
        let chunk =
            compile_snippet(r#"no strict 'refs'; my $g = "STDOUT"; local *$g;"#).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::LocalDeclareTypeglobDynamic(None))),
            "expected LocalDeclareTypeglobDynamic(None) for local *scalar glob in {:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_braced_glob_assign_to_named_glob_emits_copy_dynamic_lhs() {
        // `*{EXPR} = *FOO` — dynamic lhs name + static rhs glob → `CopyTypeglobSlotsDynamicLhs`.
        let chunk = compile_snippet(r#"no strict 'refs'; my $n = "x"; *{ $n } = *STDOUT;"#)
            .expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::CopyTypeglobSlotsDynamicLhs(_))),
            "expected CopyTypeglobSlotsDynamicLhs in {:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }
}
