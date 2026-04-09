use std::collections::HashSet;

use crate::ast::*;
use crate::bytecode::{BuiltinId, Chunk, Op};
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

#[derive(Default)]
struct ScopeLayer {
    declared_scalars: HashSet<String>,
    declared_arrays: HashSet<String>,
    declared_hashes: HashSet<String>,
    frozen_scalars: HashSet<String>,
    frozen_arrays: HashSet<String>,
    frozen_hashes: HashSet<String>,
}

/// Loop context for resolving `last`/`next` jumps.
struct LoopCtx {
    #[allow(dead_code)]
    label: Option<String>,
    /// Positions of `last` jumps to patch (jump to after loop).
    break_jumps: Vec<usize>,
    /// Target address for `next` (jump to loop step/condition).
    continue_target: usize,
}

pub struct Compiler {
    pub chunk: Chunk,
    pub begin_blocks: Vec<Block>,
    pub end_blocks: Vec<Block>,
    /// Lexical `my` declarations per scope frame (mirrors `PushFrame` / sub bodies).
    scope_stack: Vec<ScopeLayer>,
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
            begin_blocks: Vec::new(),
            end_blocks: Vec::new(),
            scope_stack: vec![ScopeLayer::default()],
        }
    }

    fn push_scope_layer(&mut self) {
        self.scope_stack.push(ScopeLayer::default());
    }

    fn pop_scope_layer(&mut self) {
        if self.scope_stack.len() > 1 {
            self.scope_stack.pop();
        }
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

    pub fn compile_program(mut self, program: &Program) -> Result<Chunk, CompileError> {
        // Extract BEGIN/END blocks before compiling.
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::Begin(block) => self.begin_blocks.push(block.clone()),
                StmtKind::End(block) => self.end_blocks.push(block.clone()),
                _ => {}
            }
        }

        // First pass: register sub names for forward calls.
        for stmt in &program.statements {
            if let StmtKind::SubDecl { name, .. } = &stmt.kind {
                let name_idx = self.chunk.intern_name(name);
                // Will be patched later
                self.chunk.sub_entries.push((name_idx, 0));
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
                    StmtKind::SubDecl { .. } | StmtKind::Begin(_) | StmtKind::End(_)
                )
            })
            .collect();
        let last_idx = main_stmts.len().saturating_sub(1);
        for (i, stmt) in main_stmts.iter().enumerate() {
            if i == last_idx {
                match &stmt.kind {
                    StmtKind::Expression(expr) => self.compile_expr(expr)?,
                    StmtKind::If {
                        condition,
                        body,
                        elsifs,
                        else_block,
                    } => {
                        self.compile_expr(condition)?;
                        let j0 = self.chunk.emit(Op::JumpIfFalse(0), stmt.line);
                        self.emit_block_value(body, stmt.line)?;
                        let mut ends = vec![self.chunk.emit(Op::Jump(0), stmt.line)];
                        self.chunk.patch_jump_here(j0);
                        for (c, blk) in elsifs {
                            self.compile_expr(c)?;
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
                        self.compile_expr(condition)?;
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
        }
        self.chunk.emit(Op::Halt, 0);

        // Third pass: compile sub bodies after Halt
        let entries: Vec<(String, Vec<Statement>)> = program
            .statements
            .iter()
            .filter_map(|s| {
                if let StmtKind::SubDecl { name, body, .. } = &s.kind {
                    Some((name.clone(), body.clone()))
                } else {
                    None
                }
            })
            .collect();

        for (name, body) in &entries {
            self.push_scope_layer();
            let entry_ip = self.chunk.len();
            let name_idx = self.chunk.intern_name(name);
            // Patch the entry point
            for e in &mut self.chunk.sub_entries {
                if e.0 == name_idx {
                    e.1 = entry_ip;
                }
            }
            // Compile sub body (VM `Call` pushes a scope frame; mirror for frozen tracking).
            for stmt in body {
                self.compile_statement(stmt)?;
            }
            // Implicit return undef
            self.chunk.emit(Op::LoadUndef, 0);
            self.chunk.emit(Op::ReturnValue, 0);
            self.pop_scope_layer();
        }

        Ok(self.chunk)
    }

    fn emit_declare_scalar(&mut self, name_idx: u16, line: usize, frozen: bool) {
        let name = self.chunk.names[name_idx as usize].clone();
        self.register_declare(Sigil::Scalar, &name, frozen);
        if frozen {
            self.chunk.emit(Op::DeclareScalarFrozen(name_idx), line);
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
        if decls.iter().any(|d| d.type_annotation.is_some()) {
            return Err(CompileError::Unsupported("typed my".into()));
        }
        let allow_frozen = is_my;
        // List assignment: my ($a, $b) = (10, 20) — distribute elements
        if decls.len() > 1 && decls[0].initializer.is_some() {
            self.compile_expr(decls[0].initializer.as_ref().unwrap())?;
            let tmp_name = self.chunk.intern_name("__list_assign_tmp__");
            self.emit_declare_array(tmp_name, line, false);
            for (i, decl) in decls.iter().enumerate() {
                let frozen = allow_frozen && decl.frozen;
                let name_idx = self.chunk.intern_name(&decl.name);
                match decl.sigil {
                    Sigil::Scalar => {
                        self.chunk.emit(Op::LoadInt(i as i64), line);
                        self.chunk.emit(Op::GetArrayElem(tmp_name), line);
                        self.emit_declare_scalar(name_idx, line, frozen);
                    }
                    Sigil::Array => {
                        self.chunk.emit(Op::GetArray(tmp_name), line);
                        self.emit_declare_array(name_idx, line, frozen);
                    }
                    Sigil::Hash => {
                        self.chunk.emit(Op::GetArray(tmp_name), line);
                        self.emit_declare_hash(name_idx, line, frozen);
                    }
                }
            }
        } else {
            for decl in decls {
                let frozen = allow_frozen && decl.frozen;
                let name_idx = self.chunk.intern_name(&decl.name);
                match decl.sigil {
                    Sigil::Scalar => {
                        if let Some(init) = &decl.initializer {
                            self.compile_expr(init)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, line);
                        }
                        self.emit_declare_scalar(name_idx, line, frozen);
                    }
                    Sigil::Array => {
                        if let Some(init) = &decl.initializer {
                            self.compile_expr(init)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, line);
                        }
                        self.emit_declare_array(name_idx, line, frozen);
                    }
                    Sigil::Hash => {
                        if let Some(init) = &decl.initializer {
                            self.compile_expr(init)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, line);
                        }
                        self.emit_declare_hash(name_idx, line, frozen);
                    }
                }
            }
        }
        Ok(())
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), CompileError> {
        let line = stmt.line;
        match &stmt.kind {
            StmtKind::Expression(expr) => {
                self.compile_expr(expr)?;
                self.chunk.emit(Op::Pop, line);
            }
            StmtKind::Local(_) | StmtKind::MySync(_) => {
                // local and mysync need special runtime semantics; fall back to tree-walker
                return Err(CompileError::Unsupported("local/mysync".into()));
            }
            StmtKind::My(decls) => self.compile_var_declarations(decls, line, true)?,
            StmtKind::Our(decls) => self.compile_var_declarations(decls, line, false)?,
            StmtKind::If {
                condition,
                body,
                elsifs,
                else_block,
            } => {
                self.compile_expr(condition)?;
                let jump_else = self.chunk.emit(Op::JumpIfFalse(0), line);
                self.compile_block(body)?;
                let mut end_jumps = vec![self.chunk.emit(Op::Jump(0), line)];
                self.chunk.patch_jump_here(jump_else);

                for (cond, blk) in elsifs {
                    self.compile_expr(cond)?;
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
                self.compile_expr(condition)?;
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
                self.compile_expr(condition)?;
                let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);

                let mut ctx = LoopCtx {
                    label: label.clone(),
                    break_jumps: vec![],
                    continue_target: loop_start,
                };
                self.compile_block_with_loop(body, &mut ctx)?;

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
                self.compile_expr(condition)?;
                let exit_jump = self.chunk.emit(Op::JumpIfTrue(0), line);

                let mut ctx = LoopCtx {
                    label: label.clone(),
                    break_jumps: vec![],
                    continue_target: loop_start,
                };
                self.compile_block_with_loop(body, &mut ctx)?;

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
                self.chunk.emit(Op::PushFrame, line);
                if let Some(init) = init {
                    self.compile_statement(init)?;
                }
                let loop_start = self.chunk.len();
                if let Some(cond) = condition {
                    self.compile_expr(cond)?;
                    let exit = self.chunk.emit(Op::JumpIfFalse(0), line);

                    let mut ctx = LoopCtx {
                        label: label.clone(),
                        break_jumps: vec![exit],
                        continue_target: 0, // patched below
                    };
                    self.compile_block_no_frame(body, &mut ctx)?;
                    ctx.continue_target = self.chunk.len();

                    if let Some(step) = step {
                        self.compile_expr(step)?;
                        self.chunk.emit(Op::Pop, line);
                    }
                    self.chunk.emit(Op::Jump(loop_start), line);

                    // Patch exit jump and break jumps
                    for j in ctx.break_jumps {
                        self.chunk.patch_jump_here(j);
                    }
                } else {
                    // Infinite loop
                    let mut ctx = LoopCtx {
                        label: label.clone(),
                        break_jumps: vec![],
                        continue_target: 0,
                    };
                    self.compile_block_no_frame(body, &mut ctx)?;
                    ctx.continue_target = self.chunk.len();
                    if let Some(step) = step {
                        self.compile_expr(step)?;
                        self.chunk.emit(Op::Pop, line);
                    }
                    self.chunk.emit(Op::Jump(loop_start), line);
                    for j in ctx.break_jumps {
                        self.chunk.patch_jump_here(j);
                    }
                }
                self.chunk.emit(Op::PopFrame, line);
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
                self.chunk.emit(Op::GetScalar(counter_name), line);
                self.chunk.emit(Op::ArrayLen(list_name), line);
                self.chunk.emit(Op::NumLt, line);
                let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);

                // $var = $list[$i]
                self.chunk.emit(Op::GetScalar(counter_name), line);
                self.chunk.emit(Op::GetArrayElem(list_name), line);
                self.chunk.emit(Op::SetScalar(var_name), line);

                let mut ctx = LoopCtx {
                    label: label.clone(),
                    break_jumps: vec![],
                    continue_target: 0,
                };
                self.compile_block_no_frame(body, &mut ctx)?;
                ctx.continue_target = self.chunk.len();

                // $i++
                self.chunk.emit(Op::PreInc(counter_name), line);
                self.chunk.emit(Op::Pop, line);
                self.chunk.emit(Op::Jump(loop_start), line);

                self.chunk.patch_jump_here(exit_jump);
                for j in ctx.break_jumps {
                    self.chunk.patch_jump_here(j);
                }
            }
            StmtKind::DoWhile { .. } => {
                // do-while requires parser-level changes to distinguish from do BLOCK
                return Err(CompileError::Unsupported("do-while".into()));
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
                let val_idx = self.chunk.add_constant(PerlValue::String(name.clone()));
                let name_idx = self.chunk.intern_name("__PACKAGE__");
                self.chunk.emit(Op::LoadConst(val_idx), line);
                self.chunk.emit(Op::SetScalar(name_idx), line);
            }
            StmtKind::SubDecl { .. } => {
                // Already handled in compile_program
            }
            StmtKind::Use { .. }
            | StmtKind::No { .. }
            | StmtKind::Begin(_)
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
                self.chunk.emit(Op::Jump(ctx.continue_target), stmt.line);
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
                self.chunk.emit(Op::Jump(ctx.continue_target), stmt.line);
            } else {
                self.compile_statement(stmt)?;
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        let line = expr.line;
        match &expr.kind {
            ExprKind::Integer(n) => {
                self.chunk.emit(Op::LoadInt(*n), line);
            }
            ExprKind::Float(f) => {
                self.chunk.emit(Op::LoadFloat(*f), line);
            }
            ExprKind::String(s) => {
                let idx = self.chunk.add_constant(PerlValue::String(s.clone()));
                self.chunk.emit(Op::LoadConst(idx), line);
            }
            ExprKind::Undef => {
                self.chunk.emit(Op::LoadUndef, line);
            }
            ExprKind::ScalarVar(name) => {
                let idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::GetScalar(idx), line);
            }
            ExprKind::ArrayVar(name) => {
                let idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::GetArray(idx), line);
            }
            ExprKind::HashVar(name) => {
                let idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::GetHash(idx), line);
            }
            ExprKind::ArrayElement { array, index } => {
                let idx = self.chunk.intern_name(array);
                self.compile_expr(index)?;
                self.chunk.emit(Op::GetArrayElem(idx), line);
            }
            ExprKind::HashElement { hash, key } => {
                let idx = self.chunk.intern_name(hash);
                self.compile_expr(key)?;
                self.chunk.emit(Op::GetHashElem(idx), line);
            }
            ExprKind::ArraySlice { array, indices } => {
                let arr_idx = self.chunk.intern_name(array);
                for index_expr in indices {
                    self.compile_expr(index_expr)?;
                    self.chunk.emit(Op::GetArrayElem(arr_idx), line);
                }
                self.chunk.emit(Op::MakeArray(indices.len() as u16), line);
            }
            ExprKind::HashSlice { hash, keys } => {
                let hash_idx = self.chunk.intern_name(hash);
                for key_expr in keys {
                    self.compile_expr(key_expr)?;
                    self.chunk.emit(Op::GetHashElem(hash_idx), line);
                }
                self.chunk.emit(Op::MakeArray(keys.len() as u16), line);
            }

            // ── Operators ──
            ExprKind::BinOp { left, op, right } => {
                // Short-circuit operators
                match op {
                    BinOp::LogAnd | BinOp::LogAndWord => {
                        self.compile_expr(left)?;
                        let j = self.chunk.emit(Op::JumpIfFalseKeep(0), line);
                        self.chunk.emit(Op::Pop, line);
                        self.compile_expr(right)?;
                        self.chunk.patch_jump_here(j);
                        return Ok(());
                    }
                    BinOp::LogOr | BinOp::LogOrWord => {
                        self.compile_expr(left)?;
                        let j = self.chunk.emit(Op::JumpIfTrueKeep(0), line);
                        self.chunk.emit(Op::Pop, line);
                        self.compile_expr(right)?;
                        self.chunk.patch_jump_here(j);
                        return Ok(());
                    }
                    BinOp::DefinedOr => {
                        self.compile_expr(left)?;
                        let j = self.chunk.emit(Op::JumpIfDefinedKeep(0), line);
                        self.chunk.emit(Op::Pop, line);
                        self.compile_expr(right)?;
                        self.chunk.patch_jump_here(j);
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
                    // Short-circuit handled above
                    BinOp::LogAnd
                    | BinOp::LogOr
                    | BinOp::DefinedOr
                    | BinOp::LogAndWord
                    | BinOp::LogOrWord => unreachable!(),
                    BinOp::BindMatch | BinOp::BindNotMatch => {
                        return Err(CompileError::Unsupported("BindMatch in BinOp".into()));
                    }
                };
                self.chunk.emit(op_code, line);
            }

            ExprKind::UnaryOp { op, expr } => match op {
                UnaryOp::PreIncrement => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
                        self.check_scalar_mutable(name, line)?;
                        let idx = self.chunk.intern_name(name);
                        self.chunk.emit(Op::PreInc(idx), line);
                    } else {
                        return Err(CompileError::Unsupported("PreInc on non-scalar".into()));
                    }
                }
                UnaryOp::PreDecrement => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
                        self.check_scalar_mutable(name, line)?;
                        let idx = self.chunk.intern_name(name);
                        self.chunk.emit(Op::PreDec(idx), line);
                    } else {
                        return Err(CompileError::Unsupported("PreDec on non-scalar".into()));
                    }
                }
                UnaryOp::Ref => {
                    self.compile_expr(expr)?;
                    self.chunk.emit(Op::MakeScalarRef, line);
                }
                _ => {
                    self.compile_expr(expr)?;
                    match op {
                        UnaryOp::Negate => {
                            self.chunk.emit(Op::Negate, line);
                        }
                        UnaryOp::LogNot | UnaryOp::LogNotWord => {
                            self.chunk.emit(Op::LogNot, line);
                        }
                        UnaryOp::BitNot => {
                            self.chunk.emit(Op::BitNot, line);
                        }
                        _ => unreachable!(),
                    }
                }
            },
            ExprKind::PostfixOp { expr, op } => {
                if let ExprKind::ScalarVar(name) = &expr.kind {
                    self.check_scalar_mutable(name, line)?;
                    let idx = self.chunk.intern_name(name);
                    match op {
                        PostfixOp::Increment => {
                            self.chunk.emit(Op::PostInc(idx), line);
                        }
                        PostfixOp::Decrement => {
                            self.chunk.emit(Op::PostDec(idx), line);
                        }
                    }
                } else {
                    return Err(CompileError::Unsupported("PostfixOp on non-scalar".into()));
                }
            }

            ExprKind::Assign { target, value } => {
                self.compile_expr(value)?;
                self.compile_assign(target, line, true)?;
            }
            ExprKind::CompoundAssign { target, op, value } => {
                if let ExprKind::ScalarVar(name) = &target.kind {
                    self.check_scalar_mutable(name, line)?;
                    let idx = self.chunk.intern_name(name);
                    self.chunk.emit(Op::GetScalar(idx), line);
                    self.compile_expr(value)?;
                    let op_code = match op {
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
                        _ => return Err(CompileError::Unsupported("CompoundAssign op".into())),
                    };
                    self.chunk.emit(op_code, line);
                    self.chunk.emit(Op::SetScalarKeep(idx), line);
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
                self.compile_expr(condition)?;
                let jump_else = self.chunk.emit(Op::JumpIfFalse(0), line);
                self.compile_expr(then_expr)?;
                let jump_end = self.chunk.emit(Op::Jump(0), line);
                self.chunk.patch_jump_here(jump_else);
                self.compile_expr(else_expr)?;
                self.chunk.patch_jump_here(jump_end);
            }

            ExprKind::Range { from, to } => {
                self.compile_expr(from)?;
                self.compile_expr(to)?;
                self.chunk.emit(Op::Range, line);
            }

            ExprKind::Repeat { expr, count } => {
                self.compile_expr(expr)?;
                self.compile_expr(count)?;
                self.chunk.emit(Op::StringRepeat, line);
            }

            // ── Function calls ──
            ExprKind::FuncCall { name, args } => match name.as_str() {
                "deque" => {
                    if !args.is_empty() {
                        return Err(CompileError::Unsupported(
                            "deque() takes no arguments".into(),
                        ));
                    }
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::DequeNew as u16, 0), line);
                }
                "heap" => {
                    if args.len() != 1 {
                        return Err(CompileError::Unsupported(
                            "heap() expects one comparator sub".into(),
                        ));
                    }
                    self.compile_expr(&args[0])?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::HeapNew as u16, 1), line);
                }
                "pipeline" => {
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.chunk.emit(
                        Op::CallBuiltin(BuiltinId::Pipeline as u16, args.len() as u8),
                        line,
                    );
                }
                "ppool" => {
                    if args.len() != 1 {
                        return Err(CompileError::Unsupported(
                            "ppool() expects one argument (worker count)".into(),
                        ));
                    }
                    self.compile_expr(&args[0])?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Ppool as u16, 1), line);
                }
                _ => {
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    let name_idx = self.chunk.intern_name(name);
                    self.chunk.emit(Op::Call(name_idx, args.len() as u8), line);
                }
            },

            // ── Method calls ──
            ExprKind::MethodCall {
                object,
                method,
                args,
            } => {
                self.compile_expr(object)?;
                for arg in args {
                    self.compile_expr(arg)?;
                }
                let name_idx = self.chunk.intern_name(method);
                self.chunk
                    .emit(Op::MethodCall(name_idx, args.len() as u8), line);
            }

            // ── Print / Say / Printf ──
            ExprKind::Print { args, .. } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.emit(Op::Print(args.len() as u8), line);
            }
            ExprKind::Say { args, .. } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.emit(Op::Say(args.len() as u8), line);
            }
            ExprKind::Printf { args, .. } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::Printf as u16, args.len() as u8),
                    line,
                );
            }

            // ── Die / Warn ──
            ExprKind::Die(args) => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::Die as u16, args.len() as u8),
                    line,
                );
            }
            ExprKind::Warn(args) => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::Warn as u16, args.len() as u8),
                    line,
                );
            }
            ExprKind::Exit(code) => {
                if let Some(c) = code {
                    self.compile_expr(c)?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Exit as u16, 1), line);
                } else {
                    self.chunk.emit(Op::LoadInt(0), line);
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Exit as u16, 1), line);
                }
            }

            // ── Array ops ──
            ExprKind::Push { array, values } => {
                if let ExprKind::ArrayVar(name) = &array.kind {
                    let idx = self.chunk.intern_name(name);
                    for v in values {
                        self.compile_expr(v)?;
                        self.chunk.emit(Op::PushArray(idx), line);
                    }
                    self.chunk.emit(Op::ArrayLen(idx), line);
                } else {
                    return Err(CompileError::Unsupported("Push on non-array".into()));
                }
            }
            ExprKind::Pop(array) => {
                if let ExprKind::ArrayVar(name) = &array.kind {
                    let idx = self.chunk.intern_name(name);
                    self.chunk.emit(Op::PopArray(idx), line);
                } else {
                    return Err(CompileError::Unsupported("Pop on non-array".into()));
                }
            }
            ExprKind::Shift(array) => {
                if let ExprKind::ArrayVar(name) = &array.kind {
                    let idx = self.chunk.intern_name(name);
                    self.chunk.emit(Op::ShiftArray(idx), line);
                } else {
                    return Err(CompileError::Unsupported("Shift on non-array".into()));
                }
            }
            ExprKind::Unshift { .. } | ExprKind::Splice { .. } => {
                // These modify arrays in-place; needs special VM support
                return Err(CompileError::Unsupported("unshift/splice".into()));
            }
            // Splice is already handled by Unsupported above
            ExprKind::ScalarContext(inner) => {
                if let ExprKind::ArrayVar(name) = &inner.kind {
                    let idx = self.chunk.intern_name(name);
                    self.chunk.emit(Op::ArrayLen(idx), line);
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
                    self.chunk.emit(Op::DeleteHashElem(idx), line);
                } else {
                    return Err(CompileError::Unsupported("Delete on non-hash".into()));
                }
            }
            ExprKind::Exists(inner) => {
                if let ExprKind::HashElement { hash, key } = &inner.kind {
                    let idx = self.chunk.intern_name(hash);
                    self.compile_expr(key)?;
                    self.chunk.emit(Op::ExistsHashElem(idx), line);
                } else {
                    return Err(CompileError::Unsupported("Exists on non-hash".into()));
                }
            }
            ExprKind::Keys(inner) => {
                if let ExprKind::HashVar(name) = &inner.kind {
                    let idx = self.chunk.intern_name(name);
                    self.chunk.emit(Op::HashKeys(idx), line);
                } else {
                    return Err(CompileError::Unsupported("Keys on non-hash".into()));
                }
            }
            ExprKind::Values(inner) => {
                if let ExprKind::HashVar(name) = &inner.kind {
                    let idx = self.chunk.intern_name(name);
                    self.chunk.emit(Op::HashValues(idx), line);
                } else {
                    return Err(CompileError::Unsupported("Values on non-hash".into()));
                }
            }
            ExprKind::Each(_) => {
                return Err(CompileError::Unsupported("each()".into()));
            }

            // ── Builtins that map to CallBuiltin ──
            ExprKind::Length(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Length as u16, 1), line);
            }
            ExprKind::Chomp(e) => {
                self.compile_expr(e)?;
                let lv = self.chunk.add_lvalue_expr(e.as_ref().clone());
                self.chunk.emit(Op::ChompInPlace(lv), line);
            }
            ExprKind::Chop(e) => {
                self.compile_expr(e)?;
                let lv = self.chunk.add_lvalue_expr(e.as_ref().clone());
                self.chunk.emit(Op::ChopInPlace(lv), line);
            }
            ExprKind::Defined(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Defined as u16, 1), line);
            }
            ExprKind::Abs(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Abs as u16, 1), line);
            }
            ExprKind::Int(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Int as u16, 1), line);
            }
            ExprKind::Sqrt(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Sqrt as u16, 1), line);
            }
            ExprKind::Sin(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Sin as u16, 1), line);
            }
            ExprKind::Cos(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Cos as u16, 1), line);
            }
            ExprKind::Atan2 { y, x } => {
                self.compile_expr(y)?;
                self.compile_expr(x)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Atan2 as u16, 2), line);
            }
            ExprKind::Exp(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Exp as u16, 1), line);
            }
            ExprKind::Log(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Log as u16, 1), line);
            }
            ExprKind::Rand(upper) => {
                if let Some(e) = upper {
                    self.compile_expr(e)?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Rand as u16, 1), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Rand as u16, 0), line);
                }
            }
            ExprKind::Srand(seed) => {
                if let Some(e) = seed {
                    self.compile_expr(e)?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Srand as u16, 1), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Srand as u16, 0), line);
                }
            }
            ExprKind::Chr(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Chr as u16, 1), line);
            }
            ExprKind::Ord(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Ord as u16, 1), line);
            }
            ExprKind::Hex(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Hex as u16, 1), line);
            }
            ExprKind::Oct(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Oct as u16, 1), line);
            }
            ExprKind::Uc(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Uc as u16, 1), line);
            }
            ExprKind::Lc(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Lc as u16, 1), line);
            }
            ExprKind::Ucfirst(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Ucfirst as u16, 1), line);
            }
            ExprKind::Lcfirst(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Lcfirst as u16, 1), line);
            }
            ExprKind::Fc(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Fc as u16, 1), line);
            }
            ExprKind::Crypt { plaintext, salt } => {
                self.compile_expr(plaintext)?;
                self.compile_expr(salt)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Crypt as u16, 2), line);
            }
            ExprKind::Pos(e) => match e {
                None => {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Pos as u16, 0), line);
                }
                Some(expr) => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
                        let idx = self.chunk.add_constant(PerlValue::String(name.clone()));
                        self.chunk.emit(Op::LoadConst(idx), line);
                        self.chunk
                            .emit(Op::CallBuiltin(BuiltinId::Pos as u16, 1), line);
                    } else {
                        return Err(CompileError::Unsupported(
                            "pos with non-simple scalar".into(),
                        ));
                    }
                }
            },
            ExprKind::Study(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Study as u16, 1), line);
            }
            ExprKind::Ref(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Ref as u16, 1), line);
            }
            ExprKind::ReverseExpr(e) => {
                self.compile_expr(e)?;
                self.chunk.emit(Op::ReverseOp, line);
            }
            ExprKind::System(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::System as u16, args.len() as u8),
                    line,
                );
            }
            ExprKind::Exec(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::Exec as u16, args.len() as u8),
                    line,
                );
            }

            // ── String builtins ──
            ExprKind::Substr {
                string,
                offset,
                length,
                replacement,
            } => {
                if replacement.is_some() {
                    return Err(CompileError::Unsupported("4-arg substr".into()));
                }
                self.compile_expr(string)?;
                self.compile_expr(offset)?;
                let mut argc: u8 = 2;
                if let Some(len) = length {
                    self.compile_expr(len)?;
                    argc = 3;
                }
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Substr as u16, argc), line);
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
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Index as u16, 3), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Index as u16, 2), line);
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
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Rindex as u16, 3), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Rindex as u16, 2), line);
                }
            }

            ExprKind::JoinExpr { separator, list } => {
                self.compile_expr(separator)?;
                self.compile_expr(list)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Join as u16, 2), line);
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
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Split as u16, 3), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Split as u16, 2), line);
                }
            }
            ExprKind::Sprintf { format, args } => {
                self.compile_expr(format)?;
                for a in args {
                    self.compile_expr(a)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::Sprintf as u16, (1 + args.len()) as u8),
                    line,
                );
            }

            // ── I/O ──
            ExprKind::Open { handle, mode, file } => {
                self.compile_expr(handle)?;
                self.compile_expr(mode)?;
                if let Some(f) = file {
                    self.compile_expr(f)?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Open as u16, 3), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Open as u16, 2), line);
                }
            }
            ExprKind::Close(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Close as u16, 1), line);
            }
            ExprKind::ReadLine(handle) => {
                if let Some(h) = handle {
                    let idx = self.chunk.add_constant(PerlValue::String(h.clone()));
                    self.chunk.emit(Op::LoadConst(idx), line);
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::ReadLine as u16, 1), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::ReadLine as u16, 0), line);
                }
            }
            ExprKind::Eof(e) => {
                if let Some(inner) = e {
                    self.compile_expr(inner)?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Eof as u16, 1), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Eof as u16, 0), line);
                }
            }
            ExprKind::Opendir { handle, path } => {
                self.compile_expr(handle)?;
                self.compile_expr(path)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Opendir as u16, 2), line);
            }
            ExprKind::Readdir(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Readdir as u16, 1), line);
            }
            ExprKind::Closedir(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Closedir as u16, 1), line);
            }
            ExprKind::Rewinddir(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Rewinddir as u16, 1), line);
            }
            ExprKind::Telldir(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Telldir as u16, 1), line);
            }
            ExprKind::Seekdir { handle, position } => {
                self.compile_expr(handle)?;
                self.compile_expr(position)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Seekdir as u16, 2), line);
            }

            // ── File tests ──
            ExprKind::FileTest { op, expr } => {
                self.compile_expr(expr)?;
                self.chunk.emit(Op::FileTestOp(*op as u8), line);
            }

            // ── Eval / Do / Require ──
            ExprKind::Eval(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Eval as u16, 1), line);
            }
            ExprKind::Do(e) => {
                // do { BLOCK } executes the block; do "file" loads a file
                if let ExprKind::CodeRef { body, .. } = &e.kind {
                    let block_idx = self.chunk.add_block(body.clone());
                    self.chunk.emit(Op::EvalBlock(block_idx), line);
                } else {
                    self.compile_expr(e)?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Do as u16, 1), line);
                }
            }
            ExprKind::Require(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Require as u16, 1), line);
            }

            // ── Filesystem ──
            ExprKind::Chdir(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Chdir as u16, 1), line);
            }
            ExprKind::Mkdir { path, mode } => {
                self.compile_expr(path)?;
                if let Some(m) = mode {
                    self.compile_expr(m)?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Mkdir as u16, 2), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Mkdir as u16, 1), line);
                }
            }
            ExprKind::Unlink(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::Unlink as u16, args.len() as u8),
                    line,
                );
            }
            ExprKind::Rename { old, new } => {
                self.compile_expr(old)?;
                self.compile_expr(new)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Rename as u16, 2), line);
            }
            ExprKind::Chmod(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::Chmod as u16, args.len() as u8),
                    line,
                );
            }
            ExprKind::Chown(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::Chown as u16, args.len() as u8),
                    line,
                );
            }
            ExprKind::Stat(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Stat as u16, 1), line);
            }
            ExprKind::Lstat(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Lstat as u16, 1), line);
            }
            ExprKind::Link { old, new } => {
                self.compile_expr(old)?;
                self.compile_expr(new)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Link as u16, 2), line);
            }
            ExprKind::Symlink { old, new } => {
                self.compile_expr(old)?;
                self.compile_expr(new)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Symlink as u16, 2), line);
            }
            ExprKind::Readlink(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Readlink as u16, 1), line);
            }
            ExprKind::Glob(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::Glob as u16, args.len() as u8),
                    line,
                );
            }
            ExprKind::GlobPar(args) => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.chunk.emit(
                    Op::CallBuiltin(BuiltinId::GlobPar as u16, args.len() as u8),
                    line,
                );
            }

            // ── OOP ──
            ExprKind::Bless { ref_expr, class } => {
                self.compile_expr(ref_expr)?;
                if let Some(c) = class {
                    self.compile_expr(c)?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Bless as u16, 2), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Bless as u16, 1), line);
                }
            }
            ExprKind::Caller(e) => {
                if let Some(inner) = e {
                    self.compile_expr(inner)?;
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Caller as u16, 1), line);
                } else {
                    self.chunk
                        .emit(Op::CallBuiltin(BuiltinId::Caller as u16, 0), line);
                }
            }
            ExprKind::Wantarray => {
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Wantarray as u16, 0), line);
            }

            // ── References ──
            ExprKind::ScalarRef(e) => {
                self.compile_expr(e)?;
                self.chunk.emit(Op::MakeScalarRef, line);
            }
            ExprKind::ArrayRef(elems) => {
                for e in elems {
                    self.compile_expr(e)?;
                }
                self.chunk.emit(Op::MakeArray(elems.len() as u16), line);
                self.chunk.emit(Op::MakeArrayRef, line);
            }
            ExprKind::HashRef(pairs) => {
                for (k, v) in pairs {
                    self.compile_expr(k)?;
                    self.compile_expr(v)?;
                }
                self.chunk
                    .emit(Op::MakeHash((pairs.len() * 2) as u16), line);
                self.chunk.emit(Op::MakeHashRef, line);
            }
            ExprKind::CodeRef { body, .. } => {
                let block_idx = self.chunk.add_block(body.clone());
                self.chunk.emit(Op::MakeCodeRef(block_idx), line);
            }

            // ── Derefs ──
            ExprKind::ArrowDeref { expr, index, kind } => {
                self.compile_expr(expr)?;
                self.compile_expr(index)?;
                match kind {
                    DerefKind::Array => {
                        self.chunk.emit(Op::ArrowArray, line);
                    }
                    DerefKind::Hash => {
                        self.chunk.emit(Op::ArrowHash, line);
                    }
                    DerefKind::Call => {
                        self.chunk.emit(Op::ArrowCall, line);
                    }
                }
            }
            ExprKind::Deref { expr, kind } => {
                self.compile_expr(expr)?;
                match kind {
                    Sigil::Scalar | Sigil::Array | Sigil::Hash => {
                        self.chunk
                            .emit(Op::CallBuiltin(BuiltinId::Scalar as u16, 1), line);
                    }
                }
            }

            // ── Interpolated strings ──
            ExprKind::InterpolatedString(parts) => {
                if parts.is_empty() {
                    let idx = self.chunk.add_constant(PerlValue::String(String::new()));
                    self.chunk.emit(Op::LoadConst(idx), line);
                } else {
                    self.compile_string_part(&parts[0], line)?;
                    for part in &parts[1..] {
                        self.compile_string_part(part, line)?;
                        self.chunk.emit(Op::Concat, line);
                    }
                }
            }

            // ── List ──
            ExprKind::List(exprs) => {
                for e in exprs {
                    self.compile_expr(e)?;
                }
                if exprs.len() != 1 {
                    self.chunk.emit(Op::MakeArray(exprs.len() as u16), line);
                }
            }

            // ── QW ──
            ExprKind::QW(words) => {
                for w in words {
                    let idx = self.chunk.add_constant(PerlValue::String(w.clone()));
                    self.chunk.emit(Op::LoadConst(idx), line);
                }
                self.chunk.emit(Op::MakeArray(words.len() as u16), line);
            }

            // ── Postfix if/unless ──
            ExprKind::PostfixIf { expr, condition } => {
                self.compile_expr(condition)?;
                let j = self.chunk.emit(Op::JumpIfFalse(0), line);
                self.compile_expr(expr)?;
                let end = self.chunk.emit(Op::Jump(0), line);
                self.chunk.patch_jump_here(j);
                self.chunk.emit(Op::LoadUndef, line);
                self.chunk.patch_jump_here(end);
            }
            ExprKind::PostfixUnless { expr, condition } => {
                self.compile_expr(condition)?;
                let j = self.chunk.emit(Op::JumpIfTrue(0), line);
                self.compile_expr(expr)?;
                let end = self.chunk.emit(Op::Jump(0), line);
                self.chunk.patch_jump_here(j);
                self.chunk.emit(Op::LoadUndef, line);
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
                    self.chunk.emit(Op::Pop, line);
                    self.compile_expr(condition)?;
                    self.chunk.emit(Op::JumpIfTrue(loop_start), line);
                    self.chunk.emit(Op::LoadUndef, line);
                } else {
                    // Regular postfix while: condition checked first
                    let loop_start = self.chunk.len();
                    self.compile_expr(condition)?;
                    let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);
                    self.compile_expr(expr)?;
                    self.chunk.emit(Op::Pop, line);
                    self.chunk.emit(Op::Jump(loop_start), line);
                    self.chunk.patch_jump_here(exit_jump);
                    self.chunk.emit(Op::LoadUndef, line);
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
                    self.chunk.emit(Op::Pop, line);
                    self.compile_expr(condition)?;
                    self.chunk.emit(Op::JumpIfFalse(loop_start), line);
                    self.chunk.emit(Op::LoadUndef, line);
                } else {
                    let loop_start = self.chunk.len();
                    self.compile_expr(condition)?;
                    let exit_jump = self.chunk.emit(Op::JumpIfTrue(0), line);
                    self.compile_expr(expr)?;
                    self.chunk.emit(Op::Pop, line);
                    self.chunk.emit(Op::Jump(loop_start), line);
                    self.chunk.patch_jump_here(exit_jump);
                    self.chunk.emit(Op::LoadUndef, line);
                }
            }
            ExprKind::PostfixForeach { expr, list } => {
                self.compile_expr(list)?;
                let list_name = self.chunk.intern_name("__pf_foreach_list__");
                self.chunk.emit(Op::DeclareArray(list_name), line);
                let counter = self.chunk.intern_name("__pf_foreach_i__");
                self.chunk.emit(Op::LoadInt(0), line);
                self.chunk.emit(Op::DeclareScalar(counter), line);
                let underscore = self.chunk.intern_name("_");

                let loop_start = self.chunk.len();
                self.chunk.emit(Op::GetScalar(counter), line);
                self.chunk.emit(Op::ArrayLen(list_name), line);
                self.chunk.emit(Op::NumLt, line);
                let exit_jump = self.chunk.emit(Op::JumpIfFalse(0), line);

                self.chunk.emit(Op::GetScalar(counter), line);
                self.chunk.emit(Op::GetArrayElem(list_name), line);
                self.chunk.emit(Op::SetScalar(underscore), line);

                self.compile_expr(expr)?;
                self.chunk.emit(Op::Pop, line);

                self.chunk.emit(Op::PreInc(counter), line);
                self.chunk.emit(Op::Pop, line);
                self.chunk.emit(Op::Jump(loop_start), line);
                self.chunk.patch_jump_here(exit_jump);
                self.chunk.emit(Op::LoadUndef, line);
            }

            // ── Match (regex) ──
            ExprKind::Match {
                expr,
                pattern,
                flags,
                scalar_g,
            } => {
                self.compile_expr(expr)?;
                let pat_idx = self.chunk.add_constant(PerlValue::String(pattern.clone()));
                let flags_idx = self.chunk.add_constant(PerlValue::String(flags.clone()));
                let pos_key_idx = if *scalar_g && flags.contains('g') {
                    if let ExprKind::ScalarVar(n) = &expr.kind {
                        self.chunk.add_constant(PerlValue::String(n.clone()))
                    } else {
                        u16::MAX
                    }
                } else {
                    u16::MAX
                };
                self.chunk.emit(
                    Op::RegexMatch(pat_idx, flags_idx, *scalar_g, pos_key_idx),
                    line,
                );
            }

            ExprKind::Substitution {
                expr,
                pattern,
                replacement,
                flags,
            } => {
                self.compile_expr(expr)?;
                let pat_idx = self.chunk.add_constant(PerlValue::String(pattern.clone()));
                let repl_idx = self
                    .chunk
                    .add_constant(PerlValue::String(replacement.clone()));
                let flags_idx = self.chunk.add_constant(PerlValue::String(flags.clone()));
                let lv_idx = self.chunk.add_lvalue_expr(expr.as_ref().clone());
                self.chunk
                    .emit(Op::RegexSubst(pat_idx, repl_idx, flags_idx, lv_idx), line);
            }
            ExprKind::Transliterate {
                expr,
                from,
                to,
                flags,
            } => {
                self.compile_expr(expr)?;
                let from_idx = self.chunk.add_constant(PerlValue::String(from.clone()));
                let to_idx = self.chunk.add_constant(PerlValue::String(to.clone()));
                let flags_idx = self.chunk.add_constant(PerlValue::String(flags.clone()));
                let lv_idx = self.chunk.add_lvalue_expr(expr.as_ref().clone());
                self.chunk.emit(
                    Op::RegexTransliterate(from_idx, to_idx, flags_idx, lv_idx),
                    line,
                );
            }

            // ── Regex literal ──
            ExprKind::Regex(_, _) => {
                return Err(CompileError::Unsupported("Regex literal as value".into()));
            }

            // ── Map/Grep/Sort with blocks ──
            ExprKind::MapExpr { block, list } => {
                self.compile_expr(list)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.chunk.emit(Op::MapWithBlock(block_idx), line);
            }
            ExprKind::GrepExpr { block, list } => {
                self.compile_expr(list)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.chunk.emit(Op::GrepWithBlock(block_idx), line);
            }
            ExprKind::SortExpr { cmp, list } => {
                self.compile_expr(list)?;
                if let Some(block) = cmp {
                    if let Some(mode) = detect_sort_block_fast(block) {
                        let tag = match mode {
                            crate::sort_fast::SortBlockFast::Numeric => 0u8,
                            crate::sort_fast::SortBlockFast::String => 1u8,
                        };
                        self.chunk.emit(Op::SortWithBlockFast(tag), line);
                    } else {
                        let block_idx = self.chunk.add_block(block.clone());
                        self.chunk.emit(Op::SortWithBlock(block_idx), line);
                    }
                } else {
                    self.chunk.emit(Op::SortNoBlock, line);
                }
            }

            // ── Parallel extensions ──
            ExprKind::PMapExpr { block, list } => {
                self.compile_expr(list)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.chunk.emit(Op::PMapWithBlock(block_idx), line);
            }
            ExprKind::PMapChunkedExpr { .. } => {
                return Err(CompileError::Unsupported("pmap_chunked".into()));
            }
            ExprKind::PGrepExpr { block, list } => {
                self.compile_expr(list)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.chunk.emit(Op::PGrepWithBlock(block_idx), line);
            }
            ExprKind::PForExpr { block, list } => {
                self.compile_expr(list)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.chunk.emit(Op::PForWithBlock(block_idx), line);
            }
            ExprKind::PSortExpr { cmp, list } => {
                self.compile_expr(list)?;
                if let Some(block) = cmp {
                    if let Some(mode) = detect_sort_block_fast(block) {
                        let tag = match mode {
                            crate::sort_fast::SortBlockFast::Numeric => 0u8,
                            crate::sort_fast::SortBlockFast::String => 1u8,
                        };
                        self.chunk.emit(Op::PSortWithBlockFast(tag), line);
                    } else {
                        let block_idx = self.chunk.add_block(block.clone());
                        self.chunk.emit(Op::PSortWithBlock(block_idx), line);
                    }
                } else {
                    self.chunk.emit(Op::SortNoBlock, line);
                }
            }
            ExprKind::ReduceExpr { .. } => {
                return Err(CompileError::Unsupported("reduce".into()));
            }
            ExprKind::PReduceExpr { .. } => {
                // No PReduce op — fall back to tree-walker
                return Err(CompileError::Unsupported("preduce".into()));
            }
            ExprKind::FanExpr { count, block } => {
                self.compile_expr(count)?;
                let block_idx = self.chunk.add_block(block.clone());
                self.chunk.emit(Op::FanWithBlock(block_idx), line);
            }
            ExprKind::AsyncBlock { body } => {
                let block_idx = self.chunk.add_block(body.clone());
                self.chunk.emit(Op::AsyncBlock(block_idx), line);
            }
            ExprKind::Trace { .. } => {
                return Err(CompileError::Unsupported("trace".into()));
            }
            ExprKind::Timer { .. } => {
                return Err(CompileError::Unsupported("timer".into()));
            }
            ExprKind::Await(e) => {
                self.compile_expr(e)?;
                self.chunk.emit(Op::Await, line);
            }
            ExprKind::Slurp(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Slurp as u16, 1), line);
            }
            ExprKind::Capture(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Capture as u16, 1), line);
            }
            ExprKind::FetchUrl(e) => {
                self.compile_expr(e)?;
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::FetchUrl as u16, 1), line);
            }
            ExprKind::Pchannel => {
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Pchannel as u16, 0), line);
            }
        }
        Ok(())
    }

    fn compile_string_part(&mut self, part: &StringPart, line: usize) -> Result<(), CompileError> {
        match part {
            StringPart::Literal(s) => {
                let idx = self.chunk.add_constant(PerlValue::String(s.clone()));
                self.chunk.emit(Op::LoadConst(idx), line);
            }
            StringPart::ScalarVar(name) => {
                let idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::GetScalar(idx), line);
            }
            StringPart::ArrayVar(name) => {
                let idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::GetArray(idx), line);
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
    ) -> Result<(), CompileError> {
        match &target.kind {
            ExprKind::ScalarVar(name) => {
                self.check_scalar_mutable(name, line)?;
                let idx = self.chunk.intern_name(name);
                if keep {
                    self.chunk.emit(Op::SetScalarKeep(idx), line);
                } else {
                    self.chunk.emit(Op::SetScalar(idx), line);
                }
            }
            ExprKind::ArrayVar(name) => {
                self.check_array_mutable(name, line)?;
                let idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::SetArray(idx), line);
                if keep {
                    self.chunk.emit(Op::GetArray(idx), line);
                }
            }
            ExprKind::HashVar(name) => {
                self.check_hash_mutable(name, line)?;
                let idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::SetHash(idx), line);
                if keep {
                    self.chunk.emit(Op::GetHash(idx), line);
                }
            }
            ExprKind::ArrayElement { array, index } => {
                self.check_array_mutable(array, line)?;
                let idx = self.chunk.intern_name(array);
                self.compile_expr(index)?;
                self.chunk.emit(Op::SetArrayElem(idx), line);
            }
            ExprKind::HashElement { hash, key } => {
                self.check_hash_mutable(hash, line)?;
                let idx = self.chunk.intern_name(hash);
                self.compile_expr(key)?;
                self.chunk.emit(Op::SetHashElem(idx), line);
            }
            ExprKind::ArrowDeref { .. } => {
                return Err(CompileError::Unsupported("Assign to arrow deref".into()));
            }
            _ => {
                return Err(CompileError::Unsupported("Assign to complex lvalue".into()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{BuiltinId, Op};
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
    fn compile_empty_program_emits_only_halt() {
        let chunk = compile_snippet("").expect("compile");
        assert_eq!(chunk.ops.len(), 1);
        assert!(matches!(chunk.ops[0], Op::Halt));
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
            .any(|c| { matches!(c, crate::value::PerlValue::String(s) if s == "hello") }));
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
                .filter(|o| matches!(o, Op::GetScalar(_)))
                .count()
                >= 1
        );
        assert_last_halt(&chunk);
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
            .any(|(idx, ip)| chunk.names[*idx as usize] == "foo" && *ip > 0));
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
}
