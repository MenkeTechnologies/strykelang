use crate::ast::*;
use crate::bytecode::{BuiltinId, Chunk, Op};
use crate::value::PerlValue;

/// Compilation error — triggers fallback to tree-walker.
#[derive(Debug)]
pub enum CompileError {
    Unsupported(String),
}

/// Loop context for resolving `last`/`next` jumps.
#[allow(dead_code)]
struct LoopCtx {
    label: Option<String>,
    /// Positions of `last` jumps to patch (jump to after loop).
    break_jumps: Vec<usize>,
    /// Target address for `next` (jump to loop step/condition).
    continue_target: usize,
}

pub struct Compiler {
    pub chunk: Chunk,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
        }
    }

    pub fn compile_program(mut self, program: &Program) -> Result<Chunk, CompileError> {
        // First pass: register sub entry points (emit placeholder jumps)
        // We'll compile subs at the end of the bytecode, after a Halt.
        for stmt in &program.statements {
            if let StmtKind::SubDecl { name, .. } = &stmt.kind {
                let name_idx = self.chunk.intern_name(name);
                // Will be patched later
                self.chunk.sub_entries.push((name_idx, 0));
            }
        }

        // Second pass: compile main body
        for stmt in &program.statements {
            if matches!(stmt.kind, StmtKind::SubDecl { .. }) {
                continue; // subs compiled separately below
            }
            self.compile_statement(stmt)?;
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
            StmtKind::My(decls) | StmtKind::Our(decls) | StmtKind::Local(decls) => {
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
                    // We need to save exit jump to patch later — use a temp vec
                    let _step_target = self.chunk.len(); // approximate; will be after body

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
                    let _end = self.chunk.len();
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
                // For simplicity: compile to equivalent while loop with index
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
            StmtKind::Return(val) => {
                if let Some(expr) = val {
                    self.compile_expr(expr)?;
                    self.chunk.emit(Op::ReturnValue, line);
                } else {
                    self.chunk.emit(Op::Return, line);
                }
            }
            StmtKind::Last(_label) => {
                // Emit a Jump(0) placeholder — will be patched by the loop context.
                // For now, this only works for innermost loop.
                self.chunk.emit(Op::Jump(0), line); // marker: needs patching
            }
            StmtKind::Next(_label) => {
                // Jump to continue target — patched by loop context.
                self.chunk.emit(Op::Jump(0), line); // marker: needs patching
            }
            StmtKind::Block(block) => {
                self.chunk.emit(Op::PushFrame, line);
                self.compile_block_inner(block)?;
                self.chunk.emit(Op::PopFrame, line);
            }
            StmtKind::SubDecl { .. } => {
                // Already handled in compile_program
            }
            StmtKind::Package { .. }
            | StmtKind::Use { .. }
            | StmtKind::No { .. }
            | StmtKind::Begin(_)
            | StmtKind::End(_)
            | StmtKind::Empty
            | StmtKind::Redo(_) => {
                // No-ops or handled elsewhere
            }
            _ => {
                return Err(CompileError::Unsupported(format!(
                    "Statement: {:?}",
                    std::mem::discriminant(&stmt.kind)
                )));
            }
        }
        Ok(())
    }

    fn compile_block(&mut self, block: &Block) -> Result<(), CompileError> {
        self.chunk.emit(Op::PushFrame, 0);
        self.compile_block_inner(block)?;
        self.chunk.emit(Op::PopFrame, 0);
        Ok(())
    }

    fn compile_block_inner(&mut self, block: &Block) -> Result<(), CompileError> {
        for stmt in block {
            self.compile_statement(stmt)?;
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
                    BinOp::LogAnd | BinOp::LogOr | BinOp::DefinedOr
                    | BinOp::LogAndWord | BinOp::LogOrWord => unreachable!(),
                    BinOp::BindMatch | BinOp::BindNotMatch => {
                        return Err(CompileError::Unsupported("BindMatch in BinOp".into()));
                    }
                };
                self.chunk.emit(op_code, line);
            }

            ExprKind::UnaryOp { op, expr } => {
                match op {
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
                    _ => {
                        self.compile_expr(expr)?;
                        match op {
                            UnaryOp::Negate => { self.chunk.emit(Op::Negate, line); }
                            UnaryOp::LogNot | UnaryOp::LogNotWord => { self.chunk.emit(Op::LogNot, line); }
                            UnaryOp::BitNot => { self.chunk.emit(Op::BitNot, line); }
                            UnaryOp::Ref => {
                                return Err(CompileError::Unsupported("Ref unary".into()));
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
            ExprKind::PostfixOp { expr, op } => {
                if let ExprKind::ScalarVar(name) = &expr.kind {
                    let idx = self.chunk.intern_name(name);
                    match op {
                        PostfixOp::Increment => { self.chunk.emit(Op::PostInc(idx), line); }
                        PostfixOp::Decrement => { self.chunk.emit(Op::PostDec(idx), line); }
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
                    return Err(CompileError::Unsupported("CompoundAssign on non-scalar".into()));
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

            // ── Print / Say ──
            ExprKind::Print { handle: None, args } | ExprKind::Print { handle: Some(_), args } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.emit(Op::Print(args.len() as u8), line);
            }
            ExprKind::Say { handle: None, args } | ExprKind::Say { handle: Some(_), args } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk.emit(Op::Say(args.len() as u8), line);
            }

            // ── Die / Warn ──
            ExprKind::Die(args) => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Die as u16, args.len() as u8), line);
            }
            ExprKind::Warn(args) => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.chunk
                    .emit(Op::CallBuiltin(BuiltinId::Warn as u16, args.len() as u8), line);
            }
            ExprKind::Exit(code) => {
                if let Some(c) = code {
                    self.compile_expr(c)?;
                    self.chunk.emit(Op::CallBuiltin(BuiltinId::Exit as u16, 1), line);
                } else {
                    self.chunk.emit(Op::LoadInt(0), line);
                    self.chunk.emit(Op::CallBuiltin(BuiltinId::Exit as u16, 1), line);
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

            // ── Builtins that map to CallBuiltin ──
            ExprKind::Length(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Length as u16, 1), line); }
            ExprKind::Chomp(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Chomp as u16, 1), line); }
            ExprKind::Defined(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Defined as u16, 1), line); }
            ExprKind::Abs(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Abs as u16, 1), line); }
            ExprKind::Int(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Int as u16, 1), line); }
            ExprKind::Sqrt(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Sqrt as u16, 1), line); }
            ExprKind::Chr(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Chr as u16, 1), line); }
            ExprKind::Ord(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Ord as u16, 1), line); }
            ExprKind::Hex(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Hex as u16, 1), line); }
            ExprKind::Oct(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Oct as u16, 1), line); }
            ExprKind::Uc(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Uc as u16, 1), line); }
            ExprKind::Lc(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Lc as u16, 1), line); }
            ExprKind::Ref(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Ref as u16, 1), line); }
            ExprKind::ReverseExpr(e) => { self.compile_expr(e)?; self.chunk.emit(Op::CallBuiltin(BuiltinId::Reverse as u16, 1), line); }
            ExprKind::System(args) => {
                for a in args { self.compile_expr(a)?; }
                self.chunk.emit(Op::CallBuiltin(BuiltinId::System as u16, args.len() as u8), line);
            }

            ExprKind::JoinExpr { separator, list } => {
                self.compile_expr(separator)?;
                self.compile_expr(list)?;
                self.chunk.emit(Op::CallBuiltin(BuiltinId::Join as u16, 2), line);
            }
            ExprKind::SplitExpr { pattern, string, limit } => {
                self.compile_expr(pattern)?;
                self.compile_expr(string)?;
                if let Some(l) = limit {
                    self.compile_expr(l)?;
                    self.chunk.emit(Op::CallBuiltin(BuiltinId::Split as u16, 3), line);
                } else {
                    self.chunk.emit(Op::CallBuiltin(BuiltinId::Split as u16, 2), line);
                }
            }
            ExprKind::Sprintf { format, args } => {
                self.compile_expr(format)?;
                for a in args { self.compile_expr(a)?; }
                self.chunk.emit(Op::CallBuiltin(BuiltinId::Sprintf as u16, (1 + args.len()) as u8), line);
            }

            // ── Interpolated strings ──
            ExprKind::InterpolatedString(parts) => {
                if parts.is_empty() {
                    let idx = self.chunk.add_constant(PerlValue::String(String::new()));
                    self.chunk.emit(Op::LoadConst(idx), line);
                } else {
                    // Compile first part
                    self.compile_string_part(&parts[0], line)?;
                    // Concat remaining parts
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

            // ── Array/Hash refs ──
            ExprKind::ArrayRef(elems) => {
                for e in elems {
                    self.compile_expr(e)?;
                }
                self.chunk.emit(Op::MakeArray(elems.len() as u16), line);
                // TODO: wrap in ArrayRef
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
            ExprKind::PostfixForeach { expr: _, list } => {
                // Compile as: for $_ (list) { expr }
                self.compile_expr(list)?;
                // We need a loop — fall back for now
                return Err(CompileError::Unsupported("PostfixForeach".into()));
            }

            // ── Match (regex) ──
            ExprKind::Match { expr, pattern, flags } => {
                self.compile_expr(expr)?;
                let pat_idx = self.chunk.add_constant(PerlValue::String(pattern.clone()));
                let flags_idx = self.chunk.add_constant(PerlValue::String(flags.clone()));
                self.chunk.emit(Op::RegexMatch(pat_idx, flags_idx), line);
            }

            // ── Regex literal ──
            ExprKind::Regex(_, _) => {
                // Regex as value — used in match context
                return Err(CompileError::Unsupported("Regex literal as value".into()));
            }

            // ── Map/Grep/Sort (block-based) — fall back ──
            ExprKind::MapExpr { .. }
            | ExprKind::GrepExpr { .. }
            | ExprKind::SortExpr { .. }
            | ExprKind::PMapExpr { .. }
            | ExprKind::PGrepExpr { .. }
            | ExprKind::PForExpr { .. }
            | ExprKind::PSortExpr { .. }
            | ExprKind::FanExpr { .. } => {
                return Err(CompileError::Unsupported("Block-based list op".into()));
            }

            // ── Anything else: fall back to tree-walker ──
            _ => {
                return Err(CompileError::Unsupported(format!(
                    "Expr: {:?}",
                    std::mem::discriminant(&expr.kind)
                )));
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

    fn compile_assign(&mut self, target: &Expr, line: usize, keep: bool) -> Result<(), CompileError> {
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
            _ => {
                return Err(CompileError::Unsupported("Assign to complex lvalue".into()));
            }
        }
        Ok(())
    }
}
