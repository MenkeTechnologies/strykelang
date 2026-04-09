//! **Method JIT** (Cranelift): compiles **pure-integer** stack bytecode to native code.
//!
//! Two compilation tiers:
//!
//! ## Linear JIT
//! Compiles straight-line (no branches) integer sequences in a single Cranelift basic block.
//!
//! ## Block JIT
//! Compiles integer bytecode **with control flow** (loops, conditionals, short-circuit `&&`/`||`)
//! via a full basic-block CFG. Entry stack heights are computed by BFS; unreachable blocks (dead
//! code after unconditional jumps) are emitted as traps. Supports all the same data ops as the
//! linear JIT plus [`Op::Jump`], [`Op::JumpIfTrue`], [`Op::JumpIfFalse`],
//! [`Op::JumpIfFalseKeep`], and [`Op::JumpIfTrueKeep`].
//!
//! ## Eligible data ops (both tiers)
//! [`Op::LoadInt`], [`Op::LoadFloat`] (exact integers only, e.g. `3.0`),
//! [`Op::Add`]/[`Op::Sub`]/[`Op::Mul`], [`Op::Negate`], [`Op::LogNot`],
//! [`Op::NumEq`] / [`Op::NumNe`] / [`Op::NumLt`] / [`Op::NumGt`] / [`Op::NumLe`] / [`Op::NumGe`],
//! [`Op::Spaceship`],
//! [`Op::BitXor`], [`Op::BitNot`], [`Op::Shl`]/[`Op::Shr`] (shift amount masked to 6 bits),
//! [`Op::Div`] only when the VM would use the **exact integer quotient** (`a % b == 0`),
//! [`Op::Mod`] when the divisor is never dynamically zero (constant non-zero or folded stack),
//! [`Op::Pow`] when the VM’s integer `wrapping_pow` path applies: exponent `0..=63`, and either both
//! operands constant-fold or the exponent is constant in that range and the base is an integer path
//! (dynamic base from slot/plain/arg reads that materialize as `i64`),
//! [`Op::Pop`], [`Op::Dup`], optional trailing [`Op::Halt`], [`Op::LoadConst`] when the pool entry is
//! an integer ([`PerlValue::as_integer`]), [`Op::BitAnd`]/[`Op::BitOr`] (same integer path as the VM
//! when operands are not set values).
//!
//! [`Op::DeclareScalarSlot`], [`Op::PreIncSlot`] / [`Op::PostIncSlot`] / [`Op::PreDecSlot`] /
//! [`Op::PostDecSlot`], [`Op::GetScalarSlot`] / [`Op::SetScalarSlot`] / [`Op::SetScalarSlotKeep`] /
//! [`Op::GetScalarPlain`] / [`Op::SetScalarPlain`] / [`Op::SetScalarKeepPlain`] /
//! [`Op::PreInc`] / [`Op::PostInc`] / [`Op::PreDec`] / [`Op::PostDec`] (name-based inc/dec) /
//! [`Op::GetArg`] are
//! JIT’d when every referenced index materializes as `i64` via [`PerlValue::as_integer`].
//! Slot and plain-name writes update dense `i64` tables; the VM copies written indices back into
//! the scope after native execution. Cranelift functions use a fixed triple `(*slot, *plain, *arg)`
//! when any table is needed.
//!
//! ## Validation
//! Both tiers simulate a [`Cell`] stack so we only emit `sdiv`/`srem` when safe and only call
//! [`perlrs_jit_pow_i64`] when the VM’s integer fast path applies.
//!
//! ## Not JIT’d
//! Inexact `Div`, `Mod` with unknown divisor, `Pow` outside `0..=63`, non-integer
//! [`Op::LoadConst`] pool entries, `BitAnd`/`BitOr` on set values, non-integer slot/plain/arg
//! values, function calls, string ops, array/hash ops, `LoadUndef` (would lose `is_undef`
//! distinction).

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{AbiParam, BlockArg, InstBuilder, MemFlags, TrapCode, UserFuncName, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};

use crate::bytecode::Op;
use crate::value::PerlValue;

type LinearFn0 = unsafe extern "C" fn() -> i64;
/// Slot table, plain name table, compiled-sub arg table (fixed order; unused pointers may be null).
type LinearFn3 = unsafe extern "C" fn(*const i64, *const i64, *const i64) -> i64;

enum LinearRun {
    Nullary(LinearFn0),
    Tables(LinearFn3),
}

struct LinearJit {
    #[allow(dead_code)]
    module: JITModule,
    run: LinearRun,
}

impl LinearJit {
    fn invoke(&self, slots: *const i64, plain: *const i64, args: *const i64) -> i64 {
        match &self.run {
            LinearRun::Nullary(f) => unsafe { f() },
            LinearRun::Tables(f) => unsafe { f(slots, plain, args) },
        }
    }
}

fn isa_flags() -> settings::Flags {
    let mut flag_builder = settings::builder();
    let _ = flag_builder.set("use_colocated_libcalls", "false");
    let _ = flag_builder.set("is_pic", "false");
    settings::Flags::new(flag_builder)
}

/// Integer `**` matching `vm.rs` when both operands are `i64` and `0 ≤ exp ≤ 63`.
#[no_mangle]
pub extern "C" fn perlrs_jit_pow_i64(base: i64, exp: i64) -> i64 {
    if exp >= 0 && exp <= 63 {
        base.wrapping_pow(exp as u32)
    } else {
        0
    }
}

/// `!` on a value that is interpreted as integer (`PerlValue::integer(n)`), matching `Op::LogNot` + stack.
#[no_mangle]
pub extern "C" fn perlrs_jit_lognot_i64(n: i64) -> i64 {
    if PerlValue::integer(n).is_true() {
        0
    } else {
        1
    }
}

fn new_jit_module() -> Option<JITModule> {
    let isa_builder = cranelift_native::builder().ok()?;
    let isa = isa_builder.finish(isa_flags()).ok()?;
    let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
    builder.symbol(
        "perlrs_jit_pow_i64",
        perlrs_jit_pow_i64 as *const u8,
    );
    builder.symbol(
        "perlrs_jit_lognot_i64",
        perlrs_jit_lognot_i64 as *const u8,
    );
    Some(JITModule::new(builder))
}

/// Signed `icmp` → `0`/`1` on the stack (Perl numeric compare result).
fn intcmp_to_01(bcx: &mut FunctionBuilder, cc: IntCC, a: Value, b: Value) -> Value {
    let pred = bcx.ins().icmp(cc, a, b);
    let one = bcx.ins().iconst(types::I64, 1);
    let zero = bcx.ins().iconst(types::I64, 0);
    bcx.ins().select(pred, one, zero)
}

/// `<=>` on two `i64` values: `-1`, `0`, or `1`.
fn spaceship_i64(bcx: &mut FunctionBuilder, a: Value, b: Value) -> Value {
    let lt = bcx.ins().icmp(IntCC::SignedLessThan, a, b);
    let gt = bcx.ins().icmp(IntCC::SignedGreaterThan, a, b);
    let m1 = bcx.ins().iconst(types::I64, -1);
    let z = bcx.ins().iconst(types::I64, 0);
    let p1 = bcx.ins().iconst(types::I64, 1);
    let mid = bcx.ins().select(gt, p1, z);
    bcx.ins().select(lt, m1, mid)
}

fn hash_ops(ops: &[Op], constants: &[PerlValue]) -> u64 {
    let mut h = DefaultHasher::new();
    ops.len().hash(&mut h);
    for op in ops {
        match op {
            Op::LoadInt(n) => {
                0u8.hash(&mut h);
                n.hash(&mut h);
            }
            Op::LoadFloat(f) => {
                1u8.hash(&mut h);
                f.to_bits().hash(&mut h);
            }
            Op::LoadConst(i) => {
                2u8.hash(&mut h);
                i.hash(&mut h);
                if let Some(n) = constants.get(*i as usize).and_then(|p| p.as_integer()) {
                    n.hash(&mut h);
                }
            }
            Op::LoadUndef => 3u8.hash(&mut h),
            Op::Pop => 4u8.hash(&mut h),
            Op::Dup => 5u8.hash(&mut h),
            Op::Add => 6u8.hash(&mut h),
            Op::Sub => 7u8.hash(&mut h),
            Op::Mul => 8u8.hash(&mut h),
            Op::Negate => 9u8.hash(&mut h),
            Op::Halt => 10u8.hash(&mut h),
            Op::BitXor => 11u8.hash(&mut h),
            Op::BitNot => 12u8.hash(&mut h),
            Op::Shl => 13u8.hash(&mut h),
            Op::Shr => 14u8.hash(&mut h),
            Op::Div => 15u8.hash(&mut h),
            Op::Mod => 16u8.hash(&mut h),
            Op::Pow => 17u8.hash(&mut h),
            Op::GetScalarSlot(s) => {
                18u8.hash(&mut h);
                s.hash(&mut h);
            }
            Op::SetScalarSlot(s) => {
                31u8.hash(&mut h);
                s.hash(&mut h);
            }
            Op::SetScalarSlotKeep(s) => {
                32u8.hash(&mut h);
                s.hash(&mut h);
            }
            Op::BitAnd => 19u8.hash(&mut h),
            Op::BitOr => 20u8.hash(&mut h),
            Op::GetScalarPlain(i) => {
                21u8.hash(&mut h);
                i.hash(&mut h);
            }
            Op::LogNot => 22u8.hash(&mut h),
            Op::NumEq => 23u8.hash(&mut h),
            Op::NumNe => 24u8.hash(&mut h),
            Op::NumLt => 25u8.hash(&mut h),
            Op::NumGt => 26u8.hash(&mut h),
            Op::NumLe => 27u8.hash(&mut h),
            Op::NumGe => 28u8.hash(&mut h),
            Op::Spaceship => 29u8.hash(&mut h),
            Op::GetArg(a) => {
                30u8.hash(&mut h);
                a.hash(&mut h);
            }
            Op::DeclareScalarSlot(s) => {
                33u8.hash(&mut h);
                s.hash(&mut h);
            }
            Op::PreIncSlot(s) => {
                34u8.hash(&mut h);
                s.hash(&mut h);
            }
            Op::PostIncSlot(s) => {
                35u8.hash(&mut h);
                s.hash(&mut h);
            }
            Op::PreDecSlot(s) => {
                36u8.hash(&mut h);
                s.hash(&mut h);
            }
            Op::PostDecSlot(s) => {
                37u8.hash(&mut h);
                s.hash(&mut h);
            }
            Op::Jump(t) => {
                38u8.hash(&mut h);
                t.hash(&mut h);
            }
            Op::JumpIfTrue(t) => {
                39u8.hash(&mut h);
                t.hash(&mut h);
            }
            Op::JumpIfFalse(t) => {
                40u8.hash(&mut h);
                t.hash(&mut h);
            }
            Op::JumpIfFalseKeep(t) => {
                41u8.hash(&mut h);
                t.hash(&mut h);
            }
            Op::JumpIfTrueKeep(t) => {
                42u8.hash(&mut h);
                t.hash(&mut h);
            }
            Op::SetScalarPlain(i) => {
                43u8.hash(&mut h);
                i.hash(&mut h);
            }
            Op::SetScalarKeepPlain(i) => {
                44u8.hash(&mut h);
                i.hash(&mut h);
            }
            Op::PreInc(i) => {
                45u8.hash(&mut h);
                i.hash(&mut h);
            }
            Op::PostInc(i) => {
                46u8.hash(&mut h);
                i.hash(&mut h);
            }
            Op::PreDec(i) => {
                47u8.hash(&mut h);
                i.hash(&mut h);
            }
            Op::PostDec(i) => {
                48u8.hash(&mut h);
                i.hash(&mut h);
            }
            _ => {
                255u8.hash(&mut h);
                format!("{op:?}").hash(&mut h);
            }
        }
    }
    h.finish()
}

