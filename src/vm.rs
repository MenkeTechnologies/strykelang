use std::collections::VecDeque;
use std::io::{self, Write as IoWrite};
use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::RwLock;
use rayon::prelude::*;

use caseless::default_case_fold_str;

use crate::ast::{Block, Expr};
use crate::bytecode::{BuiltinId, Chunk, Op};
use crate::error::{ErrorKind, PerlError, PerlResult};
use crate::interpreter::{Flow, FlowOrError, Interpreter, WantarrayCtx};
use crate::sort_fast::{sort_magic_cmp, SortBlockFast};
use crate::value::{PerlAsyncTask, PerlHeap, PerlValue, PipelineInner};
use parking_lot::Mutex;

/// Saved state when entering a function call.
#[derive(Debug)]
struct CallFrame {
    return_ip: usize,
    stack_base: usize,
    scope_depth: usize,
    saved_wantarray: WantarrayCtx,
}

/// Stack-based bytecode virtual machine.
pub struct VM<'a> {
    names: Vec<String>,
    constants: Vec<PerlValue>,
    ops: Vec<Op>,
    lines: Vec<usize>,
    sub_entries: Vec<(u16, usize)>,
    blocks: Vec<Block>,
    lvalues: Vec<Expr>,
    ip: usize,
    stack: Vec<PerlValue>,
    call_stack: Vec<CallFrame>,
    interp: &'a mut Interpreter,
}

impl<'a> VM<'a> {
    pub fn new(chunk: &Chunk, interp: &'a mut Interpreter) -> Self {
        Self {
            names: chunk.names.clone(),
            constants: chunk.constants.clone(),
            ops: chunk.ops.clone(),
            lines: chunk.lines.clone(),
            sub_entries: chunk.sub_entries.clone(),
            blocks: chunk.blocks.clone(),
            lvalues: chunk.lvalues.clone(),
            ip: 0,
            stack: Vec::with_capacity(256),
            call_stack: Vec::with_capacity(32),
            interp,
        }
    }

    #[inline]
    fn push(&mut self, val: PerlValue) {
        self.stack.push(val);
    }

    #[inline]
    fn pop(&mut self) -> PerlValue {
        self.stack.pop().unwrap_or(PerlValue::Undef)
    }

    #[inline]
    fn peek(&self) -> &PerlValue {
        self.stack.last().unwrap_or(&PerlValue::Undef)
    }

