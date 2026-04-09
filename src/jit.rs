//! **Method JIT** (Cranelift): compiles linear **pure-integer** stack bytecode to native code.
//!
//! Eligible ops: [`Op::LoadInt`], [`Op::Add`]/[`Op::Sub`]/[`Op::Mul`], [`Op::Negate`],
//! [`Op::BitXor`], [`Op::BitNot`], [`Op::Shl`]/[`Op::Shr`] (shift amount masked to 6 bits),
//! [`Op::Div`] only when the VM would use the **exact integer quotient** (`a % b == 0`),
//! [`Op::Mod`] when the divisor is never dynamically zero (constant non-zero or folded stack),
//! [`Op::Pow`] when both operands constant-fold and `0 ≤ exponent ≤ 63` (VM integer `wrapping_pow`),
//! [`Op::Pop`], [`Op::Dup`], optional trailing [`Op::Halt`].
//!
//! Validation simulates a [`Cell`] stack so we only emit `sdiv`/`srem` when safe and only call
//! [`perlrs_jit_pow_i64`] when the VM’s integer fast path applies.
//!
//! Not JIT’d: inexact `Div`, `Mod` with unknown divisor, `Pow` outside `0..=63`, [`Op::BitAnd`]/[`Op::BitOr`]
//! (set ops), non-integer slot values, control flow, calls. [`Op::GetScalarSlot`] is JIT’d when every
//! referenced slot is materialized as `i64` via [`PerlValue::as_integer`]. Hot-loop tracing remains future work.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlags, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};

use crate::bytecode::Op;
use crate::value::PerlValue;

type LinearFn0 = unsafe extern "C" fn() -> i64;
type LinearFn1 = unsafe extern "C" fn(*const i64) -> i64;

enum LinearRun {
    Nullary(LinearFn0),
    Slots(LinearFn1),
}

struct LinearJit {
    #[allow(dead_code)]
    module: JITModule,
    run: LinearRun,
}

impl LinearJit {
    fn invoke(&self, slot_ptr: *const i64) -> i64 {
        match &self.run {
            LinearRun::Nullary(f) => unsafe { f() },
            LinearRun::Slots(f) => unsafe { f(slot_ptr) },
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

fn new_jit_module() -> Option<JITModule> {
    let isa_builder = cranelift_native::builder().ok()?;
    let isa = isa_builder.finish(isa_flags()).ok()?;
    let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
    builder.symbol(
        "perlrs_jit_pow_i64",
        perlrs_jit_pow_i64 as *const u8,
    );
    Some(JITModule::new(builder))
}

fn hash_ops(ops: &[Op]) -> u64 {
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

fn validate_linear(ops: &[Op]) -> bool {
    let seq = ops_before_halt(ops);
    if seq.is_empty() {
        return false;
    }
    let mut stack: Vec<Cell> = Vec::new();
    for op in seq {
        match op {
            Op::LoadInt(n) => stack.push(Cell::Const(*n)),
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
            _ => return false,
        }
        if stack.len() > 256 {
            return false;
        }
    }
    stack.len() == 1
}

fn compile_linear(ops: &[Op]) -> Option<LinearJit> {
    let seq = ops_before_halt(ops);
    if !validate_linear(ops) {
        return None;
    }
    let needs_slots = seq.iter().any(|o| matches!(o, Op::GetScalarSlot(_)));
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

    let mut sig = module.make_signature();
    if needs_slots {
        sig.params
            .push(AbiParam::new(module.target_config().pointer_type()));
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

        let slot_base = if needs_slots {
            Some(bcx.block_params(entry)[0])
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
    let run = if needs_slots {
        LinearRun::Slots(unsafe { std::mem::transmute(ptr) })
    } else {
        LinearRun::Nullary(unsafe { std::mem::transmute(ptr) })
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

/// If `ops` is a supported pure-int linear sequence, run compiled code and return the result.
/// Otherwise returns [`None`] (VM should interpret as usual).
///
/// When the sequence contains [`Op::GetScalarSlot`], pass `Some` slice whose length is
/// `max(slot_index) + 1` and whose `i` entries are the slot values as `i64` (same as
/// [`PerlValue::as_integer`]).
pub(crate) fn try_run_linear_ops(ops: &[Op], slot_i64: Option<&[i64]>) -> Option<PerlValue> {
    if !validate_linear(ops) {
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

    let key = hash_ops(ops);
    let slot_ptr = slot_i64.map(|s| s.as_ptr()).unwrap_or(std::ptr::null());
    {
        let guard = cache().lock().ok()?;
        if let Some(j) = guard.get(&key) {
            let n = j.invoke(slot_ptr);
            return Some(PerlValue::integer(n));
        }
    }

    let jit = compile_linear(ops)?;
    let n = jit.invoke(slot_ptr);
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
        let v = try_run_linear_ops(&ops, None).expect("jit");
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
        let v = try_run_linear_ops(&ops, None).expect("jit");
        assert_eq!(v.to_int(), -7);
    }

    #[test]
    fn jit_rejects_slot_without_buffer() {
        let ops = vec![Op::LoadInt(1), Op::GetScalarSlot(0), Op::Add, Op::Halt];
        assert!(try_run_linear_ops(&ops, None).is_none());
    }

    #[test]
    fn jit_get_scalar_slot_add() {
        let ops = vec![Op::GetScalarSlot(0), Op::LoadInt(1), Op::Add, Op::Halt];
        let slots = [41i64];
        let v = try_run_linear_ops(&ops, Some(&slots)).expect("jit");
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
        assert_eq!(try_run_linear_ops(&ops, None).expect("jit").to_int(), 0xFF);
    }

    #[test]
    fn jit_shl_and_shr() {
        let shl = vec![
            Op::LoadInt(1),
            Op::LoadInt(2),
            Op::Shl,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&shl, None).expect("jit").to_int(), 4);
        let shr = vec![
            Op::LoadInt(-16),
            Op::LoadInt(2),
            Op::Shr,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&shr, None).expect("jit").to_int(), -4);
    }

    #[test]
    fn jit_bit_not() {
        let ops = vec![Op::LoadInt(0), Op::BitNot, Op::Halt];
        assert_eq!(try_run_linear_ops(&ops, None).expect("jit").to_int(), !0i64);
    }

    #[test]
    fn jit_rejects_inexact_div() {
        assert!(try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::Div, Op::Halt], None).is_none());
        assert!(try_run_linear_ops(&[Op::LoadInt(10), Op::LoadInt(3), Op::Div, Op::Halt], None).is_none());
    }

    #[test]
    fn jit_exact_div_mod_and_pow() {
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(10), Op::LoadInt(2), Op::Div, Op::Halt], None)
                .expect("jit")
                .to_int(),
            5
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(10), Op::LoadInt(3), Op::Mod, Op::Halt], None)
                .expect("jit")
                .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(3), Op::Pow, Op::Halt], None)
                .expect("jit")
                .to_int(),
            8
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
}