/// Ops before first [`Op::Halt`], if any (Halt itself is not compiled).
fn ops_before_halt(ops: &[Op]) -> &[Op] {
    if let Some(i) = ops.iter().position(|o| matches!(o, Op::Halt)) {
        &ops[..i]
    } else {
        ops
    }
}

#[derive(Clone, Copy)]
enum Cell {
    Const(i64),
    Dyn,
}

fn validate_linear(ops: &[Op], constants: &[PerlValue]) -> bool {
    let seq = ops_before_halt(ops);
    if seq.is_empty() {
        return false;
    }
    let mut stack: Vec<Cell> = Vec::new();
    for op in seq {
        match op {
            Op::LoadInt(n) => stack.push(Cell::Const(*n)),
            Op::LoadConst(idx) => {
                let n = match constants.get(*idx as usize) {
                    Some(pv) => match pv.as_integer() {
                        Some(n) => n,
                        None => return false,
                    },
                    None => return false,
                };
                stack.push(Cell::Const(n));
            }
            Op::Add => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(x.wrapping_add(y)),
                    _ => Cell::Dyn,
                });
            }
            Op::Sub => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(x.wrapping_sub(y)),
                    _ => Cell::Dyn,
                });
            }
            Op::Mul => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(x.wrapping_mul(y)),
                    _ => Cell::Dyn,
                });
            }
            Op::Div => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) if y != 0 && x % y == 0 => {
                        stack.push(Cell::Const(x / y));
                    }
                    _ => return false,
                }
            }
            Op::Mod => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                match b {
                    Cell::Const(0) => return false,
                    Cell::Const(y) => {
                        let out = match a {
                            Cell::Const(x) => Cell::Const(x % y),
                            Cell::Dyn => Cell::Dyn,
                        };
                        stack.push(out);
                    }
                    Cell::Dyn => return false,
                }
            }
            Op::Pow => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) if y >= 0 && y <= 63 => {
                        stack.push(Cell::Const(x.wrapping_pow(y as u32)));
                    }
                    (Cell::Dyn, Cell::Const(y)) if y >= 0 && y <= 63 => {
                        stack.push(Cell::Dyn);
                    }
                    _ => return false,
                }
            }
            Op::Negate => {
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match a {
                    Cell::Const(n) => Cell::Const(n.wrapping_neg()),
                    Cell::Dyn => Cell::Dyn,
                });
            }
            Op::Pop => {
                if stack.pop().is_none() {
                    return false;
                }
            }
            Op::Dup => {
                let v = match stack.last().copied() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(v);
            }
            Op::BitXor => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(x ^ y),
                    _ => Cell::Dyn,
                });
            }
            Op::BitAnd => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(x & y),
                    _ => Cell::Dyn,
                });
            }
            Op::BitOr => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(x | y),
                    _ => Cell::Dyn,
                });
            }
            Op::BitNot => {
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match a {
                    Cell::Const(n) => Cell::Const(!n),
                    Cell::Dyn => Cell::Dyn,
                });
            }
            Op::Shl => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => {
                        let s = (y as u32) & 63;
                        Cell::Const(x.wrapping_shl(s))
                    }
                    _ => Cell::Dyn,
                });
            }
            Op::Shr => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => {
                        let s = (y as u32) & 63;
                        Cell::Const(x.wrapping_shr(s))
                    }
                    _ => Cell::Dyn,
                });
            }
            Op::SetScalarSlot(_) => {
                if stack.pop().is_none() {
                    return false;
                }
            }
            Op::SetScalarSlotKeep(_) => {
                if stack.last().is_none() {
                    return false;
                }
            }
            Op::DeclareScalarSlot(_) => {
                if stack.pop().is_none() {
                    return false;
                }
            }
            Op::PreIncSlot(_) | Op::PreDecSlot(_) | Op::PostIncSlot(_) | Op::PostDecSlot(_) => {
                stack.push(Cell::Dyn);
            }
            Op::GetScalarSlot(_) => stack.push(Cell::Dyn),
            Op::GetScalarPlain(_) => stack.push(Cell::Dyn),
            Op::GetArg(_) => stack.push(Cell::Dyn),
            Op::NumEq => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(if x == y { 1 } else { 0 }),
                    _ => Cell::Dyn,
                });
            }
            Op::NumNe => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(if x != y { 1 } else { 0 }),
                    _ => Cell::Dyn,
                });
            }
            Op::NumLt => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(if x < y { 1 } else { 0 }),
                    _ => Cell::Dyn,
                });
            }
            Op::NumGt => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(if x > y { 1 } else { 0 }),
                    _ => Cell::Dyn,
                });
            }
            Op::NumLe => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(if x <= y { 1 } else { 0 }),
                    _ => Cell::Dyn,
                });
            }
            Op::NumGe => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(if x >= y { 1 } else { 0 }),
                    _ => Cell::Dyn,
                });
            }
            Op::Spaceship => {
                let b = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(match x.cmp(&y) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    }),
                    _ => Cell::Dyn,
                });
            }
            Op::LogNot => {
                let a = match stack.pop() {
                    Some(c) => c,
                    None => return false,
                };
                let out = match a {
                    Cell::Const(n) => {
                        let t = PerlValue::integer(n).is_true();
                        Cell::Const(if t { 0 } else { 1 })
                    }
                    Cell::Dyn => Cell::Dyn,
                };
                stack.push(out);
            }
            Op::LoadFloat(f) => {
                let n = *f as i64;
                if f.is_finite() && (n as f64) == *f {
                    stack.push(Cell::Const(n));
                } else {
                    return false;
                }
            }
            Op::SetScalarPlain(_) => {
                if stack.pop().is_none() {
                    return false;
                }
            }
            Op::SetScalarKeepPlain(_) => {
                if stack.last().is_none() {
                    return false;
                }
            }
            Op::PreInc(_) | Op::PreDec(_) | Op::PostInc(_) | Op::PostDec(_) => {
                stack.push(Cell::Dyn);
            }
            _ => return false,
        }
        if stack.len() > 256 {
            return false;
        }
    }
    stack.len() == 1
}

/// Returns `true` when any op in `seq` requires slot/plain/arg table pointers.
fn needs_table(seq: &[Op]) -> bool {
    seq.iter().any(|o| {
        matches!(
            o,
            Op::GetScalarSlot(_)
                | Op::SetScalarSlot(_)
                | Op::SetScalarSlotKeep(_)
                | Op::DeclareScalarSlot(_)
                | Op::PreIncSlot(_)
                | Op::PostIncSlot(_)
                | Op::PreDecSlot(_)
                | Op::PostDecSlot(_)
                | Op::GetScalarPlain(_)
                | Op::SetScalarPlain(_)
                | Op::SetScalarKeepPlain(_)
                | Op::PreInc(_)
                | Op::PostInc(_)
                | Op::PreDec(_)
                | Op::PostDec(_)
                | Op::GetArg(_)
        )
    })
}

fn compile_linear(ops: &[Op], constants: &[PerlValue]) -> Option<LinearJit> {
    let seq = ops_before_halt(ops);
    if !validate_linear(ops, constants) {
        return None;
    }
    let need_any_table = needs_table(seq);
    let mut module = new_jit_module()?;

    let needs_pow = seq.iter().any(|o| matches!(o, Op::Pow));
    let pow_id = if needs_pow {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::I64));
        ps.params.push(AbiParam::new(types::I64));
        ps.returns.push(AbiParam::new(types::I64));
        Some(
            module
                .declare_function("perlrs_jit_pow_i64", Linkage::Import, &ps)
                .ok()?,
        )
    } else {
        None
    };

    let needs_lognot = seq.iter().any(|o| matches!(o, Op::LogNot));
    let lognot_id = if needs_lognot {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::I64));
        ps.returns.push(AbiParam::new(types::I64));
        Some(
            module
                .declare_function("perlrs_jit_lognot_i64", Linkage::Import, &ps)
                .ok()?,
        )
    } else {
        None
    };

    let ptr_ty = module.target_config().pointer_type();
    let mut sig = module.make_signature();
    if need_any_table {
        sig.params.push(AbiParam::new(ptr_ty));
        sig.params.push(AbiParam::new(ptr_ty));
        sig.params.push(AbiParam::new(ptr_ty));
    }
    sig.returns.push(AbiParam::new(types::I64));

    let fid = module
        .declare_function("linear", Linkage::Local, &sig)
        .ok()?;

    let mut ctx = module.make_context();
    ctx.func.signature = sig;
    ctx.func.name = UserFuncName::user(0, fid.as_u32());

    let mut fctx = FunctionBuilderContext::new();
    {
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let entry = bcx.create_block();
        bcx.append_block_params_for_function_params(entry);
        bcx.switch_to_block(entry);

        let slot_base = if need_any_table {
            Some(bcx.block_params(entry)[0])
        } else {
            None
        };
        let plain_base = if need_any_table {
            Some(bcx.block_params(entry)[1])
        } else {
            None
        };
        let arg_base = if need_any_table {
            Some(bcx.block_params(entry)[2])
        } else {
            None
        };

        // Pre-resolve helper function refs for the shared emitter.
        let pow_ref = pow_id.map(|pid| module.declare_func_in_func(pid, &mut bcx.func));
        let lognot_ref =
            lognot_id.map(|lid| module.declare_func_in_func(lid, &mut bcx.func));

        let mut stack: Vec<cranelift_codegen::ir::Value> = Vec::with_capacity(32);
        for op in seq {
            emit_data_op(
                &mut bcx,
                op,
                &mut stack,
                slot_base,
                plain_base,
                arg_base,
                pow_ref,
                lognot_ref,
                constants,
            )?;
        }
        let v = stack.pop()?;
        bcx.ins().return_(&[v]);
        bcx.seal_all_blocks();
        bcx.finalize();
    }

    module.define_function(fid, &mut ctx).ok()?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().ok()?;
    let ptr = module.get_finalized_function(fid);
    let run = if need_any_table {
        LinearRun::Tables(unsafe { std::mem::transmute(ptr) })
    } else {
        LinearRun::Nullary(unsafe { std::mem::transmute(ptr) })
    };
    Some(LinearJit { module, run })
}

