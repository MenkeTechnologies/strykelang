//! **Method JIT** (Cranelift): compiles linear **pure-integer** stack bytecode to native code.
//!
//! Eligible ops: [`Op::LoadInt`], [`Op::Add`]/[`Op::Sub`]/[`Op::Mul`], [`Op::Negate`], [`Op::LogNot`],
//! [`Op::NumEq`] / [`Op::NumNe`] / [`Op::NumLt`] / [`Op::NumGt`] / [`Op::NumLe`] / [`Op::NumGe`],
//! [`Op::Spaceship`],
//! [`Op::BitXor`], [`Op::BitNot`], [`Op::Shl`]/[`Op::Shr`] (shift amount masked to 6 bits),
//! [`Op::Div`] only when the VM would use the **exact integer quotient** (`a % b == 0`),
//! [`Op::Mod`] when the divisor is never dynamically zero (constant non-zero or folded stack),
//! [`Op::Pow`] when both operands constant-fold and `0 ≤ exponent ≤ 63` (VM integer `wrapping_pow`),
//! [`Op::Pop`], [`Op::Dup`], optional trailing [`Op::Halt`], [`Op::LoadConst`] when the pool entry is
//! an integer ([`PerlValue::as_integer`]), [`Op::BitAnd`]/[`Op::BitOr`] (same integer path as the VM
//! when operands are not set values).
//!
//! Validation simulates a [`Cell`] stack so we only emit `sdiv`/`srem` when safe and only call
//! [`perlrs_jit_pow_i64`] when the VM’s integer fast path applies.
//!
//! Not JIT’d: inexact `Div`, `Mod` with unknown divisor, `Pow` outside `0..=63`, non-integer
//! [`Op::LoadConst`] pool entries, `BitAnd`/`BitOr` on set values (not expressible in this int-only
//! simulation), non-integer slot/plain values, control flow, calls. [`Op::GetScalarSlot`] /
//! [`Op::GetScalarPlain`] are JIT’d when every referenced index materializes as `i64` via
//! [`PerlValue::as_integer`]. [`Op::LogNot`] matches [`PerlValue::integer`] truth via
//! [`perlrs_jit_lognot_i64`]. [`Op::NumEq`] / [`Op::NumNe`] / [`Op::NumLt`] / [`Op::NumGt`] /
//! [`Op::NumLe`] / [`Op::NumGe`] / [`Op::Spaceship`] follow the VM integer compare path. Hot-loop
//! tracing remains future work.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlags, UserFuncName, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};

use crate::bytecode::Op;
use crate::value::PerlValue;

type LinearFn0 = unsafe extern "C" fn() -> i64;
type LinearFn1 = unsafe extern "C" fn(*const i64) -> i64;
type LinearFn2 = unsafe extern "C" fn(*const i64, *const i64) -> i64;

enum LinearRun {
    Nullary(LinearFn0),
    SlotsOnly(LinearFn1),
    PlainOnly(LinearFn1),
    SlotsPlain(LinearFn2),
}

struct LinearJit {
    #[allow(dead_code)]
    module: JITModule,
    run: LinearRun,
}

impl LinearJit {
    fn invoke(&self, slots: *const i64, plain: *const i64) -> i64 {
        match &self.run {
            LinearRun::Nullary(f) => unsafe { f() },
            LinearRun::SlotsOnly(f) => unsafe { f(slots) },
            LinearRun::PlainOnly(f) => unsafe { f(plain) },
            LinearRun::SlotsPlain(f) => unsafe { f(slots, plain) },
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
            Op::GetScalarSlot(_) => stack.push(Cell::Dyn),
            Op::GetScalarPlain(_) => stack.push(Cell::Dyn),
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
            _ => return false,
        }
        if stack.len() > 256 {
            return false;
        }
    }
    stack.len() == 1
}

