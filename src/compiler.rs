use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::bench_fusion::{
    try_match_array_push_sort_fusion, try_match_hash_sum_fusion, try_match_map_grep_scalar_fusion,
    try_match_regex_count_fusion, try_match_string_repeat_length_fusion, ArrayPushSortFusionSpec,
    HashSumFusionSpec, MapGrepScalarFusionSpec, RegexCountFusionSpec, StringRepeatLengthFusionSpec,
};
use crate::bytecode::{
    BuiltinId, Chunk, Op, RuntimeSubDecl, GP_CHECK, GP_END, GP_INIT, GP_RUN, GP_START,
};
use crate::interpreter::{Interpreter, WantarrayCtx};
use crate::sort_fast::detect_sort_block_fast;
use crate::value::PerlValue;

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

/// Closed-form fusion for `my $sum = 0; for (my $i = 0; $i < L; $i = $i + 1) { $sum = $sum + $i } print $sum, "\n"`.
struct TriangularForFusionSpec {
    limit: i64,
    sum_name: String,
    i_name: String,
}

fn try_match_triangular_for_fusion(
    sum_stmt: &Statement,
    for_stmt: &Statement,
    print_stmt: &Statement,
) -> Option<TriangularForFusionSpec> {
    if sum_stmt.label.is_some() || for_stmt.label.is_some() || print_stmt.label.is_some() {
        return None;
    }
    let sum_name = match &sum_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Scalar
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            match &decls[0].initializer {
                Some(Expr {
                    kind: ExprKind::Integer(0),
                    ..
                }) => decls[0].name.clone(),
                _ => return None,
            }
        }
        _ => return None,
    };

    let StmtKind::For {
        init,
        condition,
        step,
        body,
        label,
        continue_block,
        ..
    } = &for_stmt.kind
    else {
        return None;
    };
    if label.is_some() || continue_block.is_some() {
        return None;
    }
    let init = init.as_ref()?;
    let i_name = match &init.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Scalar
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            match &decls[0].initializer {
                Some(Expr {
                    kind: ExprKind::Integer(0),
                    ..
                }) => decls[0].name.clone(),
                _ => return None,
            }
        }
        _ => return None,
    };

    let condition = condition.as_ref()?;
    let limit = match &condition.kind {
        ExprKind::BinOp {
            left,
            op: BinOp::NumLt,
            right,
        } => match (&left.kind, &right.kind) {
            (ExprKind::ScalarVar(n), ExprKind::Integer(lim)) if n == &i_name => *lim,
            _ => return None,
        },
        _ => return None,
    };
    if limit < 0 {
        return None;
    }

    let step = step.as_ref()?;
    match &step.kind {
        ExprKind::Assign { target, value } => {
            match &target.kind {
                ExprKind::ScalarVar(n) if n == &i_name => {}
                _ => return None,
            }
            match &value.kind {
                ExprKind::BinOp {
                    left,
                    op: BinOp::Add,
                    right,
                } => match (&left.kind, &right.kind) {
                    (ExprKind::ScalarVar(n), ExprKind::Integer(1)) if n == &i_name => {}
                    _ => return None,
                },
                _ => return None,
            }
        }
        _ => return None,
    }

    if body.len() != 1 {
        return None;
    }
    let body_stmt = &body[0];
    if body_stmt.label.is_some() {
        return None;
    }
    let (target, value) = match &body_stmt.kind {
        StmtKind::Expression(expr) => match &expr.kind {
            ExprKind::Assign { target, value } => (target.as_ref(), value.as_ref()),
            _ => return None,
        },
        _ => return None,
    };
    match &target.kind {
        ExprKind::ScalarVar(n) if n == &sum_name => {}
        _ => return None,
    }
    match &value.kind {
        ExprKind::BinOp {
            left,
            op: BinOp::Add,
            right,
        } => match (&left.kind, &right.kind) {
            (ExprKind::ScalarVar(s), ExprKind::ScalarVar(iv))
                if s == &sum_name && iv == &i_name => {}
            _ => return None,
        },
        _ => return None,
    }

    match &print_stmt.kind {
        StmtKind::Expression(Expr {
            kind: ExprKind::Print { args, handle },
            ..
        }) => {
            if handle.is_some() || args.len() != 2 {
                return None;
            }
            match (&args[0].kind, &args[1].kind) {
                (ExprKind::ScalarVar(s), ExprKind::String(nl)) if s == &sum_name && nl == "\n" => {}
                _ => return None,
            }
        }
        _ => return None,
    }

    Some(TriangularForFusionSpec {
        limit,
        sum_name,
        i_name,
    })
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
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
            ast_expr_intern: HashMap::new(),
            begin_blocks: Vec::new(),
            unit_check_blocks: Vec::new(),
            check_blocks: Vec::new(),
            init_blocks: Vec::new(),
            end_blocks: Vec::new(),
            scope_stack: vec![ScopeLayer::default()],
            current_package: String::new(),
            program_last_stmt_takes_value: false,
            source_file: String::new(),
            frame_depth: 0,
            try_depth: 0,
            loop_stack: Vec::new(),
        }
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

    fn emit_triangular_for_fusion(
        &mut self,
        spec: &TriangularForFusionSpec,
        my_sum_stmt: &Statement,
        for_stmt: &Statement,
        print_stmt: &Statement,
        print_is_last: bool,
    ) -> Result<(), CompileError> {
        self.compile_statement(my_sum_stmt)?;
        let line = for_stmt.line;
        self.emit_push_frame(line);
        let StmtKind::For {
            init: Some(init), ..
        } = &for_stmt.kind
        else {
            return Err(CompileError::Unsupported(
                "triangular fusion: missing for init".into(),
            ));
        };
        self.compile_statement(init.as_ref())?;
        let sum_idx = self.chunk.intern_name(&spec.sum_name);
        let i_idx = self.chunk.intern_name(&spec.i_name);
        self.chunk.emit(
            Op::TriangularForAccum {
                limit: spec.limit,
                sum_name_idx: sum_idx,
                i_name_idx: i_idx,
            },
            line,
        );
        self.emit_pop_frame(line);
        if print_is_last {
            if let StmtKind::Expression(expr) = &print_stmt.kind {
                self.compile_expr(expr)?;
            } else {
                self.compile_statement(print_stmt)?;
            }
        } else {
            self.compile_statement(print_stmt)?;
        }
        Ok(())
    }

    fn emit_fused_print_int_newline(
        &mut self,
        n: i64,
        line: usize,
        print_is_last: bool,
    ) -> Result<(), CompileError> {
        self.chunk.emit(Op::LoadInt(n), line);
        let nl = self.chunk.add_constant(PerlValue::string("\n".to_string()));
        self.chunk.emit(Op::LoadConst(nl), line);
        self.chunk.emit(Op::Print(2), line);
        if !print_is_last {
            self.chunk.emit(Op::Pop, line);
        }
        Ok(())
    }

    fn emit_fused_print_four_words(
        &mut self,
        a: i64,
        space_word: &str,
        b: i64,
        line: usize,
        print_is_last: bool,
    ) -> Result<(), CompileError> {
        self.chunk.emit(Op::LoadInt(a), line);
        let sp = self
            .chunk
            .add_constant(PerlValue::string(space_word.to_string()));
        self.chunk.emit(Op::LoadConst(sp), line);
        self.chunk.emit(Op::LoadInt(b), line);
        let nl = self.chunk.add_constant(PerlValue::string("\n".to_string()));
        self.chunk.emit(Op::LoadConst(nl), line);
        self.chunk.emit(Op::Print(4), line);
        if !print_is_last {
            self.chunk.emit(Op::Pop, line);
        }
        Ok(())
    }

    fn emit_string_repeat_length_fusion(
        &mut self,
        spec: &StringRepeatLengthFusionSpec,
        line: usize,
        print_is_last: bool,
    ) -> Result<(), CompileError> {
        self.emit_fused_print_int_newline(spec.total_len, line, print_is_last)
    }

    fn emit_hash_sum_fusion(
        &mut self,
        spec: &HashSumFusionSpec,
        line: usize,
        print_is_last: bool,
    ) -> Result<(), CompileError> {
        self.emit_fused_print_int_newline(spec.sum, line, print_is_last)
    }

    fn emit_array_push_sort_fusion(
        &mut self,
        spec: &ArrayPushSortFusionSpec,
        line: usize,
        print_is_last: bool,
    ) -> Result<(), CompileError> {
        self.emit_fused_print_four_words(spec.first, " ", spec.last, line, print_is_last)
    }

    fn emit_map_grep_scalar_fusion(
        &mut self,
        spec: &MapGrepScalarFusionSpec,
        line: usize,
        print_is_last: bool,
    ) -> Result<(), CompileError> {
        self.emit_fused_print_int_newline(spec.scalar, line, print_is_last)
    }

    fn emit_regex_count_fusion(
        &mut self,
        spec: &RegexCountFusionSpec,
        line: usize,
        print_is_last: bool,
    ) -> Result<(), CompileError> {
        self.emit_fused_print_int_newline(spec.count, line, print_is_last)
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

        // First pass: register sub names for forward calls.
        for stmt in &program.statements {
            if let StmtKind::SubDecl { name, .. } = &stmt.kind {
                let name_idx = self.chunk.intern_name(name);
                // Will be patched later
                self.chunk.sub_entries.push((name_idx, 0, false));
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

        let mut i = 0;
        while i < main_stmts.len() {
            if i + 5 <= main_stmts.len() {
                if let Some(spec) = try_match_hash_sum_fusion(
                    main_stmts[i],
                    main_stmts[i + 1],
                    main_stmts[i + 2],
                    main_stmts[i + 3],
                    main_stmts[i + 4],
                ) {
                    self.emit_hash_sum_fusion(&spec, main_stmts[i + 4].line, i + 4 == last_idx)?;
                    i += 5;
                    continue;
                }
            }
            if i + 4 <= main_stmts.len() {
                if let Some(spec) = try_match_regex_count_fusion(
                    main_stmts[i],
                    main_stmts[i + 1],
                    main_stmts[i + 2],
                    main_stmts[i + 3],
                ) {
                    self.emit_regex_count_fusion(&spec, main_stmts[i + 3].line, i + 3 == last_idx)?;
                    i += 4;
                    continue;
                }
                if let Some(spec) = try_match_array_push_sort_fusion(
                    main_stmts[i],
                    main_stmts[i + 1],
                    main_stmts[i + 2],
                    main_stmts[i + 3],
                ) {
                    self.emit_array_push_sort_fusion(
                        &spec,
                        main_stmts[i + 3].line,
                        i + 3 == last_idx,
                    )?;
                    i += 4;
                    continue;
                }
                if let Some(spec) = try_match_map_grep_scalar_fusion(
                    main_stmts[i],
                    main_stmts[i + 1],
                    main_stmts[i + 2],
                    main_stmts[i + 3],
                ) {
                    self.emit_map_grep_scalar_fusion(
                        &spec,
                        main_stmts[i + 3].line,
                        i + 3 == last_idx,
                    )?;
                    i += 4;
                    continue;
                }
            }
            if i + 3 <= main_stmts.len() {
                if let Some(spec) = try_match_string_repeat_length_fusion(
                    main_stmts[i],
                    main_stmts[i + 1],
                    main_stmts[i + 2],
                ) {
                    self.emit_string_repeat_length_fusion(
                        &spec,
                        main_stmts[i + 2].line,
                        i + 2 == last_idx,
                    )?;
                    i += 3;
                    continue;
                }
                if let Some(spec) = try_match_triangular_for_fusion(
                    main_stmts[i],
                    main_stmts[i + 1],
                    main_stmts[i + 2],
                ) {
                    self.emit_triangular_for_fusion(
                        &spec,
                        main_stmts[i],
                        main_stmts[i + 1],
                        main_stmts[i + 2],
                        i + 2 == last_idx,
                    )?;
                    i += 3;
                    continue;
                }
            }

            let stmt = main_stmts[i];
            if i == last_idx {
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
            let name_idx = self.chunk.intern_name(name);
            // Patch the entry point
            for e in &mut self.chunk.sub_entries {
                if e.0 == name_idx {
                    e.1 = entry_ip;
                }
            }
            // Compile sub body (VM `Call` pushes a scope frame; mirror for frozen tracking).
            self.emit_subroutine_body_return(body)?;
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
                            "local *FH / typeglob (use tree interpreter)".into(),
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
                        return Err(CompileError::Unsupported(
                            "local *FH / typeglob (use tree interpreter)".into(),
                        ));
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

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), CompileError> {
        let line = stmt.line;
        match &stmt.kind {
            StmtKind::FormatDecl { .. } => {
                return Err(CompileError::Unsupported("format".into()));
            }
            StmtKind::Expression(expr) => {
                self.compile_expr_ctx(expr, WantarrayCtx::Void)?;
                self.chunk.emit(Op::Pop, line);
            }
            StmtKind::Local(decls) => self.compile_local_declarations(decls, line)?,
            StmtKind::LocalExpr { .. } => {
                return Err(CompileError::Unsupported(
                    r"local on computed lvalue (e.g. $SIG{__WARN__}) (use tree interpreter)".into(),
                ));
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
                continue_block: _,
            } => {
                let loop_start = self.chunk.len();
                self.compile_boolean_rvalue_condition(condition)?;
                let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);

                let mut ctx = LoopCtx {
                    label: label.clone(),
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    break_jumps: vec![],
                    continue_jumps: vec![],
                };
                self.compile_block_no_frame(body, &mut ctx)?;
                for j in ctx.continue_jumps {
                    self.chunk.patch_jump_to(j, loop_start);
                }
                self.chunk.emit(Op::Jump(loop_start), line);
                self.chunk.patch_jump_here(exit_jump);
                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
            }
            StmtKind::Until {
                condition,
                body,
                label,
                continue_block: _,
            } => {
                let loop_start = self.chunk.len();
                self.compile_boolean_rvalue_condition(condition)?;
                let exit_jump = self.chunk.emit(Op::JumpIfTrue(0), line);

                let mut ctx = LoopCtx {
                    label: label.clone(),
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    break_jumps: vec![],
                    continue_jumps: vec![],
                };
                self.compile_block_no_frame(body, &mut ctx)?;
                for j in ctx.continue_jumps {
                    self.chunk.patch_jump_to(j, loop_start);
                }
                self.chunk.emit(Op::Jump(loop_start), line);
                self.chunk.patch_jump_here(exit_jump);
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
                continue_block: _,
            } => {
                self.emit_push_frame(line);
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

                let mut ctx = LoopCtx {
                    label: label.clone(),
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    break_jumps: cond_exit.into_iter().collect(),
                    continue_jumps: vec![],
                };
                self.compile_block_no_frame(body, &mut ctx)?;

                let step_ip = self.chunk.len();
                for j in ctx.continue_jumps {
                    self.chunk.patch_jump_to(j, step_ip);
                }
                if let Some(step) = step {
                    self.compile_expr(step)?;
                    self.chunk.emit(Op::Pop, line);
                }
                self.chunk.emit(Op::Jump(loop_start), line);

                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
                self.emit_pop_frame(line);
            }
            StmtKind::Foreach {
                var,
                list,
                body,
                label,
                continue_block: _,
            } => {
                // Compile list, then use GetArray + loop counter
                self.compile_expr(list)?;
                let list_name = self.chunk.intern_name("__foreach_list__");
                self.chunk.emit(Op::DeclareArray(list_name), line);

                let counter_name = self.chunk.intern_name("__foreach_i__");
                self.chunk.emit(Op::LoadInt(0), line);
                self.chunk.emit(Op::DeclareScalar(counter_name), line);

                let var_name = self.chunk.intern_name(var);
                self.chunk.emit(Op::LoadUndef, line);
                self.chunk.emit(Op::DeclareScalar(var_name), line);

                let loop_start = self.chunk.len();
                // Check: $i < scalar @list
                self.emit_get_scalar(counter_name, line, None);
                self.chunk.emit(Op::ArrayLen(list_name), line);
                self.chunk.emit(Op::NumLt, line);
                let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);

                // $var = $list[$i]
                self.emit_get_scalar(counter_name, line, None);
                self.chunk.emit(Op::GetArrayElem(list_name), line);
                self.emit_set_scalar(var_name, line, None);

                let mut ctx = LoopCtx {
                    label: label.clone(),
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    break_jumps: vec![],
                    continue_jumps: vec![],
                };
                self.compile_block_no_frame(body, &mut ctx)?;
                let step_ip = self.chunk.len();
                for j in ctx.continue_jumps {
                    self.chunk.patch_jump_to(j, step_ip);
                }

                // $i++
                self.emit_pre_inc(counter_name, line, None);
                self.chunk.emit(Op::Pop, line);
                self.chunk.emit(Op::Jump(loop_start), line);

                self.chunk.patch_jump_here(exit_jump);
                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
            }
            StmtKind::DoWhile { body, condition } => {
                let loop_start = self.chunk.len();
                let mut ctx = LoopCtx {
                    label: None,
                    entry_frame_depth: self.frame_depth,
                    entry_try_depth: self.try_depth,
                    break_jumps: vec![],
                    continue_jumps: vec![],
                };
                self.compile_block_with_loop(body, &mut ctx)?;
                for j in ctx.continue_jumps {
                    self.chunk.patch_jump_to(j, loop_start);
                }
                self.compile_boolean_rvalue_condition(condition)?;
                let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);
                self.chunk.emit(Op::Jump(loop_start), line);
                self.chunk.patch_jump_here(exit_jump);
                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
            }
            StmtKind::Goto { .. } => {
                return Err(CompileError::Unsupported("goto".into()));
            }
            StmtKind::Continue(_) => {
                return Err(CompileError::Unsupported("continue block".into()));
            }
            StmtKind::Return(val) => {
                if let Some(expr) = val {
                    self.compile_expr(expr)?;
                    self.chunk.emit(Op::ReturnValue, line);
                } else {
                    self.chunk.emit(Op::Return, line);
                }
            }
            StmtKind::Last(_) | StmtKind::Next(_) => {
                // last/next are only safe when handled by compile_block_with_loop
                // or compile_block_no_frame. If we reach here, it means they're
                // nested inside an if/unless/other block and can't be patched.
                // Fall back to tree-walker.
                return Err(CompileError::Unsupported(
                    "last/next inside nested block".into(),
                ));
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
            StmtKind::UseOverload { .. } => {
                return Err(CompileError::Unsupported(
                    "use overload (use tree interpreter)".into(),
                ));
            }
            StmtKind::UsePerlVersion { .. }
            | StmtKind::Use { .. }
            | StmtKind::No { .. }
            | StmtKind::Begin(_)
            | StmtKind::UnitCheck(_)
            | StmtKind::Check(_)
            | StmtKind::Init(_)
            | StmtKind::End(_)
            | StmtKind::Empty
            | StmtKind::Redo(_) => {
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
        // If the block contains return statements, skip PushFrame/PopFrame
        // to avoid scope frame mismatch on ReturnValue (VM only pops the
        // call-stack frame, not intermediate scope frames).
        if Self::block_has_return(block) {
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

    fn compile_block_with_loop(
        &mut self,
        block: &Block,
        ctx: &mut LoopCtx,
    ) -> Result<(), CompileError> {
        for stmt in block {
            if matches!(stmt.kind, StmtKind::Last(_)) {
                let j = self.chunk.emit(Op::Jump(0), stmt.line);
                ctx.break_jumps.push(j);
            } else if matches!(stmt.kind, StmtKind::Next(_)) {
                let j = self.chunk.emit(Op::Jump(0), stmt.line);
                ctx.continue_jumps.push(j);
            } else {
                self.compile_statement(stmt)?;
            }
        }
        Ok(())
    }

    fn compile_block_no_frame(
        &mut self,
        block: &Block,
        ctx: &mut LoopCtx,
    ) -> Result<(), CompileError> {
        for stmt in block {
            if matches!(stmt.kind, StmtKind::Last(_)) {
                let j = self.chunk.emit(Op::Jump(0), stmt.line);
                ctx.break_jumps.push(j);
            } else if matches!(stmt.kind, StmtKind::Next(_)) {
                let j = self.chunk.emit(Op::Jump(0), stmt.line);
                ctx.continue_jumps.push(j);
            } else {
                self.compile_statement(stmt)?;
            }
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
            ExprKind::Bareword(_) => {
                return Err(CompileError::Unsupported(
                    "bareword rvalue (resolve subroutine vs string) — use tree interpreter".into(),
                ));
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
                let idx = self.chunk.intern_name(name);
                self.emit_get_scalar(idx, line, Some(root));
            }
            ExprKind::ArrayVar(name) => {
                let idx = self.chunk.intern_name(&self.qualify_stash_array_name(name));
                self.emit_op(Op::GetArray(idx), line, Some(root));
            }
            ExprKind::HashVar(name) => {
                let idx = self.chunk.intern_name(name);
                self.emit_op(Op::GetHash(idx), line, Some(root));
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
                let idx = self
                    .chunk
                    .intern_name(&self.qualify_stash_array_name(array));
                self.compile_expr(index)?;
                self.emit_op(Op::GetArrayElem(idx), line, Some(root));
            }
            ExprKind::HashElement { hash, key } => {
                let idx = self.chunk.intern_name(hash);
                self.compile_expr(key)?;
                self.emit_op(Op::GetHashElem(idx), line, Some(root));
            }
            ExprKind::ArraySlice { array, indices } => {
                let arr_idx = self
                    .chunk
                    .intern_name(&self.qualify_stash_array_name(array));
                for index_expr in indices {
                    self.compile_expr(index_expr)?;
                    self.emit_op(Op::GetArrayElem(arr_idx), line, Some(root));
                }
                self.emit_op(Op::MakeArray(indices.len() as u16), line, Some(root));
            }
            ExprKind::HashSlice { hash, keys } => {
                let hash_idx = self.chunk.intern_name(hash);
                for key_expr in keys {
                    self.compile_expr(key_expr)?;
                    self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                }
                self.emit_op(Op::MakeArray(keys.len() as u16), line, Some(root));
            }
            ExprKind::HashSliceDeref { .. } => {
                return Err(CompileError::Unsupported(
                    "hash slice through scalar ref (@$h{...})".into(),
                ));
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
                } else {
                    return Err(CompileError::Unsupported("PostfixOp on non-scalar".into()));
                }
            }

            ExprKind::Assign { target, value } => {
                self.compile_expr(value)?;
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
                    if let Some(op_b) = scalar_compound_op_to_byte(*op) {
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
                    let vm_op = binop_to_vm_op(*op).ok_or_else(|| {
                        CompileError::Unsupported("CompoundAssign op".into())
                    })?;
                    let q = self.qualify_stash_array_name(array);
                    self.check_array_mutable(&q, line)?;
                    let arr_idx = self.chunk.intern_name(&q);
                    self.compile_expr(index)?;
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::GetArrayElem(arr_idx), line, Some(root));
                    self.compile_expr(value)?;
                    self.emit_op(vm_op, line, Some(root));
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::Rot, line, Some(root));
                    self.emit_op(Op::SetArrayElem(arr_idx), line, Some(root));
                } else if let ExprKind::HashElement { hash, key } = &target.kind {
                    if self.is_mysync_hash(hash) {
                        return Err(CompileError::Unsupported(
                            "mysync hash element update (tree interpreter)".into(),
                        ));
                    }
                    let vm_op = binop_to_vm_op(*op).ok_or_else(|| {
                        CompileError::Unsupported("CompoundAssign op".into())
                    })?;
                    self.check_hash_mutable(hash, line)?;
                    let hash_idx = self.chunk.intern_name(hash);
                    self.compile_expr(key)?;
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::GetHashElem(hash_idx), line, Some(root));
                    self.compile_expr(value)?;
                    self.emit_op(vm_op, line, Some(root));
                    self.emit_op(Op::Dup, line, Some(root));
                    self.emit_op(Op::Rot, line, Some(root));
                    self.emit_op(Op::SetHashElem(hash_idx), line, Some(root));
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

            ExprKind::Range { from, to } => {
                self.compile_expr(from)?;
                self.compile_expr(to)?;
                self.emit_op(Op::Range, line, Some(root));
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
                        self.compile_expr(arg)?;
                    }
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Pipeline as u16, args.len() as u8),
                        line,
                        Some(root),
                    );
                }
                "par_pipeline" => {
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::ParPipeline as u16, args.len() as u8),
                        line,
                        Some(root),
                    );
                }
                "par_pipeline_stream" => {
                    for arg in args {
                        self.compile_expr(arg)?;
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
                    let name_idx = self.chunk.intern_name(name);
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
            ExprKind::Print { args, .. } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit_op(Op::Print(args.len() as u8), line, Some(root));
            }
            ExprKind::Say { args, .. } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit_op(Op::Say(args.len() as u8), line, Some(root));
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
                        self.compile_expr(v)?;
                        self.emit_op(Op::PushArray(idx), line, Some(root));
                    }
                    self.emit_op(Op::ArrayLen(idx), line, Some(root));
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
                } else {
                    let pool = self.chunk.add_pop_expr_entry(array.as_ref().clone());
                    self.emit_op(Op::PopExpr(pool), line, Some(root));
                }
            }
            ExprKind::Shift(array) => {
                if let ExprKind::ArrayVar(name) = &array.kind {
                    let idx = self.chunk.intern_name(&self.qualify_stash_array_name(name));
                    self.emit_op(Op::ShiftArray(idx), line, Some(root));
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
                        self.compile_expr(v)?;
                    }
                    let nargs = (1 + values.len()) as u8;
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::Unshift as u16, nargs),
                        line,
                        Some(root),
                    );
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
                } else {
                    let pool = self.chunk.add_splice_expr_entry(
                        array.as_ref().clone(),
                        offset.as_deref().cloned(),
                        length.as_deref().cloned(),
                        replacement.clone(),
                    );
                    self.emit_op(Op::SpliceExpr(pool), line, Some(root));
                }
            }
            ExprKind::ScalarContext(inner) => {
                if let ExprKind::ArrayVar(name) = &inner.kind {
                    let idx = self.chunk.intern_name(&self.qualify_stash_array_name(name));
                    self.emit_op(Op::ArrayLen(idx), line, Some(root));
                } else {
                    self.compile_expr(inner)?;
                }
            }

            // ── Hash ops ──
            ExprKind::Delete(inner) => {
                if let ExprKind::HashElement { hash, key } = &inner.kind {
                    self.check_hash_mutable(hash, line)?;
                    let idx = self.chunk.intern_name(hash);
                    self.compile_expr(key)?;
                    self.emit_op(Op::DeleteHashElem(idx), line, Some(root));
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
                } else {
                    let pool = self.chunk.add_exists_expr_entry(inner.as_ref().clone());
                    self.emit_op(Op::ExistsExpr(pool), line, Some(root));
                }
            }
            ExprKind::Keys(inner) => {
                if let ExprKind::HashVar(name) = &inner.kind {
                    let idx = self.chunk.intern_name(name);
                    self.emit_op(Op::HashKeys(idx), line, Some(root));
                } else {
                    let pool = self.chunk.add_keys_expr_entry(inner.as_ref().clone());
                    self.emit_op(Op::KeysExpr(pool), line, Some(root));
                }
            }
            ExprKind::Values(inner) => {
                if let ExprKind::HashVar(name) = &inner.kind {
                    let idx = self.chunk.intern_name(name);
                    self.emit_op(Op::HashValues(idx), line, Some(root));
                } else {
                    let pool = self.chunk.add_values_expr_entry(inner.as_ref().clone());
                    self.emit_op(Op::ValuesExpr(pool), line, Some(root));
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
                self.compile_expr(e)?;
                self.emit_op(Op::ReverseOp, line, Some(root));
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
                if matches!(handle.kind, ExprKind::OpenMyHandle { .. }) {
                    return Err(CompileError::Unsupported(
                        "open my $fh (use interpreter, not JIT)".into(),
                    ));
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
                if let Some(h) = handle {
                    let idx = self.chunk.add_constant(PerlValue::string(h.clone()));
                    self.emit_op(Op::LoadConst(idx), line, Some(root));
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::ReadLine as u16, 1),
                        line,
                        Some(root),
                    );
                } else {
                    self.emit_op(
                        Op::CallBuiltin(BuiltinId::ReadLine as u16, 0),
                        line,
                        Some(root),
                    );
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
                self.compile_expr(e)?;
                self.emit_op(
                    Op::CallBuiltin(BuiltinId::Readdir as u16, 1),
                    line,
                    Some(root),
                );
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
                    self.emit_op(Op::EvalBlock(block_idx), line, Some(root));
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
            ExprKind::ScalarRef(e) => {
                self.compile_expr(e)?;
                self.emit_op(Op::MakeScalarRef, line, Some(root));
            }
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
                let name_idx = self.chunk.intern_name(name);
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
            ExprKind::ArrowDeref { expr, index, kind } => {
                self.compile_expr(expr)?;
                self.compile_expr(index)?;
                match kind {
                    DerefKind::Array => {
                        self.emit_op(Op::ArrowArray, line, Some(root));
                    }
                    DerefKind::Hash => {
                        self.emit_op(Op::ArrowHash, line, Some(root));
                    }
                    DerefKind::Call => {
                        self.emit_op(Op::ArrowCall(ctx.as_byte()), line, Some(root));
                    }
                }
            }
            ExprKind::Deref { .. } => {
                return Err(CompileError::Unsupported(
                    "symbolic ref deref ($$name, @{...}) — use tree interpreter".into(),
                ));
            }

            // ── Interpolated strings ──
            ExprKind::InterpolatedString(parts) => {
                if parts.is_empty() {
                    let idx = self.chunk.add_constant(PerlValue::string(String::new()));
                    self.emit_op(Op::LoadConst(idx), line, Some(root));
                } else {
                    self.compile_string_part(&parts[0], line, Some(root))?;
                    for part in &parts[1..] {
                        self.compile_string_part(part, line, Some(root))?;
                        self.emit_op(Op::Concat, line, Some(root));
                    }
                }
            }

            // ── List ──
            ExprKind::List(exprs) => {
                for e in exprs {
                    self.compile_expr_ctx(e, ctx)?;
                }
                if exprs.len() != 1 {
                    self.emit_op(Op::MakeArray(exprs.len() as u16), line, Some(root));
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
                if let Some(k) = crate::map_grep_fast::detect_map_int_mul(block) {
                    self.emit_op(Op::MapIntMul(k), line, Some(root));
                } else {
                    let block_idx = self.chunk.add_block(block.clone());
                    self.emit_op(Op::MapWithBlock(block_idx), line, Some(root));
                }
            }
            ExprKind::GrepExpr { block, list } => {
                self.compile_expr(list)?;
                if let Some((m, r)) = crate::map_grep_fast::detect_grep_int_mod_eq(block) {
                    self.emit_op(Op::GrepIntModEq(m, r), line, Some(root));
                } else {
                    let block_idx = self.chunk.add_block(block.clone());
                    self.emit_op(Op::GrepWithBlock(block_idx), line, Some(root));
                }
            }
            ExprKind::GrepExprComma { expr, list } => {
                self.compile_expr(list)?;
                let idx = self.chunk.add_grep_expr_entry(*expr.clone());
                self.emit_op(Op::GrepWithExpr(idx), line, Some(root));
            }
            ExprKind::SortExpr { cmp, list } => {
                self.compile_expr(list)?;
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
                        self.emit_op(
                            Op::SortWithCodeComparator(ctx.as_byte()),
                            line,
                            Some(root),
                        );
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
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
                self.compile_expr(list)?;
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
            }
            StringPart::Expr(e) => {
                self.compile_expr(e)?;
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
                self.check_scalar_mutable(name, line)?;
                let idx = self.chunk.intern_name(name);
                if keep {
                    self.emit_set_scalar_keep(idx, line, ast);
                } else {
                    self.emit_set_scalar(idx, line, ast);
                }
            }
            ExprKind::ArrayVar(name) => {
                let q = self.qualify_stash_array_name(name);
                self.check_array_mutable(&q, line)?;
                let idx = self.chunk.intern_name(&q);
                self.emit_op(Op::SetArray(idx), line, ast);
                if keep {
                    self.emit_op(Op::GetArray(idx), line, ast);
                }
            }
            ExprKind::HashVar(name) => {
                self.check_hash_mutable(name, line)?;
                let idx = self.chunk.intern_name(name);
                self.emit_op(Op::SetHash(idx), line, ast);
                if keep {
                    self.emit_op(Op::GetHash(idx), line, ast);
                }
            }
            ExprKind::ArrayElement { array, index } => {
                let q = self.qualify_stash_array_name(array);
                self.check_array_mutable(&q, line)?;
                let idx = self.chunk.intern_name(&q);
                self.compile_expr(index)?;
                self.emit_op(Op::SetArrayElem(idx), line, ast);
            }
            ExprKind::HashElement { hash, key } => {
                self.check_hash_mutable(hash, line)?;
                let idx = self.chunk.intern_name(hash);
                self.compile_expr(key)?;
                self.emit_op(Op::SetHashElem(idx), line, ast);
            }
            ExprKind::ArrowDeref {
                expr,
                index,
                kind: DerefKind::Hash,
            } => {
                self.compile_expr(expr)?;
                self.compile_expr(index)?;
                self.emit_op(Op::SetArrowHash, line, ast);
            }
            ExprKind::ArrowDeref { .. } => {
                return Err(CompileError::Unsupported(
                    "Assign to arrow array/call deref (tree interpreter)".into(),
                ));
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
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::DeclareScalar(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_scalar_fetch_and_assign() {
        let chunk = compile_snippet("my $a = 1; $a + 0;").expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .filter(|o| matches!(o, Op::GetScalar(_) | Op::GetScalarPlain(_)))
                .count()
                >= 1
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_plain_scalar_read_emits_get_scalar_plain() {
        let chunk = compile_snippet("my $a = 1; $a + 0;").expect("compile");
        assert!(
            chunk.ops.iter().any(|o| matches!(o, Op::GetScalarPlain(_))),
            "expected GetScalarPlain for non-special $a, ops={:?}",
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
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::PostInc(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_preinc_scalar() {
        let chunk = compile_snippet("my $n = 1; ++$n;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::PreInc(_))));
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
        let chunk = compile_snippet("(1..3);").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Range)));
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_print_statement() {
        let chunk = compile_snippet("print 1;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Print(_))));
        assert_last_halt(&chunk);
    }

    #[test]
    fn bench_loop_shape_emits_triangular_for_accum() {
        let code = "my $sum = 0;\n\
            for (my $i = 0; $i < 10000; $i = $i + 1) {\n\
                $sum = $sum + $i;\n\
            }\n\
            print $sum, \"\\n\";";
        let chunk = compile_snippet(code).expect("compile");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::TriangularForAccum { .. })),
            "expected TriangularForAccum, ops={:?}",
            chunk.ops
        );
        assert_last_halt(&chunk);
    }

    #[test]
    fn compile_say_statement() {
        let chunk = compile_snippet("say 1;").expect("compile");
        assert!(chunk.ops.iter().any(|o| matches!(o, Op::Say(_))));
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
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::SetHashElem(_))),
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
}