    #[inline]
    fn name_owned(&self, idx: u16) -> String {
        self.names[idx as usize].clone()
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

    pub fn execute(&mut self) -> PerlResult<PerlValue> {
        let ops = &self.ops as *const Vec<Op>;
        // SAFETY: ops doesn't change during execution; pointer avoids borrow on self
        let ops = unsafe { &*ops };
        let len = ops.len();
        let mut last = PerlValue::Undef;
        // Safety limit: prevent infinite loops from consuming all memory.
        // 100M ops is generous — fib(25) is ~1.5M ops.
        let mut op_count: u64 = 0;
        const MAX_OPS: u64 = 100_000_000;

        loop {
            if self.ip >= len {
                break;
            }

            op_count += 1;
            if op_count > MAX_OPS {
                return Err(PerlError::runtime(
                    "VM execution limit exceeded (possible infinite loop)",
                    self.line(),
                ));
            }

            let op = &ops[self.ip];
            self.ip += 1;

            match op {
                // ── Constants ──
                Op::LoadInt(n) => self.push(PerlValue::Integer(*n)),
                Op::LoadFloat(f) => self.push(PerlValue::Float(*f)),
                Op::LoadConst(idx) => self.push(self.constant(*idx).clone()),
                Op::LoadUndef => self.push(PerlValue::Undef),

                // ── Stack ──
                Op::Pop => {
                    self.pop();
                }
                Op::Dup => {
                    let v = self.peek().clone();
                    self.push(v);
                }

                // ── Scalars ──
                Op::GetScalar(idx) => {
                    let n = self.name_owned(*idx);
                    let val = self.interp.scope.get_scalar(&n);
                    self.push(val);
                }
                Op::SetScalar(idx) => {
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.require_scalar_mutable(&n)?;
                    self.interp
                        .scope
                        .set_scalar(&n, val)
                        .map_err(|e| e.at_line(self.line()))?;
                }
                Op::SetScalarKeep(idx) => {
                    let val = self.peek().clone();
                    let n = self.name_owned(*idx);
                    self.require_scalar_mutable(&n)?;
                    self.interp
                        .scope
                        .set_scalar(&n, val)
                        .map_err(|e| e.at_line(self.line()))?;
                }
                Op::DeclareScalar(idx) => {
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.interp
                        .scope
                        .declare_scalar_frozen(&n, val, false, None)
                        .map_err(|e| e.at_line(self.line()))?;
                }
                Op::DeclareScalarFrozen(idx) => {
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.interp
                        .scope
                        .declare_scalar_frozen(&n, val, true, None)
                        .map_err(|e| e.at_line(self.line()))?;
                }

                // ── Arrays ──
                Op::GetArray(idx) => {
                    let n = self.name_owned(*idx);
                    let arr = self.interp.scope.get_array(&n);
                    self.push(PerlValue::Array(arr));
                }
                Op::SetArray(idx) => {
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.require_array_mutable(&n)?;
                    self.interp.scope.set_array(&n, val.to_list());
                }
                Op::DeclareArray(idx) => {
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.interp.scope.declare_array(&n, val.to_list());
                }
                Op::DeclareArrayFrozen(idx) => {
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.interp
                        .scope
                        .declare_array_frozen(&n, val.to_list(), true);
                }
                Op::GetArrayElem(idx) => {
                    let index = self.pop().to_int();
                    let n = self.name_owned(*idx);
                    let val = self.interp.scope.get_array_element(&n, index);
                    self.push(val);
                }
                Op::SetArrayElem(idx) => {
                    let index = self.pop().to_int();
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.require_array_mutable(&n)?;
                    self.interp.scope.set_array_element(&n, index, val);
                }
                Op::PushArray(idx) => {
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.require_array_mutable(&n)?;
                    let arr = self.interp.scope.get_array_mut(&n);
                    arr.push(val);
                }
                Op::PopArray(idx) => {
                    let n = self.name_owned(*idx);
                    self.require_array_mutable(&n)?;
                    let arr = self.interp.scope.get_array_mut(&n);
                    let val = arr.pop().unwrap_or(PerlValue::Undef);
                    self.push(val);
                }
                Op::ShiftArray(idx) => {
                    let n = self.name_owned(*idx);
                    self.require_array_mutable(&n)?;
                    let arr = self.interp.scope.get_array_mut(&n);
                    let val = if arr.is_empty() {
                        PerlValue::Undef
                    } else {
                        arr.remove(0)
                    };
                    self.push(val);
                }
                Op::ArrayLen(idx) => {
                    let n = self.name_owned(*idx);
                    let arr = self.interp.scope.get_array(&n);
                    self.push(PerlValue::Integer(arr.len() as i64));
                }

                // ── Hashes ──
                Op::GetHash(idx) => {
                    let n = self.name_owned(*idx);
                    let h = self.interp.scope.get_hash(&n);
                    self.push(PerlValue::Hash(h));
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
                    let n = self.name_owned(*idx);
                    self.require_hash_mutable(&n)?;
                    self.interp.scope.set_hash(&n, map);
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
                    let n = self.name_owned(*idx);
                    self.interp.scope.declare_hash(&n, map);
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
                    let n = self.name_owned(*idx);
                    self.interp.scope.declare_hash_frozen(&n, map, true);
                }
                Op::LocalDeclareScalar(idx) => {
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.interp
                        .scope
                        .local_set_scalar(&n, val)
                        .map_err(|e| e.at_line(self.line()))?;
                }
                Op::LocalDeclareArray(idx) => {
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.interp
                        .scope
                        .local_set_array(&n, val.to_list())
                        .map_err(|e| e.at_line(self.line()))?;
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
                    let n = self.name_owned(*idx);
                    self.interp
                        .scope
                        .local_set_hash(&n, map)
                        .map_err(|e| e.at_line(self.line()))?;
                }
                Op::GetHashElem(idx) => {
                    let key = self.pop().to_string();
                    let n = self.name_owned(*idx);
                    let val = self.interp.scope.get_hash_element(&n, &key);
                    self.push(val);
                }
                Op::SetHashElem(idx) => {
                    let key = self.pop().to_string();
                    let val = self.pop();
                    let n = self.name_owned(*idx);
                    self.require_hash_mutable(&n)?;
                    self.interp.scope.set_hash_element(&n, &key, val);
                }
                Op::DeleteHashElem(idx) => {
                    let key = self.pop().to_string();
                    let n = self.name_owned(*idx);
                    self.require_hash_mutable(&n)?;
                    let val = self.interp.scope.delete_hash_element(&n, &key);
                    self.push(val);
                }
                Op::ExistsHashElem(idx) => {
                    let key = self.pop().to_string();
                    let n = self.name_owned(*idx);
                    let exists = self.interp.scope.exists_hash_element(&n, &key);
                    self.push(PerlValue::Integer(if exists { 1 } else { 0 }));
                }
                Op::HashKeys(idx) => {
                    let n = self.name_owned(*idx);
                    let h = self.interp.scope.get_hash(&n);
                    let keys: Vec<PerlValue> =
                        h.keys().map(|k| PerlValue::String(k.clone())).collect();
                    self.push(PerlValue::Array(keys));
                }
                Op::HashValues(idx) => {
                    let n = self.name_owned(*idx);
                    let h = self.interp.scope.get_hash(&n);
                    let vals: Vec<PerlValue> = h.values().cloned().collect();
                    self.push(PerlValue::Array(vals));
                }

                // ── Arithmetic (integer fast paths) ──
                Op::Add => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(match (&a, &b) {
                        (PerlValue::Integer(x), PerlValue::Integer(y)) => {
                            PerlValue::Integer(x.wrapping_add(*y))
                        }
                        _ => PerlValue::Float(a.to_number() + b.to_number()),
                    });
                }
                Op::Sub => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(match (&a, &b) {
                        (PerlValue::Integer(x), PerlValue::Integer(y)) => {
                            PerlValue::Integer(x.wrapping_sub(*y))
                        }
                        _ => PerlValue::Float(a.to_number() - b.to_number()),
                    });
                }
                Op::Mul => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(match (&a, &b) {
                        (PerlValue::Integer(x), PerlValue::Integer(y)) => {
                            PerlValue::Integer(x.wrapping_mul(*y))
                        }
                        _ => PerlValue::Float(a.to_number() * b.to_number()),
                    });
                }
                Op::Div => {
                    let b = self.pop();
                    let a = self.pop();
                    match (&a, &b) {
                        (PerlValue::Integer(x), PerlValue::Integer(y)) => {
                            if *y == 0 {
                                return Err(PerlError::runtime(
                                    "Illegal division by zero",
                                    self.line(),
                                ));
                            }
                            self.push(if x % y == 0 {
                                PerlValue::Integer(x / y)
                            } else {
                                PerlValue::Float(*x as f64 / *y as f64)
                            });
                        }
                        _ => {
                            let d = b.to_number();
                            if d == 0.0 {
                                return Err(PerlError::runtime(
                                    "Illegal division by zero",
                                    self.line(),
                                ));
                            }
                            self.push(PerlValue::Float(a.to_number() / d));
                        }
                    }
                }
                Op::Mod => {
                    let b = self.pop().to_int();
                    let a = self.pop().to_int();
                    if b == 0 {
                        return Err(PerlError::runtime("Illegal modulus zero", self.line()));
                    }
                    self.push(PerlValue::Integer(a % b));
                }
                Op::Pow => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(match (&a, &b) {
                        (PerlValue::Integer(x), PerlValue::Integer(y)) if *y >= 0 && *y <= 63 => {
                            PerlValue::Integer(x.wrapping_pow(*y as u32))
                        }
                        _ => PerlValue::Float(a.to_number().powf(b.to_number())),
                    });
                }
                Op::Negate => {
                    let a = self.pop();
                    self.push(match a {
                        PerlValue::Integer(n) => PerlValue::Integer(-n),
                        _ => PerlValue::Float(-a.to_number()),
                    });
                }

                // ── String ──
                Op::Concat => {
                    let b = self.pop();
                    let a = self.pop();
                    let mut s = a.to_string();
                    b.append_to(&mut s);
                    self.push(PerlValue::String(s));
                }
                Op::StringRepeat => {
                    let n = self.pop().to_int().max(0) as usize;
                    let val = self.pop();
                    self.push(PerlValue::String(val.to_string().repeat(n)));
                }