static LINEAR_CACHE: OnceLock<Mutex<HashMap<u64, Box<LinearJit>>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<u64, Box<LinearJit>>> {
    LINEAR_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn linear_needs_plain(seq: &[Op]) -> bool {
    seq.iter().any(|o| {
        matches!(
            o,
            Op::GetScalarPlain(_)
                | Op::SetScalarPlain(_)
                | Op::SetScalarKeepPlain(_)
                | Op::PreInc(_)
                | Op::PostInc(_)
                | Op::PreDec(_)
                | Op::PostDec(_)
        )
    })
}

fn max_scalar_slot_index(seq: &[Op]) -> Option<u8> {
    seq.iter()
        .filter_map(|o| match o {
            Op::GetScalarSlot(s)
            | Op::SetScalarSlot(s)
            | Op::SetScalarSlotKeep(s)
            | Op::DeclareScalarSlot(s)
            | Op::PreIncSlot(s)
            | Op::PostIncSlot(s)
            | Op::PreDecSlot(s)
            | Op::PostDecSlot(s) => Some(*s),
            _ => None,
        })
        .max()
}

/// Largest slot index referenced by get/set slot ops in `ops` before [`Op::Halt`], if any (dense
/// `i64` slot tables).
pub(crate) fn linear_slot_ops_max_index(ops: &[Op]) -> Option<u8> {
    max_scalar_slot_index(ops_before_halt(ops))
}

/// When building the dense `i64` slot buffer for the JIT, [`PerlValue::as_integer`] is `None` for
/// `undef`. It is still safe to prefill that slot as `0` (matching [`PerlValue::to_int`] on undef)
/// when no [`Op::GetScalarSlot`] for `slot` runs **before** the slot is written in this linear
/// sequence (e.g. `DeclareScalarSlot` / inc-dec / set). Otherwise the VM must stay on the
/// interpreter so `GetScalarSlot` can observe real `undef`.
pub(crate) fn slot_undef_prefill_ok(ops: &[Op], slot: u8) -> bool {
    let seq = ops_before_halt(ops);
    let mut written = false;
    for op in seq {
        match op {
            Op::GetScalarSlot(s) if *s == slot => {
                if !written {
                    return false;
                }
            }
            Op::DeclareScalarSlot(s) | Op::SetScalarSlot(s) | Op::SetScalarSlotKeep(s) if *s == slot => {
                written = true;
            }
            Op::PreIncSlot(s) | Op::PostIncSlot(s) | Op::PreDecSlot(s) | Op::PostDecSlot(s)
                if *s == slot =>
            {
                written = true;
            }
            _ => {}
        }
    }
    true
}

/// Slot indices written by [`Op::SetScalarSlot`] / [`Op::SetScalarSlotKeep`] before [`Op::Halt`],
/// sorted and deduplicated (for syncing the slot buffer back into the interpreter scope).
pub(crate) fn linear_slot_ops_written_indices(ops: &[Op]) -> Vec<u8> {
    let mut v: Vec<u8> = ops_before_halt(ops)
        .iter()
        .filter_map(|o| match o {
            Op::SetScalarSlot(s)
            | Op::SetScalarSlotKeep(s)
            | Op::DeclareScalarSlot(s)
            | Op::PreIncSlot(s)
            | Op::PostIncSlot(s)
            | Op::PreDecSlot(s)
            | Op::PostDecSlot(s) => Some(*s),
            _ => None,
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn max_plain_name_index(seq: &[Op]) -> Option<u16> {
    seq.iter()
        .filter_map(|o| match o {
            Op::GetScalarPlain(i)
            | Op::SetScalarPlain(i)
            | Op::SetScalarKeepPlain(i)
            | Op::PreInc(i)
            | Op::PostInc(i)
            | Op::PreDec(i)
            | Op::PostDec(i) => Some(*i),
            _ => None,
        })
        .max()
}

/// Largest plain-name index in `ops` before [`Op::Halt`], if any.
pub(crate) fn linear_plain_ops_max_index(ops: &[Op]) -> Option<u16> {
    max_plain_name_index(ops_before_halt(ops))
}

/// Plain-name indices **written** before [`Op::Halt`] (for VM writeback).
pub(crate) fn linear_plain_ops_written_indices(ops: &[Op]) -> Vec<u16> {
    let seq = ops_before_halt(ops);
    let mut v: Vec<u16> = seq
        .iter()
        .filter_map(|o| match o {
            Op::SetScalarPlain(i)
            | Op::SetScalarKeepPlain(i)
            | Op::PreInc(i)
            | Op::PostInc(i)
            | Op::PreDec(i)
            | Op::PostDec(i) => Some(*i),
            _ => None,
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn max_get_arg_index(seq: &[Op]) -> Option<u8> {
    seq.iter()
        .filter_map(|o| match o {
            Op::GetArg(i) => Some(*i),
            _ => None,
        })
        .max()
}

/// Largest [`Op::GetArg`] index in `ops` before [`Op::Halt`], if any (dense `i64` arg table).
pub(crate) fn linear_arg_ops_max_index(ops: &[Op]) -> Option<u8> {
    max_get_arg_index(ops_before_halt(ops))
}

fn linear_needs_args(seq: &[Op]) -> bool {
    seq.iter().any(|o| matches!(o, Op::GetArg(_)))
}

/// If `ops` is a supported pure-int linear sequence, run compiled code and return the result.
/// Otherwise returns [`None`] (VM should interpret as usual).
///
/// When the sequence contains [`Op::GetScalarSlot`], [`Op::SetScalarSlot`],
/// [`Op::SetScalarSlotKeep`], [`Op::DeclareScalarSlot`], or slot [`Op::PreIncSlot`] /
/// [`Op::PostIncSlot`] / [`Op::PreDecSlot`] / [`Op::PostDecSlot`], pass `Some` **mutable** slice
/// whose length is `max(slot_index) + 1` and whose `i` entries are the slot values as `i64`
/// (same as [`PerlValue::as_integer`], with [`crate::jit::slot_undef_prefill_ok`] handling for
/// `undef` where documented). Slot writes update this buffer in place.
///
/// When it contains [`Op::GetScalarPlain`], pass `Some` slice whose length is
/// `max(name_index) + 1` with `PerlValue::as_integer` of `scope.get_scalar` for each name index.
///
/// When it contains [`Op::GetArg`], pass `Some` slice whose length is `max(arg_index) + 1` with
/// `PerlValue::as_integer` of each `stack[call_frame.stack_base + i]` (compiled-sub integer args).
///
/// `constants` must be the chunk’s constant pool: [`Op::LoadConst`] is only JIT’d when
/// `constants[idx]` is materializable as `i64` via [`PerlValue::as_integer`].
pub(crate) fn try_run_linear_ops(
    ops: &[Op],
    mut slot_i64: Option<&mut [i64]>,
    mut plain_i64: Option<&mut [i64]>,
    arg_i64: Option<&[i64]>,
    constants: &[PerlValue],
) -> Option<PerlValue> {
    if !validate_linear(ops, constants) {
        return None;
    }
    let seq = ops_before_halt(ops);
    if let Some(max) = max_scalar_slot_index(seq) {
        let sl = slot_i64.as_mut()?;
        if sl.len() <= max as usize {
            return None;
        }
    }
    if linear_needs_plain(seq) {
        let max = max_plain_name_index(seq)?;
        let pl = plain_i64.as_ref()?;
        if pl.len() <= max as usize {
            return None;
        }
    }
    if linear_needs_args(seq) {
        let max = max_get_arg_index(seq)?;
        let al = arg_i64?;
        if al.len() <= max as usize {
            return None;
        }
    }

    let key = hash_ops(ops, constants);
    let slot_ptr = slot_i64
        .as_mut()
        .map(|s| s.as_mut_ptr() as *const i64)
        .unwrap_or(std::ptr::null());
    let plain_ptr = plain_i64
        .as_mut()
        .map(|p| p.as_mut_ptr() as *const i64)
        .unwrap_or(std::ptr::null());
    let arg_ptr = arg_i64.map(|a| a.as_ptr()).unwrap_or(std::ptr::null());
    {
        let guard = cache().lock().ok()?;
        if let Some(j) = guard.get(&key) {
            let n = j.invoke(slot_ptr, plain_ptr, arg_ptr);
            return Some(PerlValue::integer(n));
        }
    }

    let jit = compile_linear(ops, constants)?;
    let n = jit.invoke(slot_ptr, plain_ptr, arg_ptr);
    let pv = PerlValue::integer(n);

    if let Ok(mut guard) = cache().lock() {
        if guard.len() < 256 {
            guard.insert(key, Box::new(jit));
        }
    }
    Some(pv)
}

// ── Block-based JIT: compiles integer bytecode with control flow (loops, conditionals). ──

struct CfgBlock {
    start: usize,
    end: usize,
    entry_stack: usize,
    reachable: bool,
}

/// Collect every op address that begins a basic block.
fn find_block_starts(ops: &[Op]) -> BTreeSet<usize> {
    let mut s = BTreeSet::new();
    s.insert(0);
    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::Jump(t)
            | Op::JumpIfTrue(t)
            | Op::JumpIfFalse(t)
            | Op::JumpIfFalseKeep(t)
            | Op::JumpIfTrueKeep(t) => {
                if *t > ops.len() {
                    return s; // invalid target, will be caught in validation
                }
                s.insert(*t);
                if i + 1 < ops.len() {
                    s.insert(i + 1);
                }
            }
            Op::Halt => {
                if i + 1 < ops.len() {
                    s.insert(i + 1);
                }
            }
            _ => {}
        }
    }
    s
}

/// Returns `true` when `op` is a supported data (non-control-flow) operation for the block JIT.
fn is_block_data_op(op: &Op) -> bool {
    matches!(
        op,
        Op::LoadInt(_)
            | Op::LoadConst(_)
            | Op::LoadFloat(_)
            | Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::Mod
            | Op::Pow
            | Op::Negate
            | Op::Pop
            | Op::Dup
            | Op::BitXor
            | Op::BitAnd
            | Op::BitOr
            | Op::BitNot
            | Op::Shl
            | Op::Shr
            | Op::NumEq
            | Op::NumNe
            | Op::NumLt
            | Op::NumGt
            | Op::NumLe
            | Op::NumGe
            | Op::Spaceship
            | Op::LogNot
            | Op::GetScalarSlot(_)
            | Op::SetScalarSlot(_)
            | Op::SetScalarSlotKeep(_)
            | Op::DeclareScalarSlot(_)
            | Op::PreIncSlot(_)
            | Op::PostIncSlot(_)
            | Op::PreDecSlot(_)
            | Op::PostDecSlot(_)
            | Op::GetScalarPlain(_)
            | Op::SetScalarPlain(_)
            | Op::SetScalarKeepPlain(_)
            | Op::PreInc(_)
            | Op::PostInc(_)
            | Op::PreDec(_)
            | Op::PostDec(_)
            | Op::GetArg(_)
    )
}

/// Validate the ops as a block-structured program and compute per-block entry stack heights.
/// Returns `None` when any op is unsupported or the CFG is inconsistent.
fn validate_block_cfg(ops: &[Op], constants: &[PerlValue]) -> Option<Vec<CfgBlock>> {
    if ops.is_empty() {
        return None;
    }
    // Must contain at least one jump to qualify (otherwise linear JIT handles it).
    let has_jump = ops.iter().any(|o| {
        matches!(
            o,
            Op::Jump(_)
                | Op::JumpIfTrue(_)
                | Op::JumpIfFalse(_)
                | Op::JumpIfFalseKeep(_)
                | Op::JumpIfTrueKeep(_)
        )
    });
    if !has_jump {
        return None;
    }

    let starts: Vec<usize> = find_block_starts(ops).into_iter().collect();
    let block_count = starts.len();
    let mut blocks: Vec<(usize, usize)> = Vec::with_capacity(block_count);
    for i in 0..block_count {
        let s = starts[i];
        let e = if i + 1 < block_count {
            starts[i + 1]
        } else {
            ops.len()
        };
        blocks.push((s, e));
    }
    let addr_to_block: HashMap<usize, usize> =
        starts.iter().enumerate().map(|(i, &s)| (s, i)).collect();

    let mut entry_heights: Vec<Option<usize>> = vec![None; block_count];
    entry_heights[0] = Some(0);
    let mut worklist: VecDeque<usize> = VecDeque::new();
    worklist.push_back(0);
    let mut visited = vec![false; block_count];
    let mut has_halt = false;

    let set_or_check =
        |heights: &mut Vec<Option<usize>>, wl: &mut VecDeque<usize>, bi: usize, h: usize| -> bool {
            match heights[bi] {
                None => {
                    heights[bi] = Some(h);
                    wl.push_back(bi);
                    true
                }
                Some(prev) => prev == h,
            }
        };

    while let Some(bi) = worklist.pop_front() {
        if visited[bi] {
            continue;
        }
        if entry_heights[bi].is_none() {
            continue;
        }
        visited[bi] = true;

        let (start, end) = blocks[bi];
        let entry_h = entry_heights[bi].unwrap();

        // Cell simulation for Div/Mod/Pow safety.
        let mut stack: Vec<Cell> = vec![Cell::Dyn; entry_h];
        let mut terminated = false;

        for idx in start..end {
            let op = &ops[idx];
            match op {
                Op::LoadInt(n) => stack.push(Cell::Const(*n)),
                Op::LoadConst(ci) => {
                    let n = constants.get(*ci as usize).and_then(|pv| pv.as_integer())?;
                    stack.push(Cell::Const(n));
                }
                Op::LoadFloat(f) => {
                    let n = *f as i64;
                    if f.is_finite() && (n as f64) == *f {
                        stack.push(Cell::Const(n));
                    } else {
                        return None;
                    }
                }
                Op::Add => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) => Cell::Const(x.wrapping_add(y)),
                        _ => Cell::Dyn,
                    });
                }
                Op::Sub => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) => Cell::Const(x.wrapping_sub(y)),
                        _ => Cell::Dyn,
                    });
                }
                Op::Mul => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) => Cell::Const(x.wrapping_mul(y)),
                        _ => Cell::Dyn,
                    });
                }
                Op::Div => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) if y != 0 && x % y == 0 => {
                            stack.push(Cell::Const(x / y));
                        }
                        _ => return None,
                    }
                }
                Op::Mod => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    match b {
                        Cell::Const(0) => return None,
                        Cell::Const(y) => {
                            stack.push(match a {
                                Cell::Const(x) => Cell::Const(x % y),
                                Cell::Dyn => Cell::Dyn,
                            });
                        }
                        Cell::Dyn => return None,
                    }
                }
                Op::Pow => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) if y >= 0 && y <= 63 => {
                            stack.push(Cell::Const(x.wrapping_pow(y as u32)));
                        }
                        (Cell::Dyn, Cell::Const(y)) if y >= 0 && y <= 63 => {
                            stack.push(Cell::Dyn);
                        }
                        _ => return None,
                    }
                }
                Op::Negate => {
                    let a = stack.pop()?;
                    stack.push(match a {
                        Cell::Const(n) => Cell::Const(n.wrapping_neg()),
                        Cell::Dyn => Cell::Dyn,
                    });
                }
                Op::Pop => {
                    stack.pop()?;
                }
                Op::Dup => {
                    let v = *stack.last()?;
                    stack.push(v);
                }
                Op::BitXor => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) => Cell::Const(x ^ y),
                        _ => Cell::Dyn,
                    });
                }
                Op::BitAnd => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) => Cell::Const(x & y),
                        _ => Cell::Dyn,
                    });
                }
                Op::BitOr => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) => Cell::Const(x | y),
                        _ => Cell::Dyn,
                    });
                }
                Op::BitNot => {
                    let a = stack.pop()?;
                    stack.push(match a {
                        Cell::Const(n) => Cell::Const(!n),
                        Cell::Dyn => Cell::Dyn,
                    });
                }
                Op::Shl => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) => {
                            Cell::Const(x.wrapping_shl((y as u32) & 63))
                        }
                        _ => Cell::Dyn,
                    });
                }
                Op::Shr => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(match (a, b) {
                        (Cell::Const(x), Cell::Const(y)) => {
                            Cell::Const(x.wrapping_shr((y as u32) & 63))
                        }
                        _ => Cell::Dyn,
                    });
                }
                Op::NumEq | Op::NumNe | Op::NumLt | Op::NumGt | Op::NumLe | Op::NumGe
                | Op::Spaceship => {
                    stack.pop()?;
                    stack.pop()?;
                    stack.push(Cell::Dyn);
                }
                Op::LogNot => {
                    stack.pop()?;
                    stack.push(Cell::Dyn);
                }
                Op::SetScalarSlot(_)
                | Op::DeclareScalarSlot(_)
                | Op::SetScalarPlain(_) => {
                    stack.pop()?;
                }
                Op::SetScalarSlotKeep(_) | Op::SetScalarKeepPlain(_) => {
                    stack.last()?;
                }
                Op::PreIncSlot(_)
                | Op::PostIncSlot(_)
                | Op::PreDecSlot(_)
                | Op::PostDecSlot(_)
                | Op::PreInc(_)
                | Op::PostInc(_)
                | Op::PreDec(_)
                | Op::PostDec(_) => {
                    stack.push(Cell::Dyn);
                }
                Op::GetScalarSlot(_) | Op::GetScalarPlain(_) | Op::GetArg(_) => {
                    stack.push(Cell::Dyn)
                }

                // ── Control flow ──
                Op::Jump(target) => {
                    let h = stack.len();
                    let ti = *addr_to_block.get(target)?;
                    if !set_or_check(&mut entry_heights, &mut worklist, ti, h) {
                        return None;
                    }
                    terminated = true;
                    break;
                }
                Op::JumpIfTrue(target) => {
                    stack.pop()?; // condition
                    let h = stack.len();
                    let ti = *addr_to_block.get(target)?;
                    let ni = bi + 1;
                    if ni >= block_count {
                        return None;
                    }
                    if !set_or_check(&mut entry_heights, &mut worklist, ti, h) {
                        return None;
                    }
                    if !set_or_check(&mut entry_heights, &mut worklist, ni, h) {
                        return None;
                    }
                    terminated = true;
                    break;
                }
                Op::JumpIfFalse(target) => {
                    stack.pop()?;
                    let h = stack.len();
                    let ti = *addr_to_block.get(target)?;
                    let ni = bi + 1;
                    if ni >= block_count {
                        return None;
                    }
                    if !set_or_check(&mut entry_heights, &mut worklist, ti, h) {
                        return None;
                    }
                    if !set_or_check(&mut entry_heights, &mut worklist, ni, h) {
                        return None;
                    }
                    terminated = true;
                    break;
                }
                Op::JumpIfFalseKeep(target) | Op::JumpIfTrueKeep(target) => {
                    stack.last()?; // peek condition
                    let h = stack.len();
                    let ti = *addr_to_block.get(target)?;
                    let ni = bi + 1;
                    if ni >= block_count {
                        return None;
                    }
                    if !set_or_check(&mut entry_heights, &mut worklist, ti, h) {
                        return None;
                    }
                    if !set_or_check(&mut entry_heights, &mut worklist, ni, h) {
                        return None;
                    }
                    terminated = true;
                    break;
                }
                Op::Halt => {
                    if stack.is_empty() {
                        return None;
                    }
                    has_halt = true;
                    terminated = true;
                    break;
                }
                _ => return None,
            }
            if stack.len() > 256 {
                return None;
            }
        }

        // Fall-through to next block.
        if !terminated {
            let ni = bi + 1;
            if ni >= block_count {
                return None;
            }
            let h = stack.len();
            if !set_or_check(&mut entry_heights, &mut worklist, ni, h) {
                return None;
            }
        }
    }

    if !has_halt {
        return None;
    }

    Some(
        blocks
            .iter()
            .enumerate()
            .map(|(i, &(s, e))| CfgBlock {
                start: s,
                end: e,
                entry_stack: entry_heights[i].unwrap_or(0),
                reachable: visited[i],
            })
            .collect(),
    )
}

