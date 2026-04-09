use crate::ast::*;
use crate::bytecode::{BuiltinId, Chunk, Op};
use crate::value::PerlValue;

/// Compilation error — triggers fallback to tree-walker.
#[derive(Debug)]
pub enum CompileError {
    Unsupported(String),
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
        }
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
                        Self::emit_block_value(&mut self.chunk, body, stmt.line)?;
                        let mut ends = vec![self.chunk.emit(Op::Jump(0), stmt.line)];
                        self.chunk.patch_jump_here(j0);
                        for (c, blk) in elsifs {
                            self.compile_expr(c)?;
                            let j = self.chunk.emit(Op::JumpIfFalse(0), c.line);
                            Self::emit_block_value(&mut self.chunk, blk, c.line)?;
                            ends.push(self.chunk.emit(Op::Jump(0), c.line));
                            self.chunk.patch_jump_here(j);
                        }
                        if let Some(eb) = else_block {
                            Self::emit_block_value(&mut self.chunk, eb, stmt.line)?;
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
                            Self::emit_block_value(&mut self.chunk, eb, stmt.line)?;
                        } else {
                            self.chunk.emit(Op::LoadUndef, stmt.line);
                        }
                        let end = self.chunk.emit(Op::Jump(0), stmt.line);
                        self.chunk.patch_jump_here(j0);
                        Self::emit_block_value(&mut self.chunk, body, stmt.line)?;
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
            let entry_ip = self.chunk.len();
            let name_idx = self.chunk.intern_name(name);
            // Patch the entry point
            for e in &mut self.chunk.sub_entries {
                if e.0 == name_idx {
                    e.1 = entry_ip;
                }
            }
            // Compile sub body
            for stmt in body {
                self.compile_statement(stmt)?;
            }
            // Implicit return undef
            self.chunk.emit(Op::LoadUndef, 0);
            self.chunk.emit(Op::ReturnValue, 0);
        }

        Ok(self.chunk)
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), CompileError> {
        let line = stmt.line;
        match &stmt.kind {
            StmtKind::Expression(expr) => {
                self.compile_expr(expr)?;
                self.chunk.emit(Op::Pop, line);
            }
            StmtKind::Local(_) => {
                return Err(CompileError::Unsupported("local".into()));
            }
            StmtKind::My(decls) | StmtKind::Our(decls) => {
                for decl in decls {
                    let name_idx = self.chunk.intern_name(&decl.name);
                    match decl.sigil {
                        Sigil::Scalar => {
                            if let Some(init) = &decl.initializer {
                                self.compile_expr(init)?;
                            } else {
                                self.chunk.emit(Op::LoadUndef, line);
                            }
                            self.chunk.emit(Op::DeclareScalar(name_idx), line);
                        }
                        Sigil::Array => {
                            if let Some(init) = &decl.initializer {
                                self.compile_expr(init)?;
                            } else {
                                self.chunk.emit(Op::LoadUndef, line);
                            }
                            self.chunk.emit(Op::DeclareArray(name_idx), line);
                        }
                        Sigil::Hash => {
                            if let Some(init) = &decl.initializer {
                                self.compile_expr(init)?;
                            } else {
                                self.chunk.emit(Op::LoadUndef, line);
                            }
                            self.chunk.emit(Op::DeclareHash(name_idx), line);
                        }
                    }
                }
            }
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
            self.chunk.emit(Op::PushFrame, 0);
            self.compile_block_inner(block)?;
            self.chunk.emit(Op::PopFrame, 0);
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
    fn emit_block_value(chunk: &mut Chunk, block: &Block, line: usize) -> Result<(), CompileError> {
        if block.is_empty() {
            chunk.emit(Op::LoadUndef, line);
            return Ok(());
        }
        // Compile all but last statement normally (via a temporary compiler is too complex;
        // instead, just compile the last expression inline).
        // For simple blocks like { 1 } or { $x }, the last statement is the expression.
        let last = &block[block.len() - 1];
        if let StmtKind::Expression(expr) = &last.kind {
            // Single expression block — compile inline
            if block.len() == 1 {
                let mut comp = Compiler {
                    chunk: std::mem::take(chunk),
                    begin_blocks: Vec::new(),
                    end_blocks: Vec::new(),
                };
                comp.compile_expr(expr)?;
                *chunk = comp.chunk;
                return Ok(());
            }
        }
        // Fallback: compile all statements, push Undef as value
        let mut comp = Compiler {
            chunk: std::mem::take(chunk),
            begin_blocks: Vec::new(),
            end_blocks: Vec::new(),
        };
        for stmt in block {
            comp.compile_statement(stmt)?;
        }
        comp.chunk.emit(Op::LoadUndef, line);
        *chunk = comp.chunk;
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
                        let idx = self.chunk.intern_name(name);
                        self.chunk.emit(Op::PreInc(idx), line);
                    } else {
                        return Err(CompileError::Unsupported("PreInc on non-scalar".into()));
                    }
                }
                UnaryOp::PreDecrement => {
                    if let ExprKind::ScalarVar(name) = &expr.kind {
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
            ExprKind::FuncCall { name, args } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                let name_idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::Call(name_idx, args.len() as u8), line);
            }

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
            ExprKind::Chomp(_) | ExprKind::Chop(_) => {
                // chomp/chop modify variables in-place; needs special VM support
                return Err(CompileError::Unsupported("chomp/chop".into()));
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
                // No Wantarray BuiltinId — fall back to tree-walker
                return Err(CompileError::Unsupported("wantarray".into()));
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
            } => {
                self.compile_expr(expr)?;
                let pat_idx = self.chunk.add_constant(PerlValue::String(pattern.clone()));
                let flags_idx = self.chunk.add_constant(PerlValue::String(flags.clone()));
                self.chunk.emit(Op::RegexMatch(pat_idx, flags_idx), line);
            }

            // ── Substitution / Transliterate — no BuiltinId, fall back ──
            ExprKind::Substitution { .. } => {
                return Err(CompileError::Unsupported("s///".into()));
            }
            ExprKind::Transliterate { .. } => {
                return Err(CompileError::Unsupported("tr///".into()));
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
                    let block_idx = self.chunk.add_block(block.clone());
                    self.chunk.emit(Op::SortWithBlock(block_idx), line);
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
                    let block_idx = self.chunk.add_block(block.clone());
                    self.chunk.emit(Op::PSortWithBlock(block_idx), line);
                } else {
                    self.chunk.emit(Op::SortNoBlock, line);
                }
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
                let idx = self.chunk.intern_name(name);
                if keep {
                    self.chunk.emit(Op::SetScalarKeep(idx), line);
                } else {
                    self.chunk.emit(Op::SetScalar(idx), line);
                }
            }
            ExprKind::ArrayVar(name) => {
                let idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::SetArray(idx), line);
                if keep {
                    self.chunk.emit(Op::GetArray(idx), line);
                }
            }
            ExprKind::HashVar(name) => {
                let idx = self.chunk.intern_name(name);
                self.chunk.emit(Op::SetHash(idx), line);
                if keep {
                    self.chunk.emit(Op::GetHash(idx), line);
                }
            }
            ExprKind::ArrayElement { array, index } => {
                let idx = self.chunk.intern_name(array);
                self.compile_expr(index)?;
                self.chunk.emit(Op::SetArrayElem(idx), line);
            }
            ExprKind::HashElement { hash, key } => {
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
