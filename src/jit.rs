//! **Method JIT** (Cranelift): compiles linear **pure-integer** stack bytecode to native code.
//!
//! Eligible ops: [`Op::LoadInt`], [`Op::Add`]/[`Op::Sub`]/[`Op::Mul`], [`Op::Negate`],
//! [`Op::BitXor`], [`Op::BitNot`], [`Op::Shl`]/[`Op::Shr`] (shift amount masked to 6 bits, matching
//! Rust/`i64::wrapping_shl` / `wrapping_shr`), [`Op::Pop`], [`Op::Dup`], optional trailing
//! [`Op::Halt`]. Matches the VM’s integer fast path (wrapping `i64` for `+`/`-`/`*`; bitwise ops
//! match [`Op::BitXor`] / [`Op::BitNot`] / shift ops in `vm.rs`).
//!
//! Not JIT’d: [`Op::Div`]/[`Op::Mod`] (runtime divide-by-zero and Perl’s mixed int/float division),
//! [`Op::Pow`], [`Op::BitAnd`]/[`Op::BitOr`] (set operations), slots, control flow, calls — those
//! keep using the interpreter. Hot-loop tracing and [`Op::GetScalarSlot`] in native code remain
//! future work.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};

use crate::bytecode::Op;
use crate::value::PerlValue;

type LinearFn = unsafe extern "C" fn() -> i64;

struct LinearJit {
    #[allow(dead_code)]
    module: JITModule,
    run: LinearFn,
}

fn isa_flags() -> settings::Flags {
    let mut flag_builder = settings::builder();
    let _ = flag_builder.set("use_colocated_libcalls", "false");
    let _ = flag_builder.set("is_pic", "false");
    settings::Flags::new(flag_builder)
}

fn new_jit_module() -> Option<JITModule> {
    let isa_builder = cranelift_native::builder().ok()?;
    let isa = isa_builder.finish(isa_flags()).ok()?;
    Some(JITModule::new(JITBuilder::with_isa(
        isa,
        default_libcall_names(),
    )))
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

fn validate_linear(ops: &[Op]) -> bool {
    let seq = ops_before_halt(ops);
    if seq.is_empty() {
        return false;
    }
    let mut depth: i32 = 0;
    for op in seq {
        match op {
            Op::LoadInt(_) => depth += 1,
            Op::Add | Op::Sub | Op::Mul => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }
            Op::Negate => {
                if depth < 1 {
                    return false;
                }
            }
            Op::Pop => {
                if depth < 1 {
                    return false;
                }
                depth -= 1;
            }
            Op::Dup => {
                if depth < 1 {
                    return false;
                }
                depth += 1;
            }
            Op::BitXor | Op::Shl | Op::Shr => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }
            Op::BitNot => {
                if depth < 1 {
                    return false;
                }
            }
            _ => return false,
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 1
}

fn compile_linear(ops: &[Op]) -> Option<LinearJit> {
    let seq = ops_before_halt(ops);
    if !validate_linear(ops) {
        return None;
    }
    let mut module = new_jit_module()?;
    let mut sig = module.make_signature();
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
        bcx.switch_to_block(entry);

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
    let run: LinearFn = unsafe { std::mem::transmute(ptr) };
    Some(LinearJit { module, run })
}

static LINEAR_CACHE: OnceLock<Mutex<HashMap<u64, Box<LinearJit>>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<u64, Box<LinearJit>>> {
    LINEAR_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// If `ops` is a supported pure-int linear sequence, run compiled code and return the result.
/// Otherwise returns [`None`] (VM should interpret as usual).
pub(crate) fn try_run_linear_ops(ops: &[Op]) -> Option<PerlValue> {
    if !validate_linear(ops) {
        return None;
    }
    let key = hash_ops(ops);
    {
        let guard = cache().lock().ok()?;
        if let Some(j) = guard.get(&key) {
            let n = unsafe { (j.run)() };
            return Some(PerlValue::integer(n));
        }
    }

    let jit = compile_linear(ops)?;
    let n = unsafe { (jit.run)() };
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
        let v = try_run_linear_ops(&ops).expect("jit");
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
        let v = try_run_linear_ops(&ops).expect("jit");
        assert_eq!(v.to_int(), -7);
    }

    #[test]
    fn jit_rejects_slot_op() {
        let ops = vec![Op::LoadInt(1), Op::GetScalarSlot(0), Op::Add, Op::Halt];
        assert!(try_run_linear_ops(&ops).is_none());
    }

    #[test]
    fn jit_bit_xor() {
        let ops = vec![
            Op::LoadInt(0xF0),
            Op::LoadInt(0x0F),
            Op::BitXor,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&ops).expect("jit").to_int(), 0xFF);
    }

    #[test]
    fn jit_shl_and_shr() {
        let shl = vec![
            Op::LoadInt(1),
            Op::LoadInt(2),
            Op::Shl,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&shl).expect("jit").to_int(), 4);
        let shr = vec![
            Op::LoadInt(-16),
            Op::LoadInt(2),
            Op::Shr,
            Op::Halt,
        ];
        assert_eq!(try_run_linear_ops(&shr).expect("jit").to_int(), -4);
    }

    #[test]
    fn jit_bit_not() {
        let ops = vec![Op::LoadInt(0), Op::BitNot, Op::Halt];
        assert_eq!(try_run_linear_ops(&ops).expect("jit").to_int(), !0i64);
    }

    #[test]
    fn jit_rejects_div_mod_pow() {
        assert!(try_run_linear_ops(&[Op::LoadInt(1), Op::LoadInt(2), Op::Div, Op::Halt]).is_none());
        assert!(try_run_linear_ops(&[Op::LoadInt(5), Op::LoadInt(2), Op::Mod, Op::Halt]).is_none());
        assert!(try_run_linear_ops(&[Op::LoadInt(2), Op::LoadInt(3), Op::Pow, Op::Halt]).is_none());
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