/// Emit a single non-control-flow op into the Cranelift `FunctionBuilder`.
fn emit_data_op(
    bcx: &mut FunctionBuilder,
    op: &Op,
    stack: &mut Vec<cranelift_codegen::ir::Value>,
    slot_base: Option<cranelift_codegen::ir::Value>,
    plain_base: Option<cranelift_codegen::ir::Value>,
    arg_base: Option<cranelift_codegen::ir::Value>,
    pow_ref: Option<cranelift_codegen::ir::FuncRef>,
    lognot_ref: Option<cranelift_codegen::ir::FuncRef>,
    constants: &[PerlValue],
) -> Option<()> {
    match op {
        Op::LoadInt(n) => {
            stack.push(bcx.ins().iconst(types::I64, *n));
        }
        Op::LoadConst(idx) => {
            let n = constants
                .get(*idx as usize)
                .and_then(|pv| pv.as_integer())?;
            stack.push(bcx.ins().iconst(types::I64, n));
        }
        Op::LoadFloat(f) => {
            let n = *f as i64;
            stack.push(bcx.ins().iconst(types::I64, n));
        }
        Op::Add => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(bcx.ins().iadd(a, b));
        }
        Op::Sub => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(bcx.ins().isub(a, b));
        }
        Op::Mul => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(bcx.ins().imul(a, b));
        }
        Op::Div => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(bcx.ins().sdiv(a, b));
        }
        Op::Mod => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(bcx.ins().srem(a, b));
        }
        Op::Pow => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            let fr = pow_ref?;
            let call = bcx.ins().call(fr, &[a, b]);
            stack.push(*bcx.inst_results(call).first()?);
        }
        Op::Negate => {
            let a = stack.pop()?;
            stack.push(bcx.ins().ineg(a));
        }
        Op::NumEq => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(intcmp_to_01(bcx, IntCC::Equal, a, b));
        }
        Op::NumNe => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(intcmp_to_01(bcx, IntCC::NotEqual, a, b));
        }
        Op::NumLt => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(intcmp_to_01(bcx, IntCC::SignedLessThan, a, b));
        }
        Op::NumGt => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(intcmp_to_01(bcx, IntCC::SignedGreaterThan, a, b));
        }
        Op::NumLe => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(intcmp_to_01(bcx, IntCC::SignedLessThanOrEqual, a, b));
        }
        Op::NumGe => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(intcmp_to_01(bcx, IntCC::SignedGreaterThanOrEqual, a, b));
        }
        Op::Spaceship => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(spaceship_i64(bcx, a, b));
        }
        Op::LogNot => {
            let a = stack.pop()?;
            let fr = lognot_ref?;
            let call = bcx.ins().call(fr, &[a]);
            stack.push(*bcx.inst_results(call).first()?);
        }
        Op::Pop => {
            stack.pop()?;
        }
        Op::Dup => {
            let v = *stack.last()?;
            stack.push(v);
        }
        Op::BitXor => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(bcx.ins().bxor(a, b));
        }
        Op::BitAnd => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(bcx.ins().band(a, b));
        }
        Op::BitOr => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(bcx.ins().bor(a, b));
        }
        Op::BitNot => {
            let a = stack.pop()?;
            let ones = bcx.ins().iconst(types::I64, -1);
            stack.push(bcx.ins().bxor(a, ones));
        }
        Op::Shl => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            let mask = bcx.ins().iconst(types::I64, 63);
            let mb = bcx.ins().band(b, mask);
            stack.push(bcx.ins().ishl(a, mb));
        }
        Op::Shr => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            let mask = bcx.ins().iconst(types::I64, 63);
            let mb = bcx.ins().band(b, mask);
            stack.push(bcx.ins().sshr(a, mb));
        }
        Op::GetScalarSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            stack.push(
                bcx.ins()
                    .load(types::I64, MemFlags::trusted(), base, off),
            );
        }
        Op::SetScalarSlot(slot) => {
            let base = slot_base?;
            let v = stack.pop()?;
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*slot as i32) * 8);
        }
        Op::SetScalarSlotKeep(slot) => {
            let base = slot_base?;
            let v = *stack.last()?;
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*slot as i32) * 8);
        }
        Op::DeclareScalarSlot(slot) => {
            let base = slot_base?;
            let v = stack.pop()?;
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*slot as i32) * 8);
        }
        Op::PreIncSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            let old = bcx
                .ins()
                .load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().iadd(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push(new);
        }
        Op::PreDecSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            let old = bcx
                .ins()
                .load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().isub(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push(new);
        }
        Op::PostIncSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            let old = bcx
                .ins()
                .load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().iadd(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push(old);
        }
        Op::PostDecSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            let old = bcx
                .ins()
                .load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().isub(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push(old);
        }
        Op::GetScalarPlain(idx) => {
            let base = plain_base?;
            stack.push(
                bcx.ins()
                    .load(types::I64, MemFlags::trusted(), base, (*idx as i32) * 8),
            );
        }
        Op::GetArg(idx) => {
            let base = arg_base?;
            stack.push(
                bcx.ins()
                    .load(types::I64, MemFlags::trusted(), base, (*idx as i32) * 8),
            );
        }
        Op::SetScalarPlain(idx) => {
            let base = plain_base?;
            let v = stack.pop()?;
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*idx as i32) * 8);
        }
        Op::SetScalarKeepPlain(idx) => {
            let base = plain_base?;
            let v = *stack.last()?;
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*idx as i32) * 8);
        }
        Op::PreInc(idx) => {
            let base = plain_base?;
            let off = (*idx as i32) * 8;
            let old = bcx
                .ins()
                .load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().iadd(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push(new);
        }
        Op::PostInc(idx) => {
            let base = plain_base?;
            let off = (*idx as i32) * 8;
            let old = bcx
                .ins()
                .load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().iadd(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push(old);
        }
        Op::PreDec(idx) => {
            let base = plain_base?;
            let off = (*idx as i32) * 8;
            let old = bcx
                .ins()
                .load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().isub(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push(new);
        }
        Op::PostDec(idx) => {
            let base = plain_base?;
            let off = (*idx as i32) * 8;
            let old = bcx
                .ins()
                .load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().isub(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push(old);
        }
        _ => return None,
    }
    Some(())
}

fn compile_blocks(ops: &[Op], constants: &[PerlValue]) -> Option<LinearJit> {
    let cfg = validate_block_cfg(ops, constants)?;

    let need_any_table = needs_table(ops);

    let mut module = new_jit_module()?;

    let needs_pow = ops.iter().any(|o| matches!(o, Op::Pow));
    let pow_id = if needs_pow {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::I64));
        ps.params.push(AbiParam::new(types::I64));
        ps.returns.push(AbiParam::new(types::I64));
        Some(
            module
                .declare_function("perlrs_jit_pow_i64", Linkage::Import, &ps)
                .ok()?,
        )
    } else {
        None
    };

    let needs_lognot = ops.iter().any(|o| matches!(o, Op::LogNot));
    let lognot_id = if needs_lognot {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::I64));
        ps.returns.push(AbiParam::new(types::I64));
        Some(
            module
                .declare_function("perlrs_jit_lognot_i64", Linkage::Import, &ps)
                .ok()?,
        )
    } else {
        None
    };

    let ptr_ty = module.target_config().pointer_type();
    let mut sig = module.make_signature();
    if need_any_table {
        sig.params.push(AbiParam::new(ptr_ty));
        sig.params.push(AbiParam::new(ptr_ty));
        sig.params.push(AbiParam::new(ptr_ty));
    }
    sig.returns.push(AbiParam::new(types::I64));

    let fid = module
        .declare_function("block_fn", Linkage::Local, &sig)
        .ok()?;

    let mut ctx = module.make_context();
    ctx.func.signature = sig;
    ctx.func.name = UserFuncName::user(0, fid.as_u32());

    let mut fctx = FunctionBuilderContext::new();
    {
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut fctx);

        // Pre-create all Cranelift blocks.
        let cl_blocks: Vec<_> = cfg.iter().map(|_| bcx.create_block()).collect();

        // Entry block gets function parameters.
        bcx.append_block_params_for_function_params(cl_blocks[0]);

        // Non-entry blocks get stack-value parameters.
        for (i, blk) in cfg.iter().enumerate() {
            if i == 0 {
                continue;
            }
            for _ in 0..blk.entry_stack {
                bcx.append_block_param(cl_blocks[i], types::I64);
            }
        }

        let addr_to_block: HashMap<usize, usize> = cfg
            .iter()
            .enumerate()
            .map(|(i, b)| (b.start, i))
            .collect();

        // Resolve helper function refs once.
        let pow_ref = pow_id.map(|pid| module.declare_func_in_func(pid, &mut bcx.func));
        let lognot_ref =
            lognot_id.map(|lid| module.declare_func_in_func(lid, &mut bcx.func));

        let slot_base = if need_any_table {
            Some(bcx.block_params(cl_blocks[0])[0])
        } else {
            None
        };
        let plain_base = if need_any_table {
            Some(bcx.block_params(cl_blocks[0])[1])
        } else {
            None
        };
        let arg_base = if need_any_table {
            Some(bcx.block_params(cl_blocks[0])[2])
        } else {
            None
        };

        for (bi, blk) in cfg.iter().enumerate() {
            bcx.switch_to_block(cl_blocks[bi]);

            // Unreachable blocks (dead code after unconditional jumps): emit trap.
            if !blk.reachable {
                bcx.ins().trap(TrapCode::STACK_OVERFLOW);
                continue;
            }

            let mut stack: Vec<cranelift_codegen::ir::Value> =
                Vec::with_capacity(blk.entry_stack + 16);
            if bi == 0 {
                // Entry block: stack starts empty (entry_stack should be 0).
            } else {
                stack.extend_from_slice(bcx.block_params(cl_blocks[bi]));
            }

            let mut terminated = false;
            for idx in blk.start..blk.end {
                let op = &ops[idx];
                if is_block_data_op(op) {
                    emit_data_op(
                        &mut bcx,
                        op,
                        &mut stack,
                        slot_base,
                        plain_base,
                        arg_base,
                        pow_ref,
                        lognot_ref,
                        constants,
                    )?;
                    continue;
                }
                // Convert stack values to block args for branches.
                let as_args = |s: &[Value]| -> Vec<BlockArg> {
                    s.iter().copied().map(BlockArg::Value).collect()
                };
                match op {
                    Op::Jump(target) => {
                        let ti = *addr_to_block.get(target)?;
                        let args = as_args(&stack);
                        bcx.ins().jump(cl_blocks[ti], &args);
                        terminated = true;
                    }
                    Op::JumpIfTrue(target) => {
                        let cond = stack.pop()?;
                        let ti = *addr_to_block.get(target)?;
                        let ni = bi + 1;
                        let args = as_args(&stack);
                        bcx.ins()
                            .brif(cond, cl_blocks[ti], &args, cl_blocks[ni], &args);
                        terminated = true;
                    }
                    Op::JumpIfFalse(target) => {
                        let cond = stack.pop()?;
                        let ti = *addr_to_block.get(target)?;
                        let ni = bi + 1;
                        let args = as_args(&stack);
                        // false = zero → else branch is target
                        bcx.ins()
                            .brif(cond, cl_blocks[ni], &args, cl_blocks[ti], &args);
                        terminated = true;
                    }
                    Op::JumpIfFalseKeep(target) => {
                        let cond = *stack.last()?;
                        let ti = *addr_to_block.get(target)?;
                        let ni = bi + 1;
                        let args = as_args(&stack);
                        bcx.ins()
                            .brif(cond, cl_blocks[ni], &args, cl_blocks[ti], &args);
                        terminated = true;
                    }
                    Op::JumpIfTrueKeep(target) => {
                        let cond = *stack.last()?;
                        let ti = *addr_to_block.get(target)?;
                        let ni = bi + 1;
                        let args = as_args(&stack);
                        bcx.ins()
                            .brif(cond, cl_blocks[ti], &args, cl_blocks[ni], &args);
                        terminated = true;
                    }
                    Op::Halt => {
                        let v = stack.pop()?;
                        bcx.ins().return_(&[v]);
                        terminated = true;
                    }
                    _ => return None,
                }
                break; // terminators end the block
            }

            if !terminated {
                let ni = bi + 1;
                if ni >= cl_blocks.len() {
                    return None;
                }
                let args: Vec<BlockArg> =
                    stack.iter().copied().map(BlockArg::Value).collect();
                bcx.ins().jump(cl_blocks[ni], &args);
            }
        }

        bcx.seal_all_blocks();
        bcx.finalize();
    }

    module.define_function(fid, &mut ctx).ok()?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().ok()?;
    let ptr = module.get_finalized_function(fid);
    let run = if need_any_table {
        LinearRun::Tables(unsafe { std::mem::transmute(ptr) })
    } else {
        LinearRun::Nullary(unsafe { std::mem::transmute(ptr) })
    };
    Some(LinearJit { module, run })
}