fn compile_linear(ops: &[Op], constants: &[PerlValue]) -> Option<LinearJit> {
    let seq = ops_before_halt(ops);
    if !validate_linear(ops, constants) {
        return None;
    }
    let needs_slots = seq.iter().any(|o| matches!(o, Op::GetScalarSlot(_)));
    let needs_plain = seq.iter().any(|o| matches!(o, Op::GetScalarPlain(_)));
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
    if needs_slots {
        sig.params.push(AbiParam::new(ptr_ty));
    }
    if needs_plain {
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

        let mut p = 0usize;
        let slot_base = if needs_slots {
            let v = bcx.block_params(entry)[p];
            p += 1;
            Some(v)
        } else {
            None
        };
        let plain_base = if needs_plain {
            let v = bcx.block_params(entry)[p];
            Some(v)
        } else {
            None
        };

        let mut stack: Vec<cranelift_codegen::ir::Value> = Vec::with_capacity(32);
        for op in seq {
            match op {
                Op::LoadInt(n) => {
                    let v = bcx.ins().iconst(types::I64, *n);
                    stack.push(v);
                }
                Op::LoadConst(idx) => {
                    let n = constants
                        .get(*idx as usize)
                        .and_then(|pv| pv.as_integer())?;
                    let v = bcx.ins().iconst(types::I64, n);
                    stack.push(v);
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
                    let pid = pow_id?;
                    let fr = module.declare_func_in_func(pid, &mut bcx.func);
                    let call = bcx.ins().call(fr, &[a, b]);
                    let v = *bcx.inst_results(call).first()?;
                    stack.push(v);
                }
                Op::Negate => {
                    let a = stack.pop()?;
                    stack.push(bcx.ins().ineg(a));
                }
                Op::NumEq => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(intcmp_to_01(&mut bcx, IntCC::Equal, a, b));
                }
                Op::NumNe => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(intcmp_to_01(&mut bcx, IntCC::NotEqual, a, b));
                }
                Op::NumLt => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(intcmp_to_01(&mut bcx, IntCC::SignedLessThan, a, b));
                }
                Op::NumGt => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(intcmp_to_01(&mut bcx, IntCC::SignedGreaterThan, a, b));
                }
                Op::NumLe => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(intcmp_to_01(
                        &mut bcx,
                        IntCC::SignedLessThanOrEqual,
                        a,
                        b,
                    ));
                }
                Op::NumGe => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(intcmp_to_01(
                        &mut bcx,
                        IntCC::SignedGreaterThanOrEqual,
                        a,
                        b,
                    ));
                }
                Op::Spaceship => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(spaceship_i64(&mut bcx, a, b));
                }
                Op::LogNot => {
                    let a = stack.pop()?;
                    let lid = lognot_id?;
                    let fr = module.declare_func_in_func(lid, &mut bcx.func);
                    let call = bcx.ins().call(fr, &[a]);
                    let v = *bcx.inst_results(call).first()?;
                    stack.push(v);
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
                    let v = bcx
                        .ins()
                        .load(types::I64, MemFlags::trusted(), base, off);
                    stack.push(v);
                }
                Op::GetScalarPlain(idx) => {
                    let base = plain_base?;
                    let off = (*idx as i32) * 8;
                    let v = bcx
                        .ins()
                        .load(types::I64, MemFlags::trusted(), base, off);
                    stack.push(v);
                }
                _ => return None,
            }
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
    let run = match (needs_slots, needs_plain) {
        (false, false) => LinearRun::Nullary(unsafe { std::mem::transmute(ptr) }),
        (true, false) => LinearRun::SlotsOnly(unsafe { std::mem::transmute(ptr) }),
        (false, true) => LinearRun::PlainOnly(unsafe { std::mem::transmute(ptr) }),
        (true, true) => LinearRun::SlotsPlain(unsafe { std::mem::transmute(ptr) }),
    };
    Some(LinearJit { module, run })
}

