//! The fusevm execution engine — stack-based bytecode dispatch loop.
//!
//! This is the hot path. Every cycle counts. The dispatch loop uses
//! a flat `match` on `Op` variants — Rust compiles this to a jump table.
//!
//! Frontends register extension handlers via `ExtensionHandler` for
//! language-specific opcodes (`Op::Extended`, `Op::ExtendedWide`).

use crate::chunk::Chunk;
use crate::op::Op;
use crate::value::Value;

/// Call frame on the frame stack.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Return address (ip to resume after call)
    pub return_ip: usize,
    /// Base pointer into the value stack (locals start here)
    pub stack_base: usize,
    /// Local variable slots (indexed by `GetSlot`/`SetSlot`)
    pub slots: Vec<Value>,
}

/// Extension handler for language-specific opcodes.
/// Frontends register this at VM init.
pub type ExtensionHandler = Box<dyn FnMut(&mut VM, u16, u8) + Send>;
/// Wide extension handler (usize payload).
pub type ExtensionWideHandler = Box<dyn FnMut(&mut VM, u16, usize) + Send>;

/// The virtual machine.
pub struct VM {
    /// Value stack
    pub stack: Vec<Value>,
    /// Call frame stack
    pub frames: Vec<Frame>,
    /// Global variables (name pool index → value)
    pub globals: Vec<Value>,
    /// Instruction pointer
    pub ip: usize,
    /// Current chunk being executed
    pub chunk: Chunk,
    /// Last exit status ($?)
    pub last_status: i32,
    /// Extension handler for `Op::Extended`
    ext_handler: Option<ExtensionHandler>,
    /// Extension handler for `Op::ExtendedWide`
    ext_wide_handler: Option<ExtensionWideHandler>,
    /// Halted flag
    halted: bool,
}

/// Result of VM execution
#[derive(Debug)]
pub enum VMResult {
    /// Normal completion with a value
    Ok(Value),
    /// Halted (no more instructions)
    Halted,
    /// Runtime error
    Error(String),
}

impl VM {
    pub fn new(chunk: Chunk) -> Self {
        let num_names = chunk.names.len();
        Self {
            stack: Vec::with_capacity(256),
            frames: Vec::with_capacity(32),
            globals: vec![Value::Undef; num_names],
            ip: 0,
            chunk,
            last_status: 0,
            ext_handler: None,
            ext_wide_handler: None,
            halted: false,
        }
    }

    /// Register a handler for `Op::Extended(id, arg)` opcodes.
    pub fn set_extension_handler(&mut self, handler: ExtensionHandler) {
        self.ext_handler = Some(handler);
    }

    /// Register a handler for `Op::ExtendedWide(id, payload)` opcodes.
    pub fn set_extension_wide_handler(&mut self, handler: ExtensionWideHandler) {
        self.ext_wide_handler = Some(handler);
    }

    // ── Stack operations ──

    #[inline(always)]
    pub fn push(&mut self, val: Value) {
        self.stack.push(val);
    }