                // ── Numeric comparison ──
                Op::NumEq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(int_cmp(&a, &b, |x, y| x == y, |x, y| x == y));
                }
                Op::NumNe => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(int_cmp(&a, &b, |x, y| x != y, |x, y| x != y));
                }
                Op::NumLt => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(int_cmp(&a, &b, |x, y| x < y, |x, y| x < y));
                }
                Op::NumGt => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(int_cmp(&a, &b, |x, y| x > y, |x, y| x > y));
                }
                Op::NumLe => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(int_cmp(&a, &b, |x, y| x <= y, |x, y| x <= y));
                }
                Op::NumGe => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(int_cmp(&a, &b, |x, y| x >= y, |x, y| x >= y));
                }
                Op::Spaceship => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(match (&a, &b) {
                        (PerlValue::Integer(x), PerlValue::Integer(y)) => {
                            PerlValue::Integer(if x < y {
                                -1
                            } else if x > y {
                                1
                            } else {
                                0
                            })
                        }
                        _ => {
                            let x = a.to_number();
                            let y = b.to_number();
                            PerlValue::Integer(if x < y {
                                -1.0 as i64
                            } else if x > y {
                                1
                            } else {
                                0
                            })
                        }
                    });
                }

                // ── String comparison ──
                Op::StrEq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(PerlValue::Integer(if a.to_string() == b.to_string() {
                        1
                    } else {
                        0
                    }));
                }
                Op::StrNe => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(PerlValue::Integer(if a.to_string() != b.to_string() {
                        1
                    } else {
                        0
                    }));
                }
                Op::StrLt => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(PerlValue::Integer(if a.to_string() < b.to_string() {
                        1
                    } else {
                        0
                    }));
                }
                Op::StrGt => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(PerlValue::Integer(if a.to_string() > b.to_string() {
                        1
                    } else {
                        0
                    }));
                }
                Op::StrLe => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(PerlValue::Integer(if a.to_string() <= b.to_string() {
                        1
                    } else {
                        0
                    }));
                }
                Op::StrGe => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(PerlValue::Integer(if a.to_string() >= b.to_string() {
                        1
                    } else {
                        0
                    }));
                }
                Op::StrCmp => {
                    let b = self.pop();
                    let a = self.pop();
                    let cmp = a.to_string().cmp(&b.to_string());
                    self.push(PerlValue::Integer(match cmp {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Greater => 1,
                        std::cmp::Ordering::Equal => 0,
                    }));
                }

                // ── Logical / Bitwise ──
                Op::LogNot => {
                    let a = self.pop();
                    self.push(PerlValue::Integer(if a.is_true() { 0 } else { 1 }));
                }
                Op::BitAnd => {
                    let rv = self.pop();
                    let lv = self.pop();
                    if let Some(s) = crate::value::set_intersection(&lv, &rv) {
                        self.push(s);
                    } else {
                        self.push(PerlValue::Integer(lv.to_int() & rv.to_int()));
                    }
                }
                Op::BitOr => {
                    let rv = self.pop();
                    let lv = self.pop();
                    if let Some(s) = crate::value::set_union(&lv, &rv) {
                        self.push(s);
                    } else {
                        self.push(PerlValue::Integer(lv.to_int() | rv.to_int()));
                    }
                }
                Op::BitXor => {
                    let b = self.pop().to_int();
                    let a = self.pop().to_int();
                    self.push(PerlValue::Integer(a ^ b));
                }
                Op::BitNot => {
                    let a = self.pop().to_int();
                    self.push(PerlValue::Integer(!a));
                }
                Op::Shl => {
                    let b = self.pop().to_int();
                    let a = self.pop().to_int();
                    self.push(PerlValue::Integer(a << b));
                }
                Op::Shr => {
                    let b = self.pop().to_int();
                    let a = self.pop().to_int();
                    self.push(PerlValue::Integer(a >> b));
                }

                // ── Control flow ──
                Op::Jump(target) => {
                    self.ip = *target;
                }
                Op::JumpIfTrue(target) => {
                    let val = self.pop();
                    if val.is_true() {
                        self.ip = *target;
                    }
                }
                Op::JumpIfFalse(target) => {
                    let val = self.pop();
                    if !val.is_true() {
                        self.ip = *target;
                    }
                }
                Op::JumpIfFalseKeep(target) => {
                    if !self.peek().is_true() {
                        self.ip = *target;
                    } else {
                        self.pop();
                    }
                }
                Op::JumpIfTrueKeep(target) => {
                    if self.peek().is_true() {
                        self.ip = *target;
                    } else {
                        self.pop();
                    }
                }
                Op::JumpIfDefinedKeep(target) => {
                    if !matches!(self.peek(), PerlValue::Undef) {
                        self.ip = *target;
                    } else {
                        self.pop();
                    }
                }

                // ── Increment / Decrement ──
                Op::PreInc(idx) => {
                    let n = self.name_owned(*idx);
                    self.require_scalar_mutable(&n)?;
                    let val = self.interp.scope.get_scalar(&n).to_int() + 1;
                    let new_val = PerlValue::Integer(val);
                    self.interp
                        .scope
                        .set_scalar(&n, new_val.clone())
                        .map_err(|e| e.at_line(self.line()))?;
                    self.push(new_val);
                }
                Op::PreDec(idx) => {
                    let n = self.name_owned(*idx);
                    self.require_scalar_mutable(&n)?;
                    let val = self.interp.scope.get_scalar(&n).to_int() - 1;
                    let new_val = PerlValue::Integer(val);
                    self.interp
                        .scope
                        .set_scalar(&n, new_val.clone())
                        .map_err(|e| e.at_line(self.line()))?;
                    self.push(new_val);
                }
                Op::PostInc(idx) => {
                    let n = self.name_owned(*idx);
                    self.require_scalar_mutable(&n)?;
                    let old = self.interp.scope.get_scalar(&n);
                    let new_val = PerlValue::Integer(old.to_int() + 1);
                    self.interp
                        .scope
                        .set_scalar(&n, new_val)
                        .map_err(|e| e.at_line(self.line()))?;
                    self.push(old);
                }
                Op::PostDec(idx) => {
                    let n = self.name_owned(*idx);
                    self.require_scalar_mutable(&n)?;
                    let old = self.interp.scope.get_scalar(&n);
                    let new_val = PerlValue::Integer(old.to_int() - 1);
                    self.interp
                        .scope
                        .set_scalar(&n, new_val)
                        .map_err(|e| e.at_line(self.line()))?;
                    self.push(old);
                }

                // ── Functions ──
                Op::Call(name_idx, argc, wa) => {
                    let name = self.name_owned(*name_idx);
                    let argc = *argc as usize;
                    let want = WantarrayCtx::from_byte(*wa);

                    // Collect args from stack
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        let v = self.pop();
                        match v {
                            PerlValue::Array(items) => args.extend(items),
                            other => args.push(other),
                        }
                    }
                    args.reverse(); // stack order is reversed

                    // Check if sub is compiled (has bytecode entry)
                    if let Some(entry_ip) = self.find_sub_entry(*name_idx) {
                        let saved_wa = self.interp.wantarray_kind;
                        // Save call frame
                        self.call_stack.push(CallFrame {
                            return_ip: self.ip,
                            stack_base: self.stack.len(),
                            scope_depth: self.interp.scope.depth(),
                            saved_wantarray: saved_wa,
                        });
                        self.interp.wantarray_kind = want;
                        // Push scope frame and set @_
                        self.interp.scope.push_frame();
                        self.interp.scope.declare_array("_", args);
                        // Jump to sub entry
                        self.ip = entry_ip;
                    } else if let Some(r) =
                        crate::builtins::try_builtin(self.interp, &name, &args, self.line())
                    {
                        self.push(r?);
                    } else if let Some(sub) = self.interp.subs.get(&name).cloned() {
                        // Fall back to tree-walker for non-compiled subs
                        let saved_wa = self.interp.wantarray_kind;
                        self.interp.wantarray_kind = want;
                        self.interp.scope.push_frame();
                        self.interp.scope.declare_array("_", args);
                        if let Some(ref env) = sub.closure_env {
                            self.interp.scope.restore_capture(env);
                        }
                        let result = self.interp.exec_block_no_scope(&sub.body);
                        self.interp.wantarray_kind = saved_wa;
                        self.interp.scope.pop_frame();
                        match result {
                            Ok(v) => self.push(v),
                            Err(crate::interpreter::FlowOrError::Flow(
                                crate::interpreter::Flow::Return(v),
                            )) => self.push(v),
                            Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                            Err(_) => self.push(PerlValue::Undef),
                        }
                    } else if let Some(result) =
                        self.interp
                            .try_autoload_call(&name, args, self.line(), want)
                    {
                        match result {
                            Ok(v) => self.push(v),
                            Err(crate::interpreter::FlowOrError::Flow(
                                crate::interpreter::Flow::Return(v),
                            )) => self.push(v),
                            Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                            Err(_) => self.push(PerlValue::Undef),
                        }
                    } else {
                        return Err(PerlError::runtime(
                            format!("Undefined subroutine &{}", name),
                            self.line(),
                        ));
                    }
                }
                Op::Return => {
                    if let Some(frame) = self.call_stack.pop() {
                        self.interp.wantarray_kind = frame.saved_wantarray;
                        self.stack.truncate(frame.stack_base);
                        self.interp.scope.pop_to_depth(frame.scope_depth);
                        self.push(PerlValue::Undef);
                        self.ip = frame.return_ip;
                    } else {
                        break;
                    }
                }
                Op::ReturnValue => {
                    let val = self.pop();
                    if let Some(frame) = self.call_stack.pop() {
                        self.interp.wantarray_kind = frame.saved_wantarray;
                        self.stack.truncate(frame.stack_base);
                        self.interp.scope.pop_to_depth(frame.scope_depth);
                        self.push(val);
                        self.ip = frame.return_ip;
                    } else {
                        last = val;
                        break;
                    }
                }

                // ── Scope ──
                Op::PushFrame => self.interp.scope.push_frame(),
                Op::PopFrame => self.interp.scope.pop_frame(),

                // ── I/O ──
                Op::Print(argc) => {
                    let argc = *argc as usize;
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse();
                    let mut output = String::new();
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 && !self.interp.ofs.is_empty() {
                            output.push_str(&self.interp.ofs);
                        }
                        output.push_str(&arg.to_string());
                    }
                    output.push_str(&self.interp.ors);
                    print!("{}", output);
                    let _ = io::stdout().flush();
                    self.push(PerlValue::Integer(1));
                }
                Op::Say(argc) => {
                    let argc = *argc as usize;
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse();
                    let mut output = String::new();
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 && !self.interp.ofs.is_empty() {
                            output.push_str(&self.interp.ofs);
                        }
                        output.push_str(&arg.to_string());
                    }
                    output.push('\n');
                    print!("{}", output);
                    let _ = io::stdout().flush();
                    self.push(PerlValue::Integer(1));
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
                }

                // ── List / Range ──
                Op::MakeArray(n) => {
                    let n = *n as usize;
                    let mut arr = Vec::with_capacity(n);
                    for _ in 0..n {
                        let v = self.pop();
                        match v {
                            PerlValue::Array(items) => arr.extend(items),
                            other => arr.push(other),
                        }
                    }
                    arr.reverse();
                    self.push(PerlValue::Array(arr));
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
                    self.push(PerlValue::Hash(map));
                }
                Op::Range => {
                    let to = self.pop().to_int();
                    let from = self.pop().to_int();
                    let arr: Vec<PerlValue> = (from..=to).map(PerlValue::Integer).collect();
                    self.push(PerlValue::Array(arr));
                }

                // ── Regex ──
                Op::RegexMatch(pat_idx, flags_idx, scalar_g, pos_key_idx) => {
                    let string = self.pop().to_string();
                    let pattern = self.constant(*pat_idx).to_string();
                    let flags = self.constant(*flags_idx).to_string();
                    let pos_key = if *pos_key_idx == u16::MAX {
                        "_".to_string()
                    } else {
                        self.constant(*pos_key_idx).to_string()
                    };
                    let line = self.line();
                    match self
                        .interp
                        .regex_match_execute(string, &pattern, &flags, *scalar_g, &pos_key, line)
                    {
                        Ok(v) => self.push(v),
                        Err(FlowOrError::Error(e)) => return Err(e),
                        Err(FlowOrError::Flow(_)) => {
                            return Err(PerlError::runtime("unexpected flow in regex match", line));
                        }
                    }
                }
                Op::RegexSubst(pat_idx, repl_idx, flags_idx, lvalue_idx) => {
                    let string = self.pop().to_string();
                    let pattern = self.constant(*pat_idx).to_string();
                    let replacement = self.constant(*repl_idx).to_string();
                    let flags = self.constant(*flags_idx).to_string();
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
                        Ok(v) => self.push(v),
                        Err(FlowOrError::Error(e)) => return Err(e),
                        Err(FlowOrError::Flow(_)) => {
                            return Err(PerlError::runtime("unexpected flow in s///", line));
                        }
                    }
                }
                Op::RegexTransliterate(from_idx, to_idx, flags_idx, lvalue_idx) => {
                    let string = self.pop().to_string();
                    let from = self.constant(*from_idx).to_string();
                    let to = self.constant(*to_idx).to_string();
                    let flags = self.constant(*flags_idx).to_string();
                    let target = &self.lvalues[*lvalue_idx as usize];
                    let line = self.line();
                    match self
                        .interp
                        .regex_transliterate_execute(string, &from, &to, &flags, target, line)
                    {
                        Ok(v) => self.push(v),
                        Err(FlowOrError::Error(e)) => return Err(e),
                        Err(FlowOrError::Flow(_)) => {
                            return Err(PerlError::runtime("unexpected flow in tr///", line));
                        }
                    }
                }
                Op::RegexMatchDyn(negate) => {
                    let pattern = self.pop().to_string();
                    let s = self.pop().to_string();
                    let line = self.line();
                    match self
                        .interp
                        .regex_match_execute(s, &pattern, "", false, "_", line)
                    {
                        Ok(v) => {
                            let matched = v.is_true();
                            let out = if *negate { !matched } else { matched };
                            self.push(PerlValue::Integer(if out { 1 } else { 0 }));
                        }
                        Err(FlowOrError::Error(e)) => return Err(e),
                        Err(FlowOrError::Flow(_)) => {
                            return Err(PerlError::runtime("unexpected flow in =~", line));
                        }
                    }
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
                }

                // ── References ──
                Op::MakeScalarRef => {
                    let val = self.pop();
                    self.push(PerlValue::ScalarRef(Arc::new(RwLock::new(val))));
                }
                Op::MakeArrayRef => {
                    let val = self.pop();
                    let arr = match val {
                        PerlValue::Array(a) => a,
                        other => vec![other],
                    };
                    self.push(PerlValue::ArrayRef(Arc::new(RwLock::new(arr))));
                }
                Op::MakeHashRef => {
                    let val = self.pop();
                    let map = match val {
                        PerlValue::Hash(h) => h,
                        _ => {
                            let items = val.to_list();
                            let mut m = IndexMap::new();
                            let mut i = 0;
                            while i + 1 < items.len() {
                                m.insert(items[i].to_string(), items[i + 1].clone());
                                i += 2;
                            }
                            m
                        }
                    };
                    self.push(PerlValue::HashRef(Arc::new(RwLock::new(map))));
                }
                Op::MakeCodeRef(block_idx) => {
                    let block = self.blocks[*block_idx as usize].clone();
                    let captured = self.interp.scope.capture();
                    self.push(PerlValue::CodeRef(Arc::new(crate::value::PerlSub {
                        name: "__ANON__".to_string(),
                        params: vec![],
                        body: block,
                        closure_env: Some(captured),
                        prototype: None,
                    })));
                }

                // ── Arrow dereference ──
                Op::ArrowArray => {
                    let idx = self.pop().to_int();
                    let r = self.pop();
                    match r {
                        PerlValue::ArrayRef(a) => {
                            let arr = a.read();
                            let i = if idx < 0 {
                                (arr.len() as i64 + idx) as usize
                            } else {
                                idx as usize
                            };
                            self.push(arr.get(i).cloned().unwrap_or(PerlValue::Undef));
                        }
                        _ => self.push(PerlValue::Undef),
                    }
                }
                Op::ArrowHash => {
                    let key = self.pop().to_string();
                    let r = self.pop();
                    match r {
                        PerlValue::HashRef(h) => {
                            self.push(h.read().get(&key).cloned().unwrap_or(PerlValue::Undef));
                        }
                        PerlValue::Blessed(b) => {
                            let data = b.data.read();
                            if let PerlValue::Hash(ref h) = *data {
                                self.push(h.get(&key).cloned().unwrap_or(PerlValue::Undef));
                            } else {
                                self.push(PerlValue::Undef);
                            }
                        }
                        _ => self.push(PerlValue::Undef),
                    }
                }
                Op::ArrowCall(wa) => {
                    let want = WantarrayCtx::from_byte(*wa);
                    let args_val = self.pop();
                    let r = self.pop();
                    let args = args_val.to_list();
                    match r {
                        PerlValue::CodeRef(sub) => {
                            let saved_wa = self.interp.wantarray_kind;
                            self.interp.wantarray_kind = want;
                            self.interp.scope.push_frame();
                            self.interp.scope.declare_array("_", args);
                            if let Some(ref env) = sub.closure_env {
                                self.interp.scope.restore_capture(env);
                            }
                            let result = self.interp.exec_block_no_scope(&sub.body);
                            self.interp.wantarray_kind = saved_wa;
                            self.interp.scope.pop_frame();
                            match result {
                                Ok(v) => self.push(v),
                                Err(crate::interpreter::FlowOrError::Flow(
                                    crate::interpreter::Flow::Return(v),
                                )) => self.push(v),
                                Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                                Err(_) => self.push(PerlValue::Undef),
                            }
                        }
                        _ => return Err(PerlError::runtime("Not a code reference", self.line())),
                    }
                }

                // ── Method call ──
                Op::MethodCall(name_idx, argc, wa) => {
                    let method = self.name_owned(*name_idx);
                    let argc = *argc as usize;
                    let want = WantarrayCtx::from_byte(*wa);
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse();
                    let obj = self.pop();
                    if let Some(r) =
                        crate::pchannel::dispatch_method(&obj, &method, &args, self.line())
                    {
                        self.push(r?);
                        continue;
                    }
                    if let Some(r) =
                        self.interp
                            .try_native_method(&obj, &method, &args, self.line())
                    {
                        self.push(r?);
                        continue;
                    }
                    let class = match &obj {
                        PerlValue::Blessed(b) => b.class.clone(),
                        PerlValue::String(s) => s.clone(),
                        _ => {
                            return Err(PerlError::runtime(
                                "Can't call method on non-object",
                                self.line(),
                            ))
                        }
                    };
                    let mut all_args = vec![obj];
                    all_args.extend(args);
                    let full_name = format!("{}::{}", class, method);
                    if let Some(sub) = self.interp.subs.get(&full_name).cloned() {
                        let saved_wa = self.interp.wantarray_kind;
                        self.interp.wantarray_kind = want;
                        self.interp.scope.push_frame();
                        self.interp.scope.declare_array("_", all_args);
                        if let Some(ref env) = sub.closure_env {
                            self.interp.scope.restore_capture(env);
                        }
                        let result = self.interp.exec_block_no_scope(&sub.body);
                        self.interp.wantarray_kind = saved_wa;
                        self.interp.scope.pop_frame();
                        match result {
                            Ok(v) => self.push(v),
                            Err(crate::interpreter::FlowOrError::Flow(
                                crate::interpreter::Flow::Return(v),
                            )) => self.push(v),
                            Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                            Err(_) => self.push(PerlValue::Undef),
                        }
                    } else if method == "new" {
                        if class == "Set" {
                            self.push(crate::value::set_from_elements(
                                all_args.into_iter().skip(1),
                            ));
                        } else {
                            let mut map = IndexMap::new();
                            let mut i = 1;
                            while i + 1 < all_args.len() {
                                map.insert(all_args[i].to_string(), all_args[i + 1].clone());
                                i += 2;
                            }
                            self.push(PerlValue::Blessed(Arc::new(crate::value::BlessedRef {
                                class,
                                data: RwLock::new(PerlValue::Hash(map)),
                            })));
                        }
                    } else if let Some(result) =
                        self.interp
                            .try_autoload_call(&full_name, all_args, self.line(), want)
                    {
                        match result {
                            Ok(v) => self.push(v),
                            Err(crate::interpreter::FlowOrError::Flow(
                                crate::interpreter::Flow::Return(v),
                            )) => self.push(v),
                            Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                            Err(_) => self.push(PerlValue::Undef),
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
                    self.push(PerlValue::Integer(if result { 1 } else { 0 }));
                }

                // ── Map/Grep/Sort with blocks (delegate to tree-walker) ──
                Op::MapWithBlock(block_idx) => {
                    let list = self.pop().to_list();
                    let block = self.blocks[*block_idx as usize].clone();
                    let mut result = Vec::new();
                    for item in list {
                        let _ = self.interp.scope.set_scalar("_", item);
                        match self.interp.exec_block_no_scope(&block) {
                            Ok(val) => match val {
                                PerlValue::Array(a) => result.extend(a),
                                other => result.push(other),
                            },
                            Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                            Err(_) => {}
                        }
                    }
                    self.push(PerlValue::Array(result));
                }
                Op::GrepWithBlock(block_idx) => {
                    let list = self.pop().to_list();
                    let block = self.blocks[*block_idx as usize].clone();
                    let mut result = Vec::new();
                    for item in list {
                        let _ = self.interp.scope.set_scalar("_", item.clone());
                        match self.interp.exec_block_no_scope(&block) {
                            Ok(val) => {
                                if val.is_true() {
                                    result.push(item);
                                }
                            }
                            Err(crate::interpreter::FlowOrError::Error(e)) => return Err(e),
                            Err(_) => {}
                        }
                    }
                    self.push(PerlValue::Array(result));
                }
                Op::SortWithBlock(block_idx) => {
                    let mut items = self.pop().to_list();
                    let block = self.blocks[*block_idx as usize].clone();
                    items.sort_by(|a, b| {
                        let _ = self.interp.scope.set_scalar("a", a.clone());
                        let _ = self.interp.scope.set_scalar("b", b.clone());
                        match self.interp.exec_block_no_scope(&block) {
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
                    self.push(PerlValue::Array(items));
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
                    self.push(PerlValue::Array(items));
                }
                Op::SortNoBlock => {
                    let mut items = self.pop().to_list();
                    items.sort_by_key(|a| a.to_string());
                    self.push(PerlValue::Array(items));
                }
                Op::ReverseOp => {
                    let val = self.pop();
                    match val {
                        PerlValue::Array(mut a) => {
                            a.reverse();
                            self.push(PerlValue::Array(a));
                        }
                        PerlValue::String(s) => {
                            self.push(PerlValue::String(s.chars().rev().collect()))
                        }
                        other => {
                            self.push(PerlValue::String(other.to_string().chars().rev().collect()))
                        }
                    }
                }

                // ── Eval block ──
                Op::EvalBlock(block_idx) => {
                    let block = self.blocks[*block_idx as usize].clone();
                    // Use exec_block (with scope frame) so local/my declarations
                    // inside the block are properly scoped.
                    match self.interp.exec_block(&block) {
                        Ok(v) => {
                            self.interp.eval_error = String::new();
                            self.push(v);
                        }
                        Err(crate::interpreter::FlowOrError::Error(e)) => {
                            self.interp.eval_error = e.to_string();
                            self.push(PerlValue::Undef);
                        }
                        Err(_) => self.push(PerlValue::Undef),
                    }
                }

                // ── Parallel operations (rayon) ──
                Op::PMapWithBlock(block_idx) => {
                    let list = self.pop().to_list();
                    let block = self.blocks[*block_idx as usize].clone();
                    let subs = self.interp.subs.clone();
                    let scope_capture = self.interp.scope.capture();
                    let results: Vec<PerlValue> = list
                        .into_par_iter()
                        .map(|item| {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            let _ = local_interp.scope.set_scalar("_", item);
                            match local_interp.exec_block_no_scope(&block) {
                                Ok(val) => val,
                                Err(_) => PerlValue::Undef,
                            }
                        })
                        .collect();
                    self.push(PerlValue::Array(results));
                }
                Op::PGrepWithBlock(block_idx) => {
                    let list = self.pop().to_list();
                    let block = self.blocks[*block_idx as usize].clone();
                    let subs = self.interp.subs.clone();
                    let scope_capture = self.interp.scope.capture();
                    let results: Vec<PerlValue> = list
                        .into_par_iter()
                        .filter(|item| {
                            let mut local_interp = Interpreter::new();
                            local_interp.subs = subs.clone();
                            local_interp.scope.restore_capture(&scope_capture);
                            let _ = local_interp.scope.set_scalar("_", item.clone());
                            match local_interp.exec_block_no_scope(&block) {
                                Ok(val) => val.is_true(),
                                Err(_) => false,
                            }
                        })
                        .collect();
                    self.push(PerlValue::Array(results));
                }
                Op::PForWithBlock(block_idx) => {
                    let list = self.pop().to_list();
                    let block = self.blocks[*block_idx as usize].clone();
                    let subs = self.interp.subs.clone();
                    let scope_capture = self.interp.scope.capture();
                    list.into_par_iter().for_each(|item| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        let _ = local_interp.scope.set_scalar("_", item);
                        let _ = local_interp.exec_block_no_scope(&block);
                    });
                    self.push(PerlValue::Undef);
                }
                Op::PSortWithBlock(block_idx) => {
                    let mut items = self.pop().to_list();
                    let block = self.blocks[*block_idx as usize].clone();
                    let subs = self.interp.subs.clone();
                    let scope_capture = self.interp.scope.capture();
                    items.par_sort_by(|a, b| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        let _ = local_interp.scope.set_scalar("a", a.clone());
                        let _ = local_interp.scope.set_scalar("b", b.clone());
                        match local_interp.exec_block_no_scope(&block) {
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
                    self.push(PerlValue::Array(items));
                }
                Op::PSortWithBlockFast(tag) => {
                    let mut items = self.pop().to_list();
                    let mode = match *tag {
                        0 => SortBlockFast::Numeric,
                        1 => SortBlockFast::String,
                        2 => SortBlockFast::NumericRev,
                        3 => SortBlockFast::StringRev,
                        _ => SortBlockFast::Numeric,
                    };
                    items.par_sort_by(|a, b| sort_magic_cmp(a, b, mode));
                    self.push(PerlValue::Array(items));
                }
                Op::FanWithBlock(block_idx) => {
                    let n = self.pop().to_int().max(0) as usize;
                    let block = self.blocks[*block_idx as usize].clone();
                    let subs = self.interp.subs.clone();
                    let scope_capture = self.interp.scope.capture();
                    (0..n).into_par_iter().for_each(|i| {
                        let mut local_interp = Interpreter::new();
                        local_interp.subs = subs.clone();
                        local_interp.scope.restore_capture(&scope_capture);
                        let _ = local_interp
                            .scope
                            .set_scalar("_", PerlValue::Integer(i as i64));
                        let _ = local_interp.exec_block_no_scope(&block);
                    });
                    self.push(PerlValue::Undef);
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
                        let out = match local_interp.exec_block_no_scope(&block) {
                            Ok(v) => Ok(v),
                            Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
                            Err(FlowOrError::Error(e)) => Err(e),
                            Err(_) => Ok(PerlValue::Undef),
                        };
                        *rs.lock() = Some(out);
                    });
                    *join_slot.lock() = Some(h);
                    self.push(PerlValue::AsyncTask(Arc::new(PerlAsyncTask {
                        result: result_slot,
                        join: join_slot,
                    })));
                }
                Op::Await => {
                    let v = self.pop();
                    match v {
                        PerlValue::AsyncTask(t) => {
                            let r = t.await_result();
                            self.push(r?);
                        }
                        other => self.push(other),
                    }
                }

                // ── Halt ──
                Op::Halt => break,
            }
        }

        if !self.stack.is_empty() {
            last = self.stack.last().cloned().unwrap_or(PerlValue::Undef);
        }

        Ok(last)
    }

    fn find_sub_entry(&self, name_idx: u16) -> Option<usize> {
        for (n, ip) in &self.sub_entries {
            if *n == name_idx {
                return Some(*ip);
            }
        }
        None
    }

    fn exec_builtin(&mut self, id: u16, args: Vec<PerlValue>) -> PerlResult<PerlValue> {
        let line = self.line();
        let bid = BuiltinId::from_u16(id);
        match bid {
            Some(BuiltinId::Length) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(match val {
                    PerlValue::Array(a) => PerlValue::Integer(a.len() as i64),
                    PerlValue::Hash(h) => PerlValue::Integer(h.len() as i64),
                    PerlValue::Bytes(b) => PerlValue::Integer(b.len() as i64),
                    other => PerlValue::Integer(other.to_string().len() as i64),
                })
            }
            Some(BuiltinId::Defined) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Integer(if matches!(val, PerlValue::Undef) {
                    0
                } else {
                    1
                }))
            }
            Some(BuiltinId::Abs) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Float(val.to_number().abs()))
            }
            Some(BuiltinId::Int) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Integer(val.to_number() as i64))
            }
            Some(BuiltinId::Sqrt) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Float(val.to_number().sqrt()))
            }
            Some(BuiltinId::Sin) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Float(val.to_number().sin()))
            }
            Some(BuiltinId::Cos) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Float(val.to_number().cos()))
            }
            Some(BuiltinId::Atan2) => {
                let mut it = args.into_iter();
                let y = it.next().unwrap_or(PerlValue::Undef);
                let x = it.next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Float(y.to_number().atan2(x.to_number())))
            }
            Some(BuiltinId::Exp) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Float(val.to_number().exp()))
            }
            Some(BuiltinId::Log) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Float(val.to_number().ln()))
            }
            Some(BuiltinId::Rand) => {
                let upper = match args.len() {
                    0 => 1.0,
                    _ => args[0].to_number(),
                };
                Ok(PerlValue::Float(self.interp.perl_rand(upper)))
            }
            Some(BuiltinId::Srand) => {
                let seed = match args.len() {
                    0 => None,
                    _ => Some(args[0].to_number()),
                };
                Ok(PerlValue::Integer(self.interp.perl_srand(seed)))
            }
            Some(BuiltinId::Crypt) => {
                let mut it = args.into_iter();
                let p = it.next().unwrap_or(PerlValue::Undef).to_string();
                let salt = it.next().unwrap_or(PerlValue::Undef).to_string();
                Ok(PerlValue::String(crate::crypt_util::perl_crypt(&p, &salt)))
            }
            Some(BuiltinId::Fc) => {
                let s = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::String(default_case_fold_str(&s.to_string())))
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
                    .map(|n| PerlValue::Integer(n as i64))
                    .unwrap_or(PerlValue::Undef))
            }
            Some(BuiltinId::Study) => {
                let s = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(PerlValue::Integer(s.to_string().len() as i64))
            }
            Some(BuiltinId::Chr) => {
                let n = args.into_iter().next().unwrap_or(PerlValue::Undef).to_int() as u32;
                Ok(PerlValue::String(
                    char::from_u32(n).map(|c| c.to_string()).unwrap_or_default(),
                ))
            }
            Some(BuiltinId::Ord) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                Ok(PerlValue::Integer(
                    s.chars().next().map(|c| c as i64).unwrap_or(0),
                ))
            }
            Some(BuiltinId::Hex) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
                Ok(PerlValue::Integer(
                    i64::from_str_radix(clean, 16).unwrap_or(0),
                ))
            }
            Some(BuiltinId::Oct) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                let s = s.trim();
                let n = if s.starts_with("0x") || s.starts_with("0X") {
                    i64::from_str_radix(&s[2..], 16).unwrap_or(0)
                } else if s.starts_with("0b") || s.starts_with("0B") {
                    i64::from_str_radix(&s[2..], 2).unwrap_or(0)
                } else {
                    i64::from_str_radix(s.trim_start_matches('0'), 8).unwrap_or(0)
                };
                Ok(PerlValue::Integer(n))
            }
            Some(BuiltinId::Uc) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                Ok(PerlValue::String(s.to_uppercase()))
            }
            Some(BuiltinId::Lc) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                Ok(PerlValue::String(s.to_lowercase()))
            }
            Some(BuiltinId::Ref) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(val.ref_type())
            }
            Some(BuiltinId::Scalar) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(val.scalar_context())
            }
            Some(BuiltinId::Join) => {
                let mut iter = args.into_iter();
                let sep = iter.next().unwrap_or(PerlValue::Undef).to_string();
                let list = iter.next().unwrap_or(PerlValue::Undef).to_list();
                let joined = list
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(&sep);
                Ok(PerlValue::String(joined))
            }
            Some(BuiltinId::Split) => {
                let mut iter = args.into_iter();
                let pat = iter
                    .next()
                    .unwrap_or(PerlValue::String(" ".into()))
                    .to_string();
                let s = iter.next().unwrap_or(PerlValue::Undef).to_string();
                let lim = iter.next().map(|v| v.to_int() as usize);
                let re =
                    regex::Regex::new(&pat).unwrap_or_else(|_| regex::Regex::new(" ").unwrap());
                let parts: Vec<PerlValue> = if let Some(l) = lim {
                    re.splitn(&s, l)
                        .map(|p| PerlValue::String(p.to_string()))
                        .collect()
                } else {
                    re.split(&s)
                        .map(|p| PerlValue::String(p.to_string()))
                        .collect()
                };
                Ok(PerlValue::Array(parts))
            }
            Some(BuiltinId::Sprintf) => {
                if args.is_empty() {
                    return Ok(PerlValue::String(String::new()));
                }
                let fmt = args[0].to_string();
                let rest = &args[1..];
                Ok(PerlValue::String(crate::interpreter::perl_sprintf(
                    &fmt, rest,
                )))
            }
            Some(BuiltinId::Reverse) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                Ok(match val {
                    PerlValue::Array(mut a) => {
                        a.reverse();
                        PerlValue::Array(a)
                    }
                    PerlValue::String(s) => PerlValue::String(s.chars().rev().collect()),
                    other => PerlValue::String(other.to_string().chars().rev().collect()),
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
                    msg.push_str(&format!(" at {} line {}", self.interp.file, line));
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
                Ok(PerlValue::Integer(1))
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
                Ok(PerlValue::Integer(
                    status.map(|s| s.code().unwrap_or(-1) as i64).unwrap_or(-1),
                ))
            }
            Some(BuiltinId::Chomp) => {
                // Chomp modifies the variable in-place — but in CallBuiltin we get the value, not a reference.
                // Return the number of chars removed (like Perl).
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                let s = val.to_string();
                Ok(PerlValue::Integer(if s.ends_with('\n') { 1 } else { 0 }))
            }
            Some(BuiltinId::Chop) => {
                let val = args.into_iter().next().unwrap_or(PerlValue::Undef);
                let s = val.to_string();
                Ok(s.chars()
                    .last()
                    .map(|c| PerlValue::String(c.to_string()))
                    .unwrap_or(PerlValue::Undef))
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
                Ok(PerlValue::String(
                    s.get(start..end).unwrap_or("").to_string(),
                ))
            }
            Some(BuiltinId::Index) => {
                let s = args.first().map(|v| v.to_string()).unwrap_or_default();
                let sub = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                let pos = args.get(2).map(|v| v.to_int() as usize).unwrap_or(0);
                Ok(PerlValue::Integer(
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
                Ok(PerlValue::Integer(
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
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                let mut chars = s.chars();
                let result = match chars.next() {
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                };
                Ok(PerlValue::String(result))
            }
            Some(BuiltinId::Lcfirst) => {
                let s = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                let mut chars = s.chars();
                let result = match chars.next() {
                    Some(c) => c.to_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                };
                Ok(PerlValue::String(result))
            }
            Some(BuiltinId::Splice) => {
                // Simplified — return empty array
                Ok(PerlValue::Array(vec![]))
            }
            Some(BuiltinId::Unshift) => Ok(PerlValue::Integer(0)),
            Some(BuiltinId::Printf) => {
                if args.is_empty() {
                    return Ok(PerlValue::Integer(1));
                }
                let fmt = args[0].to_string();
                let rest = &args[1..];
                print!("{}", crate::interpreter::perl_sprintf(&fmt, rest));
                let _ = io::stdout().flush();
                Ok(PerlValue::Integer(1))
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
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                self.interp.close_builtin_execute(name)
            }
            Some(BuiltinId::Eof) => {
                if args.is_empty() {
                    Ok(PerlValue::Integer(0))
                } else {
                    let name = args[0].to_string();
                    let at_eof = !self.interp.has_input_handle(&name);
                    Ok(PerlValue::Integer(if at_eof { 1 } else { 0 }))
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
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                Ok(PerlValue::Integer(
                    if std::env::set_current_dir(&path).is_ok() {
                        1
                    } else {
                        0
                    },
                ))
            }
            Some(BuiltinId::Mkdir) => {
                let path = args.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(PerlValue::Integer(if std::fs::create_dir(&path).is_ok() {
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
                Ok(PerlValue::Integer(count))
            }
            Some(BuiltinId::Rename) => {
                let old = args.first().map(|v| v.to_string()).unwrap_or_default();
                let new = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                Ok(crate::perl_fs::rename_paths(&old, &new))
            }
            Some(BuiltinId::Chmod) => {
                if args.is_empty() {
                    return Ok(PerlValue::Integer(0));
                }
                let mode = args[0].to_int();
                let paths: Vec<String> = args.iter().skip(1).map(|v| v.to_string()).collect();
                Ok(PerlValue::Integer(crate::perl_fs::chmod_paths(
                    &paths, mode,
                )))
            }
            Some(BuiltinId::Chown) => {
                if args.len() < 3 {
                    return Ok(PerlValue::Integer(0));
                }
                let uid = args[0].to_int();
                let gid = args[1].to_int();
                let paths: Vec<String> = args.iter().skip(2).map(|v| v.to_string()).collect();
                Ok(PerlValue::Integer(crate::perl_fs::chown_paths(
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
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                std::fs::read_to_string(&path)
                    .map(PerlValue::String)
                    .map_err(|e| PerlError::runtime(format!("slurp: {}", e), line))
            }
            Some(BuiltinId::Capture) => {
                let cmd = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                crate::capture::run_capture(&cmd, line)
            }
            Some(BuiltinId::Ppool) => {
                let n = args
                    .first()
                    .map(|v| v.to_int().max(0) as usize)
                    .unwrap_or(1);
                crate::ppool::create_pool(n)
            }
            Some(BuiltinId::Wantarray) => Ok(match self.interp.wantarray_kind {
                crate::interpreter::WantarrayCtx::Void => PerlValue::Undef,
                crate::interpreter::WantarrayCtx::Scalar => PerlValue::Integer(0),
                crate::interpreter::WantarrayCtx::List => PerlValue::Integer(1),
            }),
            Some(BuiltinId::FetchUrl) => {
                let url = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                ureq::get(&url)
                    .call()
                    .map_err(|e| PerlError::runtime(format!("fetch_url: {}", e), line))
                    .and_then(|r| {
                        r.into_string()
                            .map(PerlValue::String)
                            .map_err(|e| PerlError::runtime(format!("fetch_url: {}", e), line))
                    })
            }
            Some(BuiltinId::Pchannel) => Ok(crate::pchannel::create_pair()),
            Some(BuiltinId::DequeNew) => {
                if !args.is_empty() {
                    return Err(PerlError::runtime("deque() takes no arguments", line));
                }
                Ok(PerlValue::Deque(Arc::new(Mutex::new(VecDeque::new()))))
            }
            Some(BuiltinId::HeapNew) => {
                if args.len() != 1 {
                    return Err(PerlError::runtime(
                        "heap() expects one comparator sub",
                        line,
                    ));
                }
                let a0 = args.into_iter().next().unwrap_or(PerlValue::Undef);
                match a0 {
                    PerlValue::CodeRef(sub) => {
                        Ok(PerlValue::Heap(Arc::new(Mutex::new(PerlHeap {
                            items: Vec::new(),
                            cmp: sub.clone(),
                        }))))
                    }
                    _ => Err(PerlError::runtime("heap() requires a code reference", line)),
                }
            }
            Some(BuiltinId::Pipeline) => {
                let mut items = Vec::new();
                for v in args {
                    match v {
                        PerlValue::Array(a) => items.extend(a),
                        other => items.push(other),
                    }
                }
                Ok(PerlValue::Pipeline(Arc::new(Mutex::new(PipelineInner {
                    source: items,
                    ops: Vec::new(),
                }))))
            }
            Some(BuiltinId::Eval) => {
                let code = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                match crate::parse_and_run_string(&code, self.interp) {
                    Ok(v) => {
                        self.interp.eval_error = String::new();
                        Ok(v)
                    }
                    Err(e) => {
                        self.interp.eval_error = e.to_string();
                        Ok(PerlValue::Undef)
                    }
                }
            }
            Some(BuiltinId::Do) => {
                let filename = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                match std::fs::read_to_string(&filename) {
                    Ok(code) => {
                        crate::parse_and_run_string(&code, self.interp).or(Ok(PerlValue::Undef))
                    }
                    Err(_) => Ok(PerlValue::Undef),
                }
            }
            Some(BuiltinId::Require) => {
                let name = args
                    .into_iter()
                    .next()
                    .unwrap_or(PerlValue::Undef)
                    .to_string();
                let path = name.replace("::", "/") + ".pm";
                for dir in [".", "/usr/lib/perl5", "/usr/share/perl5"] {
                    let full = format!("{}/{}", dir, path);
                    if std::path::Path::new(&full).exists() {
                        if let Ok(code) = std::fs::read_to_string(&full) {
                            return crate::parse_and_run_string(&code, self.interp)
                                .or(Ok(PerlValue::Integer(1)));
                        }
                    }
                }
                Ok(PerlValue::Integer(1))
            }
            Some(BuiltinId::Bless) => {
                let ref_val = args.first().cloned().unwrap_or(PerlValue::Undef);
                let class = args
                    .get(1)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| self.interp.scope.get_scalar("__PACKAGE__").to_string());
                Ok(PerlValue::Blessed(Arc::new(crate::value::BlessedRef {
                    class,
                    data: RwLock::new(ref_val),
                })))
            }
            Some(BuiltinId::Caller) => Ok(PerlValue::Array(vec![
                PerlValue::String("main".into()),
                PerlValue::String(self.interp.file.clone()),
                PerlValue::Integer(line as i64),
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
            | Some(BuiltinId::Sort) => Ok(PerlValue::Undef),
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
    match (a, b) {
        (PerlValue::Integer(x), PerlValue::Integer(y)) => {
            PerlValue::Integer(if int_op(x, y) { 1 } else { 0 })
        }
        _ => PerlValue::Integer(if float_op(a.to_number(), b.to_number()) {
            1
        } else {
            0
        }),
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
        let i1 = c.add_constant(PerlValue::String("a".into()));
        let i2 = c.add_constant(PerlValue::String("b".into()));
        c.emit(Op::LoadConst(i1), 1);
        c.emit(Op::LoadConst(i2), 1);
        c.emit(Op::Concat, 1);
        c.emit(Op::Halt, 1);
        assert_eq!(run_chunk(&c).expect("vm").to_string(), "ab");

        let mut c = Chunk::new();
        let i1 = c.add_constant(PerlValue::String("a".into()));
        let i2 = c.add_constant(PerlValue::String("b".into()));
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
        assert!(matches!(run_chunk(&c).expect("vm"), PerlValue::Undef));

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
        let idx = c.add_constant(PerlValue::String("abc".into()));
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
        match v {
            PerlValue::Array(a) => {
                assert_eq!(a.len(), 2);
                assert_eq!(a[0].to_int(), 1);
                assert_eq!(a[1].to_int(), 2);
            }
            _ => panic!("expected array"),
        }
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
}