static LINEAR_CACHE: OnceLock<Mutex<HashMap<u64, Box<LinearJit>>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<u64, Box<LinearJit>>> {
    LINEAR_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn linear_needs_slots(seq: &[Op]) -> bool {
    seq.iter().any(|o| matches!(o, Op::GetScalarSlot(_)))
}

fn linear_needs_plain(seq: &[Op]) -> bool {
    seq.iter().any(|o| matches!(o, Op::GetScalarPlain(_)))
}

fn max_scalar_slot_index(seq: &[Op]) -> Option<u8> {
    seq.iter()
        .filter_map(|o| match o {
            Op::GetScalarSlot(s) => Some(*s),
            _ => None,
        })
        .max()
}

/// Largest `GetScalarSlot` index in `ops` before [`Op::Halt`], if any (for dense `i64` slot tables).
pub(crate) fn linear_slot_ops_max_index(ops: &[Op]) -> Option<u8> {
    max_scalar_slot_index(ops_before_halt(ops))
}

fn max_plain_name_index(seq: &[Op]) -> Option<u16> {
    seq.iter()
        .filter_map(|o| match o {
            Op::GetScalarPlain(i) => Some(*i),
            _ => None,
        })
        .max()
}

/// Largest `GetScalarPlain` name-pool index in `ops` before [`Op::Halt`], if any.
pub(crate) fn linear_plain_ops_max_index(ops: &[Op]) -> Option<u16> {
    max_plain_name_index(ops_before_halt(ops))
}

/// If `ops` is a supported pure-int linear sequence, run compiled code and return the result.
/// Otherwise returns [`None`] (VM should interpret as usual).
///
/// When the sequence contains [`Op::GetScalarSlot`], pass `Some` slice whose length is
/// `max(slot_index) + 1` and whose `i` entries are the slot values as `i64` (same as
/// [`PerlValue::as_integer`]).
///
/// When it contains [`Op::GetScalarPlain`], pass `Some` slice whose length is
/// `max(name_index) + 1` with `PerlValue::as_integer` of `scope.get_scalar` for each name index.
///
/// `constants` must be the chunk’s constant pool: [`Op::LoadConst`] is only JIT’d when
/// `constants[idx]` is materializable as `i64` via [`PerlValue::as_integer`].
pub(crate) fn try_run_linear_ops(
    ops: &[Op],
    slot_i64: Option<&[i64]>,
    plain_i64: Option<&[i64]>,
    constants: &[PerlValue],
) -> Option<PerlValue> {
    if !validate_linear(ops, constants) {
        return None;
    }
    let seq = ops_before_halt(ops);
    if linear_needs_slots(seq) {
        let max = max_scalar_slot_index(seq)?;
        let sl = slot_i64?;
        if sl.len() <= max as usize {
            return None;
        }
    }
    if linear_needs_plain(seq) {
        let max = max_plain_name_index(seq)?;
        let pl = plain_i64?;
        if pl.len() <= max as usize {
            return None;
        }
    }

    let key = hash_ops(ops, constants);
    let slot_ptr = slot_i64.map(|s| s.as_ptr()).unwrap_or(std::ptr::null());
    let plain_ptr = plain_i64.map(|p| p.as_ptr()).unwrap_or(std::ptr::null());
    {
        let guard = cache().lock().ok()?;
        if let Some(j) = guard.get(&key) {
            let n = j.invoke(slot_ptr, plain_ptr);
            return Some(PerlValue::integer(n));
        }
    }

    let jit = compile_linear(ops, constants)?;
    let n = jit.invoke(slot_ptr, plain_ptr);
    let pv = PerlValue::integer(n);

    if let Ok(mut guard) = cache().lock() {
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
        let v = try_run_linear_ops(&ops, None, None, &[]).expect("jit");
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
        let v = try_run_linear_ops(&ops, None, None, &[]).expect("jit");
        assert_eq!(v.to_int(), -7);
    }

    #[test]
    fn jit_rejects_slot_without_buffer() {
        let ops = vec![Op::LoadInt(1), Op::GetScalarSlot(0), Op::Add, Op::Halt];
        assert!(try_run_linear_ops(&ops, None, None, &[]).is_none());
    }

    #[test]
    fn jit_get_scalar_slot_add() {
        let ops = vec![Op::GetScalarSlot(0), Op::LoadInt(1), Op::Add, Op::Halt];
        let slots = [41i64];
        let v = try_run_linear_ops(&ops, Some(&slots), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_bit_xor() {
        let ops = vec![
            Op::LoadInt(0xF0),
            Op::LoadInt(0x0F),
            Op::BitXor,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&ops, None, None, &[]).expect("jit").to_int(), 0xFF);
    }

    #[test]
    fn jit_shl_and_shr() {
        let shl = vec![
            Op::LoadInt(1),
            Op::LoadInt(2),
            Op::Shl,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&shl, None, None, &[]).expect("jit").to_int(), 4);
        let shr = vec![
            Op::LoadInt(-16),
            Op::LoadInt(2),
            Op::Shr,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&shr, None, None, &[]).expect("jit").to_int(), -4);
    }

    #[test]
    fn jit_bit_not() {
        let ops = vec![Op::LoadInt(0), Op::BitNot, Op::Halt];
        assert_eq!(try_run_linear_ops(&ops, None, None, &[]).expect("jit").to_int(), !0i64);
    }

    #[test]
    fn jit_num_cmp_and_spaceship() {
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(2), Op::NumEq, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::NumEq, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            0
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::NumNe, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::NumLt, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(3), Op::LoadInt(2), Op::NumGt, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(2), Op::NumLe, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(3), Op::LoadInt(2), Op::NumGe, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::Spaceship, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            -1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(2), Op::Spaceship, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            0
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(3), Op::LoadInt(2), Op::Spaceship, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
    }

    #[test]
    fn jit_rejects_inexact_div() {
        assert!(try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::Div, Op::Halt], None, None, &[]).is_none());
        assert!(try_run_linear_ops(&[Op::LoadInt(10), Op::LoadInt(3), Op::Div, Op::Halt], None, None, &[]).is_none());
    }

    #[test]
    fn jit_exact_div_mod_and_pow() {
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(10), Op::LoadInt(2), Op::Div, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            5
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(10), Op::LoadInt(3), Op::Mod, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(3), Op::Pow, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            8
        );
    }

    #[test]
    fn jit_load_const_add() {
        let pool = [PerlValue::integer(40)];
        let ops = vec![Op::LoadConst(0), Op::LoadInt(2), Op::Add, Op::Halt];
        let v = try_run_linear_ops(&ops, None, None, &pool).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_rejects_non_integer_load_const() {
        let pool = [PerlValue::string("x".into())];
        let ops = vec![Op::LoadConst(0), Op::Halt];
        assert!(try_run_linear_ops(&ops, None, None, &pool).is_none());
    }

    #[test]
    fn jit_bit_and_or() {
        let and = vec![
            Op::LoadInt(0b1100),
            Op::LoadInt(0b1010),
            Op::BitAnd,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&and, None, None, &[]).expect("jit").to_int(), 0b1000);
        let or = vec![
            Op::LoadInt(0b1100),
            Op::LoadInt(0b1010),
            Op::BitOr,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&or, None, None, &[]).expect("jit").to_int(), 0b1110);
    }

    #[test]
    fn jit_get_scalar_plain_add() {
        let plain = [40i64];
        let ops = vec![Op::GetScalarPlain(0), Op::LoadInt(2), Op::Add, Op::Halt];
        let v = try_run_linear_ops(&ops, None, Some(&plain), &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_rejects_plain_without_buffer() {
        let ops = vec![Op::GetScalarPlain(0), Op::Halt];
        assert!(try_run_linear_ops(&ops, None, None, &[]).is_none());
    }

    #[test]
    fn jit_slot_and_plain_add() {
        let slots = [10i64];
        let plain = [32i64];
        let ops = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarPlain(0),
            Op::Add,
            Op::Halt,
        ];
        let v = try_run_linear_ops(&ops, Some(&slots), Some(&plain), &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_lognot() {
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(0), Op::LogNot, Op::Halt], None, None, &[])
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(1), Op::LogNot, Op::Halt], None, None, &[])
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
}