    #[inline(always)]
    pub fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::Undef)
    }

    #[inline(always)]
    pub fn peek(&self) -> &Value {
        self.stack.last().unwrap_or(&Value::Undef)
    }

    // ── Main dispatch loop ──

    /// Execute the loaded chunk until completion or error.
    pub fn run(&mut self) -> VMResult {
        while self.ip < self.chunk.ops.len() && !self.halted {
            let op = self.chunk.ops[self.ip].clone();
            self.ip += 1;

            match op {
                Op::Nop => {}

                // ── Constants ──
                Op::LoadInt(n) => self.push(Value::Int(n)),
                Op::LoadFloat(f) => self.push(Value::Float(f)),
                Op::LoadConst(idx) => {
                    let val = self.chunk.constants.get(idx as usize).cloned().unwrap_or(Value::Undef);
                    self.push(val);
                }
                Op::LoadTrue => self.push(Value::Bool(true)),
                Op::LoadFalse => self.push(Value::Bool(false)),
                Op::LoadUndef => self.push(Value::Undef),

                // ── Stack ──
                Op::Pop => { self.pop(); }
                Op::Dup => {
                    let val = self.peek().clone();
                    self.push(val);
                }
                Op::Dup2 => {
                    let len = self.stack.len();
                    if len >= 2 {
                        let a = self.stack[len - 2].clone();
                        let b = self.stack[len - 1].clone();
                        self.push(a);
                        self.push(b);
                    }
                }
                Op::Swap => {
                    let len = self.stack.len();
                    if len >= 2 {
                        self.stack.swap(len - 1, len - 2);
                    }
                }
                Op::Rot => {
                    let len = self.stack.len();
                    if len >= 3 {
                        let a = self.stack.remove(len - 3);
                        self.stack.push(a);
                    }
                }

                // ── Variables ──
                Op::GetVar(idx) => {
                    let val = self.get_var(idx);
                    self.push(val);
                }
                Op::SetVar(idx) => {
                    let val = self.pop();
                    self.set_var(idx, val);
                }
                Op::DeclareVar(idx) => {
                    let val = self.pop();
                    self.set_var(idx, val);
                }
                Op::GetSlot(slot) => {
                    let val = self.get_slot(slot);
                    self.push(val);
                }
                Op::SetSlot(slot) => {
                    let val = self.pop();
                    self.set_slot(slot, val);
                }

                // ── Arithmetic ──
                Op::Add => self.binary_op(|a, b| match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) => Value::Int(x.wrapping_add(*y)),
                    _ => Value::Float(a.to_float() + b.to_float()),
                }),
                Op::Sub => self.binary_op(|a, b| match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) => Value::Int(x.wrapping_sub(*y)),
                    _ => Value::Float(a.to_float() - b.to_float()),
                }),
                Op::Mul => self.binary_op(|a, b| match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) => Value::Int(x.wrapping_mul(*y)),
                    _ => Value::Float(a.to_float() * b.to_float()),
                }),
                Op::Div => self.binary_op(|a, b| {
                    let divisor = b.to_float();
                    if divisor == 0.0 {
                        Value::Undef
                    } else {
                        Value::Float(a.to_float() / divisor)
                    }
                }),
                Op::Mod => self.binary_op(|a, b| match (&a, &b) {
                    (Value::Int(x), Value::Int(y)) if *y != 0 => Value::Int(x % y),
                    _ => Value::Float(a.to_float() % b.to_float()),
                }),
                Op::Pow => self.binary_op(|a, b| {
                    Value::Float(a.to_float().powf(b.to_float()))
                }),
                Op::Negate => {
                    let val = self.pop();
                    self.push(match val {
                        Value::Int(n) => Value::Int(-n),
                        _ => Value::Float(-val.to_float()),
                    });
                }
                Op::Inc => {
                    let val = self.pop();
                    self.push(Value::Int(val.to_int() + 1));
                }
                Op::Dec => {
                    let val = self.pop();
                    self.push(Value::Int(val.to_int() - 1));
                }

                // ── String ──
                Op::Concat => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(Value::str(format!("{}{}", a.to_str(), b.to_str())));
                }
                Op::StringRepeat => {
                    let count = self.pop().to_int();
                    let s = self.pop().to_str();
                    self.push(Value::str(s.repeat(count.max(0) as usize)));
                }
                Op::StringLen => {
                    let s = self.pop();
                    self.push(Value::Int(s.len() as i64));
                }

                // ── Comparison (numeric) ──
                Op::NumEq => self.cmp_op(|a, b| a.to_float() == b.to_float()),
                Op::NumNe => self.cmp_op(|a, b| a.to_float() != b.to_float()),
                Op::NumLt => self.cmp_op(|a, b| a.to_float() < b.to_float()),
                Op::NumGt => self.cmp_op(|a, b| a.to_float() > b.to_float()),
                Op::NumLe => self.cmp_op(|a, b| a.to_float() <= b.to_float()),
                Op::NumGe => self.cmp_op(|a, b| a.to_float() >= b.to_float()),
                Op::Spaceship => {
                    let b = self.pop().to_float();
                    let a = self.pop().to_float();
                    self.push(Value::Int(if a < b { -1 } else if a > b { 1 } else { 0 }));
                }

                // ── Comparison (string) ──
                Op::StrEq => self.cmp_op(|a, b| a.to_str() == b.to_str()),
                Op::StrNe => self.cmp_op(|a, b| a.to_str() != b.to_str()),
                Op::StrLt => self.cmp_op(|a, b| a.to_str() < b.to_str()),
                Op::StrGt => self.cmp_op(|a, b| a.to_str() > b.to_str()),
                Op::StrLe => self.cmp_op(|a, b| a.to_str() <= b.to_str()),
                Op::StrGe => self.cmp_op(|a, b| a.to_str() >= b.to_str()),
                Op::StrCmp => {
                    let b = self.pop().to_str();
                    let a = self.pop().to_str();
                    self.push(Value::Int(match a.cmp(&b) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    }));
                }

                // ── Logical / Bitwise ──
                Op::LogNot => {
                    let val = self.pop();
                    self.push(Value::Bool(!val.is_truthy()));
                }
                Op::LogAnd => self.cmp_op(|a, b| a.is_truthy() && b.is_truthy()),
                Op::LogOr => self.cmp_op(|a, b| a.is_truthy() || b.is_truthy()),
                Op::BitAnd => self.binary_op(|a, b| Value::Int(a.to_int() & b.to_int())),
                Op::BitOr => self.binary_op(|a, b| Value::Int(a.to_int() | b.to_int())),
                Op::BitXor => self.binary_op(|a, b| Value::Int(a.to_int() ^ b.to_int())),
                Op::BitNot => {
                    let val = self.pop();
                    self.push(Value::Int(!val.to_int()));
                }
                Op::Shl => self.binary_op(|a, b| Value::Int(a.to_int() << (b.to_int() as u32 & 63))),
                Op::Shr => self.binary_op(|a, b| Value::Int(a.to_int() >> (b.to_int() as u32 & 63))),

                // ── Control flow ──
                Op::Jump(target) => self.ip = target,
                Op::JumpIfTrue(target) => {
                    if self.pop().is_truthy() { self.ip = target; }
                }
                Op::JumpIfFalse(target) => {
                    if !self.pop().is_truthy() { self.ip = target; }
                }
                Op::JumpIfTrueKeep(target) => {
                    if self.peek().is_truthy() { self.ip = target; }
                }
                Op::JumpIfFalseKeep(target) => {
                    if !self.peek().is_truthy() { self.ip = target; }
                }

                // ── Functions ──
                Op::Call(name_idx, argc) => {
                    if let Some(entry_ip) = self.chunk.find_sub(name_idx) {
                        self.frames.push(Frame {
                            return_ip: self.ip,
                            stack_base: self.stack.len() - argc as usize,
                            slots: Vec::new(),
                        });
                        self.ip = entry_ip;
                    } else {
                        return VMResult::Error(format!(
                            "undefined function: {}",
                            self.chunk.names.get(name_idx as usize).map(|s| s.as_str()).unwrap_or("?")
                        ));
                    }
                }
                Op::Return => {
                    if let Some(frame) = self.frames.pop() {
                        self.stack.truncate(frame.stack_base);
                        self.ip = frame.return_ip;
                    } else {
                        self.halted = true;
                    }
                }
                Op::ReturnValue => {
                    let val = self.pop();
                    if let Some(frame) = self.frames.pop() {
                        self.stack.truncate(frame.stack_base);
                        self.ip = frame.return_ip;
                        self.push(val);
                    } else {
                        self.halted = true;
                        return VMResult::Ok(val);
                    }
                }

                // ── Scope ──
                Op::PushFrame => {
                    self.frames.push(Frame {
                        return_ip: self.ip,
                        stack_base: self.stack.len(),
                        slots: Vec::new(),
                    });
                }
                Op::PopFrame => {
                    if let Some(frame) = self.frames.pop() {
                        self.stack.truncate(frame.stack_base);
                    }
                }

                // ── I/O ──
                Op::Print(n) => {
                    let start = self.stack.len().saturating_sub(n as usize);
                    let vals: Vec<String> = self.stack[start..].iter().map(|v| v.to_str()).collect();
                    self.stack.truncate(start);
                    print!("{}", vals.join(""));
                }
                Op::PrintLn(n) => {
                    let start = self.stack.len().saturating_sub(n as usize);
                    let vals: Vec<String> = self.stack[start..].iter().map(|v| v.to_str()).collect();
                    self.stack.truncate(start);
                    println!("{}", vals.join(""));
                }
                Op::ReadLine => {
                    let mut line = String::new();
                    let _ = std::io::stdin().read_line(&mut line);
                    self.push(Value::str(line.trim_end_matches('\n')));
                }

                // ── Fused superinstructions ──
                Op::PreIncSlot(slot) => {
                    let val = self.get_slot(slot).to_int() + 1;
                    self.set_slot(slot, Value::Int(val));
                    self.push(Value::Int(val));
                }
                Op::PreIncSlotVoid(slot) => {
                    let val = self.get_slot(slot).to_int() + 1;
                    self.set_slot(slot, Value::Int(val));
                }
                Op::SlotLtIntJumpIfFalse(slot, limit, target) => {
                    if self.get_slot(slot).to_int() >= limit as i64 {
                        self.ip = target;
                    }
                }
                Op::SlotIncLtIntJumpBack(slot, limit, target) => {
                    let val = self.get_slot(slot).to_int() + 1;
                    self.set_slot(slot, Value::Int(val));
                    if val < limit as i64 {
                        self.ip = target;
                    }
                }
                Op::AccumSumLoop(sum_slot, i_slot, limit) => {
                    let mut sum = self.get_slot(sum_slot).to_int();
                    let mut i = self.get_slot(i_slot).to_int();
                    let lim = limit as i64;
                    while i < lim {
                        sum += i;
                        i += 1;
                    }
                    self.set_slot(sum_slot, Value::Int(sum));
                    self.set_slot(i_slot, Value::Int(i));
                }
                Op::AddAssignSlotVoid(a, b) => {
                    let sum = self.get_slot(a).to_int() + self.get_slot(b).to_int();
                    self.set_slot(a, Value::Int(sum));
                }

                // ── Status ──
                Op::SetStatus => {
                    self.last_status = self.pop().to_int() as i32;
                }
                Op::GetStatus => {
                    self.push(Value::Status(self.last_status));
                }

                // ── Extension dispatch ──
                Op::Extended(id, arg) => {
                    if let Some(mut handler) = self.ext_handler.take() {
                        handler(self, id, arg);
                        self.ext_handler = Some(handler);
                    }
                }
                Op::ExtendedWide(id, payload) => {
                    if let Some(mut handler) = self.ext_wide_handler.take() {
                        handler(self, id, payload);
                        self.ext_wide_handler = Some(handler);
                    }
                }

                // ── Shell ops, arrays, hashes, collections, etc. ──
                // These will be filled in as we wire up frontends.
                // For now, unimplemented ops are no-ops with a trace.
                _ => {
                    tracing::trace!(op = ?op, ip = self.ip - 1, "unimplemented op");
                }
            }
        }

        if let Some(val) = self.stack.pop() {
            VMResult::Ok(val)
        } else {
            VMResult::Halted
        }
    }

    // ── Helpers ──

    #[inline(always)]
    fn binary_op(&mut self, f: impl FnOnce(Value, Value) -> Value) {
        let b = self.pop();
        let a = self.pop();
        self.push(f(a, b));
    }

    #[inline(always)]
    fn cmp_op(&mut self, f: impl FnOnce(&Value, &Value) -> bool) {
        let b = self.pop();
        let a = self.pop();
        self.push(Value::Bool(f(&a, &b)));
    }

    fn get_var(&self, idx: u16) -> Value {
        self.globals.get(idx as usize).cloned().unwrap_or(Value::Undef)
    }

    fn set_var(&mut self, idx: u16, val: Value) {
        let idx = idx as usize;
        if idx >= self.globals.len() {
            self.globals.resize(idx + 1, Value::Undef);
        }
        self.globals[idx] = val;
    }

    fn get_slot(&self, slot: u8) -> Value {
        self.frames
            .last()
            .and_then(|f| f.slots.get(slot as usize))
            .cloned()
            .unwrap_or(Value::Undef)
    }

    fn set_slot(&mut self, slot: u8, val: Value) {
        if let Some(frame) = self.frames.last_mut() {
            let idx = slot as usize;
            if idx >= frame.slots.len() {
                frame.slots.resize(idx + 1, Value::Undef);
            }
            frame.slots[idx] = val;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::ChunkBuilder;

    #[test]
    fn test_arithmetic() {
        let mut b = ChunkBuilder::new();
        b.emit(Op::LoadInt(10), 1);
        b.emit(Op::LoadInt(32), 1);
        b.emit(Op::Add, 1);
        let mut vm = VM::new(b.build());
        match vm.run() {
            VMResult::Ok(Value::Int(42)) => {}
            other => panic!("expected Int(42), got {:?}", other),
        }
    }

    #[test]
    fn test_jump() {
        let mut b = ChunkBuilder::new();
        b.emit(Op::LoadInt(1), 1);
        b.emit(Op::Jump(3), 1);
        b.emit(Op::LoadInt(999), 1); // skipped
        // ip 3:
        b.emit(Op::LoadInt(2), 1);
        b.emit(Op::Add, 1);
        let mut vm = VM::new(b.build());
        match vm.run() {
            VMResult::Ok(Value::Int(3)) => {}
            other => panic!("expected Int(3), got {:?}", other),
        }
    }

    #[test]
    fn test_fused_sum_loop() {
        // sum = 0; for i in 0..100 { sum += i }
        let mut b = ChunkBuilder::new();
        b.emit(Op::PushFrame, 1);
        b.emit(Op::LoadInt(0), 1);
        b.emit(Op::SetSlot(0), 1); // sum = 0
        b.emit(Op::LoadInt(0), 1);
        b.emit(Op::SetSlot(1), 1); // i = 0
        b.emit(Op::AccumSumLoop(0, 1, 100), 1);
        b.emit(Op::GetSlot(0), 1);

        let mut vm = VM::new(b.build());
        match vm.run() {
            VMResult::Ok(Value::Int(4950)) => {}
            other => panic!("expected Int(4950), got {:?}", other),
        }
    }

    #[test]
    fn test_function_call() {
        let mut b = ChunkBuilder::new();
        let double_name = b.add_name("double");

        // main: push 21, call double, result on stack
        b.emit(Op::LoadInt(21), 1);
        b.emit(Op::Call(double_name, 1), 1);
        let end_jump = b.emit(Op::Jump(0), 1); // jump past function body

        // double: arg * 2
        let double_ip = b.current_pos();
        b.add_sub_entry(double_name, double_ip);
        b.emit(Op::LoadInt(2), 2);
        b.emit(Op::Mul, 2);
        b.emit(Op::ReturnValue, 2);

        b.patch_jump(end_jump, b.current_pos());

        let mut vm = VM::new(b.build());
        match vm.run() {
            VMResult::Ok(Value::Int(42)) => {}
            other => panic!("expected Int(42), got {:?}", other),
        }
    }
}