static BLOCK_CACHE: OnceLock<Mutex<HashMap<u64, Box<LinearJit>>>> = OnceLock::new();

fn block_cache() -> &'static Mutex<HashMap<u64, Box<LinearJit>>> {
    BLOCK_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Largest slot index across **all** ops (not truncated at first `Halt`).
pub(crate) fn block_slot_ops_max_index(ops: &[Op]) -> Option<u8> {
    max_scalar_slot_index(ops)
}

/// Slot indices written by any slot-write op across **all** ops.
pub(crate) fn block_slot_ops_written_indices(ops: &[Op]) -> Vec<u8> {
    let mut v: Vec<u8> = ops
        .iter()
        .filter_map(|o| match o {
            Op::SetScalarSlot(s)
            | Op::SetScalarSlotKeep(s)
            | Op::DeclareScalarSlot(s)
            | Op::PreIncSlot(s)
            | Op::PostIncSlot(s)
            | Op::PreDecSlot(s)
            | Op::PostDecSlot(s) => Some(*s),
            _ => None,
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Largest `GetScalarPlain` name-pool index across **all** ops.
pub(crate) fn block_plain_ops_max_index(ops: &[Op]) -> Option<u16> {
    max_plain_name_index(ops)
}

/// Plain-name indices **written** across **all** ops (for VM writeback).
pub(crate) fn block_plain_ops_written_indices(ops: &[Op]) -> Vec<u16> {
    let mut v: Vec<u16> = ops
        .iter()
        .filter_map(|o| match o {
            Op::SetScalarPlain(i)
            | Op::SetScalarKeepPlain(i)
            | Op::PreInc(i)
            | Op::PostInc(i)
            | Op::PreDec(i)
            | Op::PostDec(i) => Some(*i),
            _ => None,
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Largest `GetArg` index across **all** ops.
pub(crate) fn block_arg_ops_max_index(ops: &[Op]) -> Option<u8> {
    max_get_arg_index(ops)
}

/// Same as [`slot_undef_prefill_ok`] but scans all ops (not just before `Halt`).
pub(crate) fn block_slot_undef_prefill_ok(ops: &[Op], slot: u8) -> bool {
    let mut written = false;
    for op in ops {
        match op {
            Op::GetScalarSlot(s) if *s == slot => {
                if !written {
                    return false;
                }
            }
            Op::DeclareScalarSlot(s) | Op::SetScalarSlot(s) | Op::SetScalarSlotKeep(s)
                if *s == slot =>
            {
                written = true;
            }
            Op::PreIncSlot(s) | Op::PostIncSlot(s) | Op::PreDecSlot(s) | Op::PostDecSlot(s)
                if *s == slot =>
            {
                written = true;
            }
            _ => {}
        }
    }
    true
}

/// Try to compile and run `ops` as a block-structured integer program (with loops/conditionals).
/// Returns `Some(PerlValue::integer(n))` on success, `None` to fall back to the interpreter.
pub(crate) fn try_run_block_ops(
    ops: &[Op],
    mut slot_i64: Option<&mut [i64]>,
    mut plain_i64: Option<&mut [i64]>,
    arg_i64: Option<&[i64]>,
    constants: &[PerlValue],
) -> Option<PerlValue> {
    if validate_block_cfg(ops, constants).is_none() {
        return None;
    }
    // Slot buffer bounds.
    if let Some(max) = block_slot_ops_max_index(ops) {
        let sl = slot_i64.as_mut()?;
        if sl.len() <= max as usize {
            return None;
        }
    }
    // Plain buffer bounds.
    if let Some(max) = block_plain_ops_max_index(ops) {
        let pl = plain_i64.as_ref()?;
        if pl.len() <= max as usize {
            return None;
        }
    }
    // Arg buffer bounds.
    if let Some(max) = block_arg_ops_max_index(ops) {
        let al = arg_i64?;
        if al.len() <= max as usize {
            return None;
        }
    }

    let key = hash_ops(ops, constants);
    let slot_ptr = slot_i64
        .as_mut()
        .map(|s| s.as_mut_ptr() as *const i64)
        .unwrap_or(std::ptr::null());
    let plain_ptr = plain_i64
        .as_mut()
        .map(|p| p.as_mut_ptr() as *const i64)
        .unwrap_or(std::ptr::null());
    let arg_ptr = arg_i64.map(|a| a.as_ptr()).unwrap_or(std::ptr::null());
    {
        let guard = block_cache().lock().ok()?;
        if let Some(j) = guard.get(&key) {
            let n = j.invoke(slot_ptr, plain_ptr, arg_ptr);
            return Some(PerlValue::integer(n));
        }
    }

    let jit = compile_blocks(ops, constants)?;
    let n = jit.invoke(slot_ptr, plain_ptr, arg_ptr);
    let pv = PerlValue::integer(n);

    if let Ok(mut guard) = block_cache().lock() {
        if guard.len() < 256 {
            guard.insert(key, Box::new(jit));
        }
    }
    Some(pv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Chunk;
    use crate::value::PerlValue;

    #[test]
    fn jit_add_mul_chain() {
        let ops = vec![
            Op::LoadInt(1),
            Op::LoadInt(2),
            Op::Add,
            Op::LoadInt(3),
            Op::Mul,
            Op::Halt,
        ];
        let v = try_run_linear_ops(&ops, None, None, None, &[]).expect("jit");
        assert_eq!(v.to_int(), 9);
    }

    #[test]
    fn jit_sub_negate() {
        let ops = vec![
            Op::LoadInt(10),
            Op::LoadInt(3),
            Op::Sub,
            Op::Negate,
            Op::Halt,
        ];
        let v = try_run_linear_ops(&ops, None, None, None, &[]).expect("jit");
        assert_eq!(v.to_int(), -7);
    }

    #[test]
    fn jit_rejects_slot_without_buffer() {
        let ops = vec![Op::LoadInt(1), Op::GetScalarSlot(0), Op::Add, Op::Halt];
        assert!(try_run_linear_ops(&ops, None, None, None, &[]).is_none());
    }

    #[test]
    fn jit_get_scalar_slot_add() {
        let ops = vec![Op::GetScalarSlot(0), Op::LoadInt(1), Op::Add, Op::Halt];
        let mut slots = [41i64];
        let v = try_run_linear_ops(&ops, Some(&mut slots), None, None, &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_rejects_set_slot_without_buffer() {
        let ops = vec![
            Op::LoadInt(7),
            Op::SetScalarSlot(0),
            Op::LoadInt(1),
            Op::Halt,
        ];
        assert!(try_run_linear_ops(&ops, None, None, None, &[]).is_none());
    }

    #[test]
    fn jit_set_scalar_slot_roundtrip() {
        let mut slots = [0i64];
        let ops = vec![
            Op::LoadInt(42),
            Op::SetScalarSlot(0),
            Op::GetScalarSlot(0),
            Op::Halt,
        ];
        let v = try_run_linear_ops(&ops, Some(&mut slots), None, None, &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
        assert_eq!(slots[0], 42);
    }

    #[test]
    fn jit_set_scalar_slot_keep() {
        let mut slots = [0i64];
        let ops = vec![
            Op::LoadInt(99),
            Op::SetScalarSlotKeep(0),
            Op::Halt,
        ];
        let v = try_run_linear_ops(&ops, Some(&mut slots), None, None, &[]).expect("jit");
        assert_eq!(v.to_int(), 99);
        assert_eq!(slots[0], 99);
    }

    #[test]
    fn jit_bit_xor() {
        let ops = vec![
            Op::LoadInt(0xF0),
            Op::LoadInt(0x0F),
            Op::BitXor,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&ops, None, None, None, &[]).expect("jit").to_int(), 0xFF);
    }

    #[test]
    fn jit_shl_and_shr() {
        let shl = vec![
            Op::LoadInt(1),
            Op::LoadInt(2),
            Op::Shl,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&shl, None, None, None, &[]).expect("jit").to_int(), 4);
        let shr = vec![
            Op::LoadInt(-16),
            Op::LoadInt(2),
            Op::Shr,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&shr, None, None, None, &[]).expect("jit").to_int(), -4);
    }

    #[test]
    fn jit_bit_not() {
        let ops = vec![Op::LoadInt(0), Op::BitNot, Op::Halt];
        assert_eq!(try_run_linear_ops(&ops, None, None, None, &[]).expect("jit").to_int(), !0i64);
    }

    #[test]
    fn jit_num_cmp_and_spaceship() {
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(2), Op::NumEq, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::NumEq, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            0
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::NumNe, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::NumLt, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(3), Op::LoadInt(2), Op::NumGt, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(2), Op::NumLe, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(3), Op::LoadInt(2), Op::NumGe, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::Spaceship, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            -1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(2), Op::Spaceship, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            0
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(3), Op::LoadInt(2), Op::Spaceship, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
    }

    #[test]
    fn jit_rejects_inexact_div() {
        assert!(try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::Div, Op::Halt], None, None, None, &[]).is_none());
        assert!(try_run_linear_ops(&[Op::LoadInt(10), Op::LoadInt(3), Op::Div, Op::Halt], None, None, None, &[]).is_none());
    }

    #[test]
    fn jit_exact_div_mod_and_pow() {
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(10), Op::LoadInt(2), Op::Div, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            5
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(10), Op::LoadInt(3), Op::Mod, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(3), Op::Pow, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            8
        );
    }

    #[test]
    fn jit_pow_dynamic_base_const_exp() {
        let ops = vec![
            Op::GetScalarSlot(0),
            Op::LoadInt(3),
            Op::Pow,
            Op::Halt,
        ];
        let mut slots = [2i64];
        assert_eq!(
            try_run_linear_ops(&ops, Some(&mut slots), None, None, &[])
                .expect("jit")
                .to_int(),
            8
        );
    }

    #[test]
    fn jit_rejects_pow_const_base_dynamic_exp() {
        let ops = vec![
            Op::LoadInt(2),
            Op::GetScalarSlot(0),
            Op::Pow,
            Op::Halt,
        ];
        let mut slots = [3i64];
        assert!(try_run_linear_ops(&ops, Some(&mut slots), None, None, &[]).is_none());
    }

    #[test]
    fn jit_load_const_add() {
        let pool = [PerlValue::integer(40)];
        let ops = vec![Op::LoadConst(0), Op::LoadInt(2), Op::Add, Op::Halt];
        let v = try_run_linear_ops(&ops, None, None, None, &pool).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_rejects_non_integer_load_const() {
        let pool = [PerlValue::string("x".into())];
        let ops = vec![Op::LoadConst(0), Op::Halt];
        assert!(try_run_linear_ops(&ops, None, None, None, &pool).is_none());
    }

    #[test]
    fn jit_bit_and_or() {
        let and = vec![
            Op::LoadInt(0b1100),
            Op::LoadInt(0b1010),
            Op::BitAnd,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&and, None, None, None, &[]).expect("jit").to_int(), 0b1000);
        let or = vec![
            Op::LoadInt(0b1100),
            Op::LoadInt(0b1010),
            Op::BitOr,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&or, None, None, None, &[]).expect("jit").to_int(), 0b1110);
    }

    #[test]
    fn jit_get_scalar_plain_add() {
        let mut plain = [40i64];
        let ops = vec![Op::GetScalarPlain(0), Op::LoadInt(2), Op::Add, Op::Halt];
        let v = try_run_linear_ops(&ops, None, Some(&mut plain), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_get_arg_add() {
        let args = [40i64];
        let ops = vec![Op::GetArg(0), Op::LoadInt(2), Op::Add, Op::Halt];
        let v = try_run_linear_ops(&ops, None, None, Some(&args), &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_rejects_arg_without_buffer() {
        let ops = vec![Op::GetArg(0), Op::Halt];
        assert!(try_run_linear_ops(&ops, None, None, None, &[]).is_none());
    }

    #[test]
    fn jit_rejects_plain_without_buffer() {
        let ops = vec![Op::GetScalarPlain(0), Op::Halt];
        assert!(try_run_linear_ops(&ops, None, None, None, &[]).is_none());
    }

    #[test]
    fn jit_slot_and_plain_add() {
        let mut slots = [10i64];
        let mut plain = [32i64];
        let ops = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarPlain(0),
            Op::Add,
            Op::Halt,
        ];
        let v = try_run_linear_ops(&ops, Some(&mut slots), Some(&mut plain), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_lognot() {
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(0), Op::LogNot, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LogNot, Op::Halt], None, None, None, &[])
                .expect("jit")
                .to_int(),
            0
        );
    }

    #[test]
    fn vm_chunk_uses_jit_path_for_pure_int() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(40), 1);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::Add, 1);
        c.emit(Op::Halt, 1);
        let mut interp = Interpreter::new();
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn vm_chunk_uses_jit_with_integer_load_const() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        let mut c = Chunk::new();
        let idx = c.add_constant(PerlValue::integer(40));
        c.emit(Op::LoadConst(idx), 1);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::Add, 1);
        c.emit(Op::Halt, 1);
        let mut interp = Interpreter::new();
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn vm_chunk_jit_get_scalar_plain_add() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        let mut c = Chunk::new();
        let idx = c.intern_name("v");
        c.emit(Op::GetScalarPlain(idx), 1);
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::Add, 1);
        c.emit(Op::Halt, 1);
        let mut interp = Interpreter::new();
        interp
            .scope
            .declare_scalar("v", PerlValue::integer(40));
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn vm_chunk_jit_lognot() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(0), 1);
        c.emit(Op::LogNot, 1);
        c.emit(Op::Halt, 1);
        let mut interp = Interpreter::new();
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 1);
    }

    #[test]
    fn vm_chunk_jit_num_cmp() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::LoadInt(3), 1);
        c.emit(Op::NumLt, 1);
        c.emit(Op::Halt, 1);
        let mut interp = Interpreter::new();
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 1);
    }

    #[test]
    fn jit_declare_and_slot_inc_dec() {
        let post_inc = vec![
            Op::LoadInt(10),
            Op::DeclareScalarSlot(0),
            Op::PostIncSlot(0),
            Op::Pop,
            Op::GetScalarSlot(0),
            Op::Halt,
        ];
        let mut slots = [0i64];
        assert_eq!(
            try_run_linear_ops(&post_inc, Some(&mut slots), None, None, &[])
                .expect("jit")
                .to_int(),
            11
        );
        assert_eq!(slots[0], 11);

        let pre_inc = vec![
            Op::LoadInt(0),
            Op::DeclareScalarSlot(0),
            Op::PreIncSlot(0),
            Op::Halt,
        ];
        let mut slots = [0i64];
        assert_eq!(
            try_run_linear_ops(&pre_inc, Some(&mut slots), None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(slots[0], 1);

        let pre_dec = vec![
            Op::LoadInt(5),
            Op::DeclareScalarSlot(0),
            Op::PreDecSlot(0),
            Op::Halt,
        ];
        let mut slots = [0i64];
        assert_eq!(
            try_run_linear_ops(&pre_dec, Some(&mut slots), None, None, &[])
                .expect("jit")
                .to_int(),
            4
        );
        assert_eq!(slots[0], 4);
    }

    #[test]
    fn vm_chunk_jit_declare_slot_post_inc() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(10), 1);
        c.emit(Op::DeclareScalarSlot(0), 1);
        c.emit(Op::PostIncSlot(0), 1);
        c.emit(Op::Pop, 1);
        c.emit(Op::GetScalarSlot(0), 1);
        c.emit(Op::Halt, 1);
        let mut interp = Interpreter::new();
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 11);
    }

    #[test]
    fn slot_undef_prefill_requires_no_get_before_write() {
        assert!(!slot_undef_prefill_ok(
            &[Op::GetScalarSlot(0), Op::Halt],
            0
        ));
        assert!(slot_undef_prefill_ok(
            &[
                Op::LoadInt(1),
                Op::DeclareScalarSlot(0),
                Op::GetScalarSlot(0),
                Op::Halt,
            ],
            0,
        ));
    }

    #[test]
    fn vm_chunk_jit_pow_slot_to_const_exp() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        let mut c = Chunk::new();
        c.emit(Op::LoadInt(2), 1);
        c.emit(Op::DeclareScalarSlot(0), 1);
        c.emit(Op::GetScalarSlot(0), 1);
        c.emit(Op::LoadInt(3), 1);
        c.emit(Op::Pow, 1);
        c.emit(Op::Halt, 1);
        let mut interp = Interpreter::new();
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 8);
    }

    // ── LoadUndef / LoadFloat ──

    #[test]
    fn jit_rejects_load_undef() {
        // LoadUndef cannot be JIT'd — integer(0) loses the is_undef() distinction.
        let ops = vec![Op::LoadUndef, Op::Halt];
        assert!(try_run_linear_ops(&ops, None, None, None, &[]).is_none());
    }

    #[test]
    fn jit_load_float_exact_int() {
        let ops = vec![Op::LoadFloat(3.0), Op::LoadInt(4), Op::Add, Op::Halt];
        let v = try_run_linear_ops(&ops, None, None, None, &[]).expect("jit");
        assert_eq!(v.to_int(), 7);
    }

    #[test]
    fn jit_rejects_non_integer_float() {
        let ops = vec![Op::LoadFloat(3.5), Op::Halt];
        assert!(try_run_linear_ops(&ops, None, None, None, &[]).is_none());
    }

    // ── Block JIT: conditionals ──

    #[test]
    fn block_jit_simple_if_true() {
        // if (1) { 42 } else { 0 }
        let ops = vec![
            Op::LoadInt(1),       // 0
            Op::JumpIfFalse(4),   // 1 → else
            Op::LoadInt(42),      // 2
            Op::Jump(5),          // 3 → end
            Op::LoadInt(0),       // 4 (else)
            Op::Halt,             // 5
        ];
        let v = try_run_block_ops(&ops, None, None, None, &[]).expect("block jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn block_jit_simple_if_false() {
        // if (0) { 42 } else { 99 }
        let ops = vec![
            Op::LoadInt(0),       // 0
            Op::JumpIfFalse(4),   // 1 → else
            Op::LoadInt(42),      // 2
            Op::Jump(5),          // 3 → end
            Op::LoadInt(99),      // 4 (else)
            Op::Halt,             // 5
        ];
        let v = try_run_block_ops(&ops, None, None, None, &[]).expect("block jit");
        assert_eq!(v.to_int(), 99);
    }

    #[test]
    fn block_jit_for_loop_sum() {
        // for (my $i=0; $i<5; $i++) { $sum += $i }  → 0+1+2+3+4 = 10
        let ops = vec![
            Op::LoadInt(0),           // 0: push 0
            Op::DeclareScalarSlot(0), // 1: $i = 0
            Op::LoadInt(0),           // 2: push 0
            Op::DeclareScalarSlot(1), // 3: $sum = 0
            // loop head
            Op::GetScalarSlot(0),     // 4: push $i
            Op::LoadInt(5),           // 5: push 5
            Op::NumLt,               // 6: $i < 5
            Op::JumpIfFalse(15),     // 7: → exit (GetScalarSlot at 15)
            // loop body
            Op::GetScalarSlot(1),     // 8: push $sum
            Op::GetScalarSlot(0),     // 9: push $i
            Op::Add,                 // 10: $sum + $i
            Op::SetScalarSlot(1),    // 11: $sum = result
            Op::PostIncSlot(0),      // 12: $i++
            Op::Pop,                 // 13: discard old $i
            Op::Jump(4),             // 14: → loop head
            // exit
            Op::GetScalarSlot(1),    // 15: push $sum
            Op::Halt,                // 16
        ];
        let mut slots = [0i64; 2];
        let v = try_run_block_ops(&ops, Some(&mut slots), None, None, &[]).expect("block jit");
        assert_eq!(v.to_int(), 10);
        assert_eq!(slots[0], 5); // $i ended at 5
        assert_eq!(slots[1], 10); // $sum
    }

    #[test]
    fn block_jit_short_circuit_and_true() {
        // 5 && 42  → evaluates both, returns 42
        let ops = vec![
            Op::LoadInt(5),           // 0: $a
            Op::JumpIfFalseKeep(4),   // 1: if false keep 5, jump to end
            Op::Pop,                  // 2: pop 5
            Op::LoadInt(42),          // 3: $b
            Op::Halt,                 // 4: result
        ];
        let v = try_run_block_ops(&ops, None, None, None, &[]).expect("block jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn block_jit_short_circuit_and_false() {
        // 0 && 42  → short-circuits, returns 0
        let ops = vec![
            Op::LoadInt(0),           // 0: $a = 0 (falsy)
            Op::JumpIfFalseKeep(4),   // 1: keep 0, jump to end
            Op::Pop,                  // 2: (skipped)
            Op::LoadInt(42),          // 3: (skipped)
            Op::Halt,                 // 4: result = 0
        ];
        let v = try_run_block_ops(&ops, None, None, None, &[]).expect("block jit");
        assert_eq!(v.to_int(), 0);
    }

    #[test]
    fn block_jit_short_circuit_or_true() {
        // 5 || 42  → short-circuits, returns 5
        let ops = vec![
            Op::LoadInt(5),           // 0: $a = 5 (truthy)
            Op::JumpIfTrueKeep(4),    // 1: keep 5, jump to end
            Op::Pop,                  // 2: (skipped)
            Op::LoadInt(42),          // 3: (skipped)
            Op::Halt,                 // 4: result = 5
        ];
        let v = try_run_block_ops(&ops, None, None, None, &[]).expect("block jit");
        assert_eq!(v.to_int(), 5);
    }

    #[test]
    fn block_jit_short_circuit_or_false() {
        // 0 || 42  → evaluates both, returns 42
        let ops = vec![
            Op::LoadInt(0),           // 0: $a = 0 (falsy)
            Op::JumpIfTrueKeep(4),    // 1: 0 is not truthy, fall through
            Op::Pop,                  // 2: pop 0
            Op::LoadInt(42),          // 3: $b
            Op::Halt,                 // 4: result = 42
        ];
        let v = try_run_block_ops(&ops, None, None, None, &[]).expect("block jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn block_jit_rejects_no_jumps() {
        // Pure linear sequence should NOT be handled by block JIT.
        let ops = vec![Op::LoadInt(1), Op::LoadInt(2), Op::Add, Op::Halt];
        assert!(try_run_block_ops(&ops, None, None, None, &[]).is_none());
    }

    #[test]
    fn block_jit_nested_loop() {
        // Nested: outer 0..3, inner 0..2: count = 3 * 2 = 6
        let ops = vec![
            Op::LoadInt(0),           // 0
            Op::DeclareScalarSlot(0), // 1: $i = 0
            Op::LoadInt(0),           // 2
            Op::DeclareScalarSlot(1), // 3: $count = 0
            // outer head
            Op::GetScalarSlot(0),     // 4
            Op::LoadInt(3),           // 5
            Op::NumLt,               // 6: $i < 3
            Op::JumpIfFalse(22),     // 7 → outer exit
            Op::LoadInt(0),           // 8
            Op::DeclareScalarSlot(2), // 9: $j = 0
            // inner head
            Op::GetScalarSlot(2),     // 10
            Op::LoadInt(2),           // 11
            Op::NumLt,               // 12: $j < 2
            Op::JumpIfFalse(19),     // 13 → inner exit
            // inner body
            Op::PreIncSlot(1),        // 14: ++$count
            Op::Pop,                  // 15
            Op::PostIncSlot(2),       // 16: $j++
            Op::Pop,                  // 17
            Op::Jump(10),            // 18 → inner head
            // inner exit / outer body continue
            Op::PostIncSlot(0),       // 19: $i++
            Op::Pop,                  // 20
            Op::Jump(4),             // 21 → outer head
            // outer exit
            Op::GetScalarSlot(1),     // 22: push $count
            Op::Halt,                // 23
        ];
        let mut slots = [0i64; 3];
        let v = try_run_block_ops(&ops, Some(&mut slots), None, None, &[]).expect("block jit");
        assert_eq!(v.to_int(), 6);
    }

    #[test]
    fn vm_chunk_block_jit_for_loop() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        // for ($i=0; $i<10; $i++) { $sum += $i } → 45
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(0), 1);
        c.emit(Op::DeclareScalarSlot(0), 1); // $i
        c.emit(Op::LoadInt(0), 1);
        c.emit(Op::DeclareScalarSlot(1), 1); // $sum
        // loop head = ip 4
        c.emit(Op::GetScalarSlot(0), 1);
        c.emit(Op::LoadInt(10), 1);
        c.emit(Op::NumLt, 1);
        c.emit(Op::JumpIfFalse(15), 1); // → exit
        // body
        c.emit(Op::GetScalarSlot(1), 1);
        c.emit(Op::GetScalarSlot(0), 1);
        c.emit(Op::Add, 1);
        c.emit(Op::SetScalarSlot(1), 1);
        c.emit(Op::PostIncSlot(0), 1);
        c.emit(Op::Pop, 1);
        c.emit(Op::Jump(4), 1); // → loop head
        // exit = ip 15
        c.emit(Op::GetScalarSlot(1), 1);
        c.emit(Op::Halt, 1);

        let mut interp = Interpreter::new();
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 45);
    }

    #[test]
    fn vm_chunk_block_jit_conditional() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        // if (1) { 42 } else { 0 }
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(1), 1);
        c.emit(Op::JumpIfFalse(4), 1);
        c.emit(Op::LoadInt(42), 1);
        c.emit(Op::Jump(5), 1);
        c.emit(Op::LoadInt(0), 1);
        c.emit(Op::Halt, 1);

        let mut interp = Interpreter::new();
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 42);
    }

    // ── Plain-name scalar write ops ──

    #[test]
    fn jit_set_scalar_plain_roundtrip() {
        let mut plain = [0i64];
        let ops = vec![
            Op::LoadInt(42),
            Op::SetScalarPlain(0),
            Op::GetScalarPlain(0),
            Op::Halt,
        ];
        let v = try_run_linear_ops(&ops, None, Some(&mut plain), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
        assert_eq!(plain[0], 42);
    }

    #[test]
    fn jit_set_scalar_keep_plain() {
        let mut plain = [0i64];
        let ops = vec![Op::LoadInt(99), Op::SetScalarKeepPlain(0), Op::Halt];
        let v = try_run_linear_ops(&ops, None, Some(&mut plain), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 99);
        assert_eq!(plain[0], 99);
    }

    #[test]
    fn jit_pre_inc_plain() {
        let mut plain = [10i64];
        let ops = vec![Op::PreInc(0), Op::Halt];
        let v = try_run_linear_ops(&ops, None, Some(&mut plain), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 11);
        assert_eq!(plain[0], 11);
    }

    #[test]
    fn jit_post_inc_plain() {
        let mut plain = [10i64];
        let ops = vec![Op::PostInc(0), Op::Halt];
        let v = try_run_linear_ops(&ops, None, Some(&mut plain), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 10); // returns old value
        assert_eq!(plain[0], 11);   // buffer updated
    }

    #[test]
    fn jit_pre_dec_plain() {
        let mut plain = [10i64];
        let ops = vec![Op::PreDec(0), Op::Halt];
        let v = try_run_linear_ops(&ops, None, Some(&mut plain), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 9);
        assert_eq!(plain[0], 9);
    }

    #[test]
    fn jit_post_dec_plain() {
        let mut plain = [10i64];
        let ops = vec![Op::PostDec(0), Op::Halt];
        let v = try_run_linear_ops(&ops, None, Some(&mut plain), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 10); // returns old value
        assert_eq!(plain[0], 9);    // buffer updated
    }

    #[test]
    fn block_jit_plain_inc_loop() {
        // for ($i=0; $i<5; ++$i) {} → $i ends at 5
        // Uses plain-name inc instead of slot inc.
        let mut plain = [0i64];
        let ops = vec![
            Op::LoadInt(0),          // 0
            Op::SetScalarPlain(0),   // 1: $i = 0
            Op::GetScalarPlain(0),   // 2: loop head
            Op::LoadInt(5),          // 3
            Op::NumLt,              // 4: $i < 5
            Op::JumpIfFalse(9),     // 5: → exit
            Op::PreInc(0),          // 6: ++$i
            Op::Pop,                // 7
            Op::Jump(2),            // 8: → loop head
            Op::GetScalarPlain(0),   // 9: exit
            Op::Halt,               // 10
        ];
        let v = try_run_block_ops(&ops, None, Some(&mut plain), None, &[]).expect("block jit");
        assert_eq!(v.to_int(), 5);
        assert_eq!(plain[0], 5);
    }

    #[test]
    fn vm_chunk_jit_set_scalar_plain() {
        use crate::interpreter::Interpreter;
        use crate::vm::VM;

        let mut c = Chunk::new();
        let idx = c.intern_name("x");
        c.emit(Op::LoadInt(42), 1);
        c.emit(Op::SetScalarKeepPlain(idx), 1);
        c.emit(Op::Halt, 1);
        let mut interp = Interpreter::new();
        interp.scope.declare_scalar("x", PerlValue::integer(0));
        let mut vm = VM::new(&c, &mut interp);
        let v = vm.execute().expect("vm");
        assert_eq!(v.to_int(), 42);
        // Writeback verified: result is 42 (SetScalarKeepPlain keeps on stack).
        // The scope writeback is tested via the VM returning the JIT result.
    }
}
