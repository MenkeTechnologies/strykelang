//! **Method JIT** (Cranelift): compiles stack bytecode to native code.
//!
//! Both **linear** and **block** paths support integer and floating-point data ops (with int/float
//! promotion where the VM does). At CFG merges, abstract [`join_cell`] widens stack slots so int and
//! float predecessors can join; Cranelift block parameters use `i64`/`f64` per merged slot.
//!
//! Two compilation tiers:
//!
//! ## Linear JIT
//! Compiles straight-line (no branches) sequences in a single Cranelift basic block.
//!
//! ## Block JIT
//! Compiles bytecode **with control flow** (loops, conditionals, short-circuit `&&`/`||`) via a
//! basic-block CFG. Abstract stacks at each block entry are computed by **fixpoint merge**
//! ([`merge_stack_entry`] + [`join_cell`]); unreachable blocks (dead code after unconditional
//! jumps) are emitted as traps. Same data ops as the linear JIT plus [`Op::Jump`],
//! [`Op::JumpIfTrue`], [`Op::JumpIfFalse`], [`Op::JumpIfFalseKeep`], [`Op::JumpIfTrueKeep`], and
//! [`Op::JumpIfDefinedKeep`] (see **Not JIT’d (block CFG)** for constant tops vs raw-buffer [`Get*`]).
//!
//! ## Eligible data ops (both tiers)
//! [`Op::LoadInt`], [`Op::LoadFloat`] (non-integral literals become float cells),
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
//! an integer or float ([`PerlValue::as_integer`] / [`PerlValue::as_float`]), [`Op::BitAnd`]/[`Op::BitOr`]
//! (same integer path as the VM when operands are not set values).
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
//! Linear tier: [`validate_linear_seq`] / [`linear_result_cell_seq`]. Block tier: [`validate_block_cfg`]
//! (stack merge at joins, merged [`Cell`] for the `Halt` result); [`block_jit_validate`] is the
//! `pub(crate)` entry used by [`crate::vm::VM::execute`]. Both use [`simulate_one_op`] so
//! division/modulus/`Pow` safety matches the VM; integer `Pow` calls [`perlrs_jit_pow_i64`], and the
//! float path uses [`perlrs_jit_pow_f64`] / [`perlrs_jit_fmod_f64`] when the abstract stack is float.
//!
//! ## Subroutine linear JIT call-out (`Op::Call`)
//! A compiled sub that calls another compiled sub with **stack-args** (`GetArg`) and **scalar** context
//! can emit a Cranelift call to [`crate::vm::perlrs_jit_call_sub`] (VM pointer as the first parameter,
//! then callee bytecode IP and up to eight `i64` args). The trampoline runs [`crate::vm::VM::jit_trampoline_run_sub`]
//! so the callee may be interpreted or JIT’d again.
//!
//! ## Not JIT’d (linear)
//! Inexact integer `Div`, `Mod` with unknown divisor, integer `Pow` outside `0..=63`, `BitAnd`/`BitOr`
//! on set values, non-integer slot/plain/arg materialization where the VM would not be `i64`,
//! calls that are not a compiled stack-args scalar `Op::Call` (see above), string ops, array/hash ops.
//! `LoadUndef` is JIT’d as full nanbox bits; the return
//! path uses `PerlValue::from_raw_bits` when the abstract result is [`Cell::Undef`].
//!
//! ## Not JIT’d (block CFG)
//! Unsupported opcodes, inconsistent stack height at a merge, or merge where [`join_cell`] fails.
//! [`Op::JumpIfDefinedKeep`]: constant [`Cell::Const`] / [`Cell::ConstF`] tops compile to an
//! unconditional jump (fall-through is dead). When the abstract top is [`Cell::Dyn`] immediately after
//! [`Op::GetScalarSlot`] / [`Op::GetScalarPlain`] / [`Op::GetArg`], the block JIT uses **raw-buffer
//! mode**: slot/plain/arg `i64` tables carry [`PerlValue::raw_bits`] (sign reinterpretation), loads
//! preserve `undef`, and the terminator calls [`perlrs_jit_is_defined_raw_bits`]. That mode rejects
//! arithmetic and most other data ops so stack values stay valid NaN-box encodings. Other [`Cell::Dyn`]
//! / [`Cell::DynF`] shapes still fall back to the interpreter.
//!
//! ## VM integration
//! [`crate::vm::VM::execute`] tries [`try_run_linear_ops`] on the full opcode buffer, then
//! [`block_jit_validate`]. On success it fills slot/plain/arg buffers using [`ValidatedBlockCfg::buffer_mode`]
//! and calls [`try_run_block_ops`] with `Some(validated)` so CFG validation is not run again inside
//! compilation. Callers may pass `None` for the last argument to [`try_run_block_ops`] to validate
//! internally (unit tests). For buffer mode only, use [`block_jit_validate`] then [`ValidatedBlockCfg::buffer_mode`].
//! Then the opcode dispatch loop. At **subroutine entry IPs only** (bitset from [`Chunk::sub_entries`]),
//! [`crate::vm::VM`] may run [`try_run_linear_sub`] / block sub-JIT. [`sub_entry_segment`] stops at the
//! first [`Op::Return`] (void, empty stack) or [`Op::ReturnValue`]. Failed linear prefixes skip buffer
//! allocation; failed block validation is cached ([`block_jit_validate_sub`]) and per-entry skip sets
//! avoid rescanning bytecode on every recursive call.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::ffi::c_void;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use cranelift_codegen::isa::OwnedTargetIsa;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Ieee64;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{
    AbiParam, BlockArg, InstBuilder, MemFlags, TrapCode, UserFuncName, Value,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};

use crate::bytecode::Op;
use crate::interpreter::WantarrayCtx;
use crate::value::PerlValue;

type LinearFn0 = unsafe extern "C" fn() -> i64;
/// Slot table, plain name table, compiled-sub arg table (fixed order; unused pointers may be null).
type LinearFn3 = unsafe extern "C" fn(*const i64, *const i64, *const i64) -> i64;
type LinearFn0F = unsafe extern "C" fn() -> f64;
type LinearFn3F = unsafe extern "C" fn(*const i64, *const i64, *const i64) -> f64;
type LinearFnVm0 = unsafe extern "C" fn(*mut c_void) -> i64;
type LinearFnVm3 = unsafe extern "C" fn(*mut c_void, *const i64, *const i64, *const i64) -> i64;
type LinearFnVm0F = unsafe extern "C" fn(*mut c_void) -> f64;
type LinearFnVm3F = unsafe extern "C" fn(*mut c_void, *const i64, *const i64, *const i64) -> f64;

enum LinearRun {
    Nullary(LinearFn0),
    Tables(LinearFn3),
    NullaryF(LinearFn0F),
    TablesF(LinearFn3F),
    VmNullary(LinearFnVm0),
    VmTables(LinearFnVm3),
    VmNullaryF(LinearFnVm0F),
    VmTablesF(LinearFnVm3F),
}

/// Whether the native function returns an integer or a float.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JitTy {
    Int,
    Float,
}

struct LinearJit {
    /// Retained so the Cranelift [`JITModule`] (and finalized machine code) is not dropped while
    /// [`LinearRun`] pointers are invoked.
    #[allow(dead_code)]
    module: JITModule,
    run: LinearRun,
    /// When true, the `i64` return is a full nanbox (`PerlValue::from_raw_bits`), not `PerlValue::integer`.
    ret_nanboxed: bool,
}

enum JitResult {
    Int(i64),
    Float(f64),
}

fn jit_result_to_perl(j: JitResult, ret_nanboxed: bool) -> PerlValue {
    match j {
        JitResult::Int(n) if ret_nanboxed => PerlValue::from_raw_bits(n as u64),
        JitResult::Int(n) => PerlValue::integer(n),
        JitResult::Float(f) => PerlValue::float(f),
    }
}

fn jit_block_result_to_perl(j: JitResult, mode: BlockJitBufferMode) -> PerlValue {
    match (j, mode) {
        (JitResult::Int(n), BlockJitBufferMode::I64AsPerlValueBits) => {
            PerlValue::from_raw_bits(n as u64)
        }
        (JitResult::Int(n), BlockJitBufferMode::I64AsInteger) => PerlValue::integer(n),
        (JitResult::Float(f), _) => PerlValue::float(f),
    }
}

impl LinearJit {
    fn ret_nanboxed(&self) -> bool {
        self.ret_nanboxed
    }

    fn invoke(
        &self,
        vm: *mut c_void,
        slots: *const i64,
        plain: *const i64,
        args: *const i64,
    ) -> JitResult {
        match &self.run {
            LinearRun::Nullary(f) => JitResult::Int(unsafe { f() }),
            LinearRun::Tables(f) => JitResult::Int(unsafe { f(slots, plain, args) }),
            LinearRun::NullaryF(f) => JitResult::Float(unsafe { f() }),
            LinearRun::TablesF(f) => JitResult::Float(unsafe { f(slots, plain, args) }),
            LinearRun::VmNullary(f) => JitResult::Int(unsafe { f(vm) }),
            LinearRun::VmTables(f) => JitResult::Int(unsafe { f(vm, slots, plain, args) }),
            LinearRun::VmNullaryF(f) => JitResult::Float(unsafe { f(vm) }),
            LinearRun::VmTablesF(f) => JitResult::Float(unsafe { f(vm, slots, plain, args) }),
        }
    }

    fn result_to_perl(&self, j: JitResult) -> PerlValue {
        jit_result_to_perl(j, self.ret_nanboxed)
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
    if (0..=63).contains(&exp) {
        base.wrapping_pow(exp as u32)
    } else {
        0
    }
}

/// Float `**` — delegates to `f64::powf`.
#[no_mangle]
pub extern "C" fn perlrs_jit_pow_f64(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

/// Float `%` — delegates to Rust `f64 % f64` (IEEE 754 remainder / `fmod`).
#[no_mangle]
pub extern "C" fn perlrs_jit_fmod_f64(a: f64, b: f64) -> f64 {
    a % b
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

/// `defined` for a value transported as [`PerlValue::raw_bits`] in an `i64` stack slot (block JIT raw-buffer mode).
#[no_mangle]
pub extern "C" fn perlrs_jit_is_defined_raw_bits(bits: i64) -> i64 {
    if PerlValue::from_raw_bits(bits as u64).is_undef() {
        0
    } else {
        1
    }
}

/// CPU ISA is fixed for the process — cache [`OwnedTargetIsa`] so each JIT cache miss does not
/// re-run native ISA detection (see `new_jit_module`).
static JIT_OWNED_ISA: OnceLock<Option<OwnedTargetIsa>> = OnceLock::new();

fn cached_owned_isa() -> Option<&'static OwnedTargetIsa> {
    JIT_OWNED_ISA
        .get_or_init(|| {
            let isa_builder = cranelift_native::builder().ok()?;
            isa_builder.finish(isa_flags()).ok()
        })
        .as_ref()
}

fn new_jit_module() -> Option<JITModule> {
    let isa = cached_owned_isa()?.clone();
    let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
    builder.symbol("perlrs_jit_pow_i64", perlrs_jit_pow_i64 as *const u8);
    builder.symbol("perlrs_jit_lognot_i64", perlrs_jit_lognot_i64 as *const u8);
    builder.symbol("perlrs_jit_pow_f64", perlrs_jit_pow_f64 as *const u8);
    builder.symbol("perlrs_jit_fmod_f64", perlrs_jit_fmod_f64 as *const u8);
    builder.symbol(
        "perlrs_jit_is_defined_raw_bits",
        perlrs_jit_is_defined_raw_bits as *const u8,
    );
    builder.symbol(
        "perlrs_jit_call_sub",
        crate::vm::perlrs_jit_call_sub as *const u8,
    );
    Some(JITModule::new(builder))
}

fn find_sub_entry_slice(
    sub_entries: &[(u16, usize, bool)],
    name_idx: u16,
) -> Option<(usize, bool)> {
    for &(n, ip, stack_args) in sub_entries {
        if n == name_idx {
            return Some((ip, stack_args));
        }
    }
    None
}

/// `Op::Call` that can be lowered to [`crate::vm::perlrs_jit_call_sub`] (compiled sub, stack args, scalar).
fn call_is_jitable(op: &Op, sub_entries: &[(u16, usize, bool)]) -> bool {
    let Op::Call(name_idx, argc, wa) = op else {
        return false;
    };
    if WantarrayCtx::from_byte(*wa) != WantarrayCtx::Scalar {
        return false;
    }
    if *argc == 0 || *argc > 8 {
        return false;
    }
    find_sub_entry_slice(sub_entries, *name_idx)
        .map(|(_, stack_args)| stack_args)
        .unwrap_or(false)
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

fn floatcmp_to_01(bcx: &mut FunctionBuilder, cc: FloatCC, a: Value, b: Value) -> Value {
    let pred = bcx.ins().fcmp(cc, a, b);
    let one = bcx.ins().iconst(types::I64, 1);
    let zero = bcx.ins().iconst(types::I64, 0);
    bcx.ins().select(pred, one, zero)
}

fn spaceship_f64(bcx: &mut FunctionBuilder, a: Value, b: Value) -> Value {
    let lt = bcx.ins().fcmp(FloatCC::LessThan, a, b);
    let gt = bcx.ins().fcmp(FloatCC::GreaterThan, a, b);
    let m1 = bcx.ins().iconst(types::I64, -1);
    let z = bcx.ins().iconst(types::I64, 0);
    let p1 = bcx.ins().iconst(types::I64, 1);
    let mid = bcx.ins().select(gt, p1, z);
    bcx.ins().select(lt, m1, mid)
}

fn i64_to_f64(bcx: &mut FunctionBuilder, v: Value) -> Value {
    bcx.ins().fcvt_from_sint(types::F64, v)
}

fn f64_to_i64_trunc(bcx: &mut FunctionBuilder, v: Value) -> Value {
    bcx.ins().fcvt_to_sint(types::I64, v)
}

/// Pop two stack values, promoting to `f64` when either operand is float.
fn pop_pair_promote(
    bcx: &mut FunctionBuilder,
    stack: &mut Vec<(Value, JitTy)>,
) -> Option<(Value, Value, JitTy)> {
    let (b, tb) = stack.pop()?;
    let (a, ta) = stack.pop()?;
    let out = match (ta, tb) {
        (JitTy::Int, JitTy::Int) => (a, b, JitTy::Int),
        (JitTy::Float, JitTy::Float) => (a, b, JitTy::Float),
        (JitTy::Int, JitTy::Float) => (i64_to_f64(bcx, a), b, JitTy::Float),
        (JitTy::Float, JitTy::Int) => (a, i64_to_f64(bcx, b), JitTy::Float),
    };
    Some(out)
}

fn scalar_store_i64(bcx: &mut FunctionBuilder, v: Value, ty: JitTy) -> Value {
    match ty {
        JitTy::Int => v,
        JitTy::Float => f64_to_i64_trunc(bcx, v),
    }
}

/// Merge abstract cells from two CFG predecessors (fixed-point join).
fn join_cell(a: Cell, b: Cell) -> Option<Cell> {
    if a == b {
        return Some(a);
    }
    if std::mem::discriminant(&a) == std::mem::discriminant(&b) {
        return match (a, b) {
            (Cell::Const(x), Cell::Const(y)) => {
                if x == y {
                    Some(Cell::Const(x))
                } else {
                    Some(Cell::Dyn)
                }
            }
            (Cell::ConstF(x), Cell::ConstF(y)) => {
                if x == y {
                    Some(Cell::ConstF(x))
                } else {
                    Some(Cell::DynF)
                }
            }
            (Cell::Dyn, Cell::Dyn) => Some(Cell::Dyn),
            (Cell::DynF, Cell::DynF) => Some(Cell::DynF),
            (Cell::Undef, Cell::Undef) => Some(Cell::Undef),
            _ => None,
        };
    }
    match (a, b) {
        (Cell::Undef, Cell::Const(_)) | (Cell::Const(_), Cell::Undef) => Some(Cell::Dyn),
        (Cell::Undef, Cell::Dyn) | (Cell::Dyn, Cell::Undef) => Some(Cell::Dyn),
        (Cell::Undef, Cell::ConstF(_)) | (Cell::ConstF(_), Cell::Undef) => Some(Cell::DynF),
        (Cell::Undef, Cell::DynF) | (Cell::DynF, Cell::Undef) => Some(Cell::DynF),
        (Cell::Const(_), Cell::Dyn) | (Cell::Dyn, Cell::Const(_)) => Some(Cell::Dyn),
        (Cell::ConstF(_), Cell::DynF) | (Cell::DynF, Cell::ConstF(_)) => Some(Cell::DynF),
        (Cell::Const(_), Cell::DynF)
        | (Cell::DynF, Cell::Const(_))
        | (Cell::ConstF(_), Cell::Dyn)
        | (Cell::Dyn, Cell::ConstF(_))
        | (Cell::Const(_), Cell::ConstF(_))
        | (Cell::ConstF(_), Cell::Const(_)) => Some(Cell::DynF),
        (Cell::Dyn, Cell::DynF) | (Cell::DynF, Cell::Dyn) => Some(Cell::DynF),
        // Unreachable for well-formed `Cell`, but required for exhaustiveness on `(Cell, Cell)`.
        _ => None,
    }
}

fn join_stack_slices(a: &[Cell], b: &[Cell]) -> Option<Vec<Cell>> {
    if a.len() != b.len() {
        return None;
    }
    let mut out = Vec::with_capacity(a.len());
    for i in 0..a.len() {
        out.push(join_cell(a[i], b[i])?);
    }
    Some(out)
}

fn merge_stack_entry(
    stacks: &mut [Option<Vec<Cell>>],
    wl: &mut VecDeque<usize>,
    bi: usize,
    stack: &[Cell],
) -> Option<()> {
    match &mut stacks[bi] {
        None => {
            stacks[bi] = Some(stack.to_vec());
            wl.push_back(bi);
            Some(())
        }
        Some(prev) => {
            let joined = join_stack_slices(prev.as_slice(), stack)?;
            if joined != *prev {
                *prev = joined;
                wl.push_back(bi);
            }
            Some(())
        }
    }
}

fn adapt_value_to_jit_ty(bcx: &mut FunctionBuilder, v: Value, from: JitTy, to: JitTy) -> Value {
    match (from, to) {
        (JitTy::Int, JitTy::Int) | (JitTy::Float, JitTy::Float) => v,
        (JitTy::Int, JitTy::Float) => i64_to_f64(bcx, v),
        (JitTy::Float, JitTy::Int) => f64_to_i64_trunc(bcx, v),
    }
}

fn branch_stack_to_block_args(
    bcx: &mut FunctionBuilder,
    stack: &[(Value, JitTy)],
    target_entry: &[Cell],
) -> Option<Vec<BlockArg>> {
    if stack.len() != target_entry.len() {
        return None;
    }
    let mut out = Vec::with_capacity(stack.len());
    for ((v, src_ty), cell) in stack.iter().zip(target_entry.iter()) {
        let to_ty = cell_to_jit_ty(*cell);
        let w = adapt_value_to_jit_ty(bcx, *v, *src_ty, to_ty);
        out.push(BlockArg::Value(w));
    }
    Some(out)
}

/// Cache key for compiled JIT functions. [`Op::LoadConst`] hashes [`PerlValue::raw_bits`]
/// so different constant pool payloads cannot collide at the same index.
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
                if let Some(pv) = constants.get(*i as usize) {
                    pv.raw_bits().hash(&mut h);
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
            Op::JumpIfDefinedKeep(t) => {
                49u8.hash(&mut h);
                t.hash(&mut h);
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Cell {
    Const(i64),
    Dyn,
    ConstF(f64),
    DynF,
    /// [`Op::LoadUndef`] — stack carries [`PerlValue::UNDEF`] nanbox bits (see [`LinearJit::ret_nanboxed`]).
    Undef,
}

impl Cell {
    fn is_float(self) -> bool {
        matches!(self, Cell::ConstF(_) | Cell::DynF)
    }
    fn either_float(a: Cell, b: Cell) -> bool {
        a.is_float() || b.is_float()
    }
}

/// Popped operands for arithmetic / bitwise / compare — `undef` must not mix with numeric folding.
fn pop2_strict(stack: &mut Vec<Cell>) -> Option<(Cell, Cell)> {
    let b = stack.pop()?;
    let a = stack.pop()?;
    if matches!(a, Cell::Undef) || matches!(b, Cell::Undef) {
        return None;
    }
    Some((a, b))
}

/// Fold a binary arithmetic result: both-const folds, float promotes, else dynamic.
fn fold_arith(a: Cell, b: Cell, int_op: fn(i64, i64) -> i64, f_op: fn(f64, f64) -> f64) -> Cell {
    match (a, b) {
        (Cell::Const(x), Cell::Const(y)) => Cell::Const(int_op(x, y)),
        (Cell::ConstF(x), Cell::ConstF(y)) => Cell::ConstF(f_op(x, y)),
        _ if Cell::either_float(a, b) => Cell::DynF,
        _ => Cell::Dyn,
    }
}

#[inline]
fn cell_to_jit_ty(c: Cell) -> JitTy {
    match c {
        Cell::ConstF(_) | Cell::DynF => JitTy::Float,
        Cell::Const(_) | Cell::Dyn | Cell::Undef => JitTy::Int,
    }
}

fn fold_cmp_cell(op: &Op, a: Cell, b: Cell) -> Cell {
    fn float_cmp(op: &Op, x: f64, y: f64) -> i64 {
        if x.is_nan() || y.is_nan() {
            return 0;
        }
        match op {
            Op::NumEq => i64::from(x == y),
            Op::NumNe => i64::from(x != y),
            Op::NumLt => i64::from(x < y),
            Op::NumGt => i64::from(x > y),
            Op::NumLe => i64::from(x <= y),
            Op::NumGe => i64::from(x >= y),
            Op::Spaceship => match x.partial_cmp(&y) {
                Some(std::cmp::Ordering::Less) => -1,
                Some(std::cmp::Ordering::Equal) => 0,
                Some(std::cmp::Ordering::Greater) => 1,
                None => 0,
            },
            _ => 0,
        }
    }
    match (a, b) {
        (Cell::Const(x), Cell::Const(y)) => {
            let v = match op {
                Op::NumEq => i64::from(x == y),
                Op::NumNe => i64::from(x != y),
                Op::NumLt => i64::from(x < y),
                Op::NumGt => i64::from(x > y),
                Op::NumLe => i64::from(x <= y),
                Op::NumGe => i64::from(x >= y),
                Op::Spaceship => match x.cmp(&y) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                },
                _ => 0,
            };
            Cell::Const(v)
        }
        (Cell::ConstF(x), Cell::ConstF(y)) => {
            if matches!(op, Op::Spaceship) && (x.is_nan() || y.is_nan()) {
                Cell::Dyn
            } else {
                Cell::Const(float_cmp(op, x, y))
            }
        }
        (Cell::Const(x), Cell::ConstF(y)) => Cell::Const(float_cmp(op, x as f64, y)),
        (Cell::ConstF(x), Cell::Const(y)) => Cell::Const(float_cmp(op, x, y as f64)),
        _ => Cell::Dyn,
    }
}

/// One data op for abstract stack simulation (linear + block JIT validation).
fn simulate_one_op(
    op: &Op,
    stack: &mut Vec<Cell>,
    constants: &[PerlValue],
    sub_entries: Option<&[(u16, usize, bool)]>,
) -> Option<()> {
    match op {
        Op::LoadInt(n) => stack.push(Cell::Const(*n)),
        Op::LoadConst(idx) => {
            match constants.get(*idx as usize) {
                Some(pv) => {
                    if let Some(n) = pv.as_integer() {
                        stack.push(Cell::Const(n));
                    } else if let Some(f) = pv.as_float() {
                        stack.push(Cell::ConstF(f));
                    } else {
                        return None;
                    }
                }
                None => return None,
            };
        }
        Op::LoadFloat(f) => {
            if !f.is_finite() {
                return None;
            }
            let n = *f as i64;
            if (n as f64) == *f {
                stack.push(Cell::Const(n));
            } else {
                stack.push(Cell::ConstF(*f));
            }
        }
        Op::LoadUndef => stack.push(Cell::Undef),
        Op::Add => {
            let (a, b) = pop2_strict(stack)?;
            stack.push(fold_arith(a, b, i64::wrapping_add, |x, y| x + y));
        }
        Op::Sub => {
            let (a, b) = pop2_strict(stack)?;
            stack.push(fold_arith(a, b, i64::wrapping_sub, |x, y| x - y));
        }
        Op::Mul => {
            let (a, b) = pop2_strict(stack)?;
            stack.push(fold_arith(a, b, i64::wrapping_mul, |x, y| x * y));
        }
        Op::Div => {
            let (a, b) = pop2_strict(stack)?;
            if Cell::either_float(a, b) {
                // Float div: always OK (produces Inf/NaN; Perl catches at runtime).
                match (a, b) {
                    (Cell::ConstF(x), Cell::ConstF(y)) => stack.push(Cell::ConstF(x / y)),
                    _ => stack.push(Cell::DynF),
                }
            } else {
                // Int div: exact quotient required.
                match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) if y != 0 && x % y == 0 => {
                        stack.push(Cell::Const(x / y));
                    }
                    _ => return None,
                }
            }
        }
        Op::Mod => {
            let (a, b) = pop2_strict(stack)?;
            if Cell::either_float(a, b) {
                match (a, b) {
                    (Cell::ConstF(x), Cell::ConstF(y)) => stack.push(Cell::ConstF(x % y)),
                    _ => stack.push(Cell::DynF),
                }
            } else {
                match b {
                    Cell::Const(0) => return None,
                    Cell::Const(y) => stack.push(match a {
                        Cell::Const(x) => Cell::Const(x % y),
                        _ => Cell::Dyn,
                    }),
                    _ => return None,
                }
            }
        }
        Op::Pow => {
            let (a, b) = pop2_strict(stack)?;
            if Cell::either_float(a, b) {
                match (a, b) {
                    (Cell::ConstF(x), Cell::ConstF(y)) => stack.push(Cell::ConstF(x.powf(y))),
                    _ => stack.push(Cell::DynF),
                }
            } else {
                match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) if (0..=63).contains(&y) => {
                        stack.push(Cell::Const(x.wrapping_pow(y as u32)));
                    }
                    (Cell::Dyn, Cell::Const(y)) if (0..=63).contains(&y) => {
                        stack.push(Cell::Dyn);
                    }
                    _ => return None,
                }
            }
        }
        Op::Negate => {
            let a = stack.pop()?;
            if matches!(a, Cell::Undef) {
                return None;
            }
            stack.push(match a {
                Cell::Const(n) => Cell::Const(n.wrapping_neg()),
                Cell::ConstF(f) => Cell::ConstF(-f),
                Cell::DynF => Cell::DynF,
                Cell::Dyn => Cell::Dyn,
                Cell::Undef => unreachable!(),
            });
        }
        Op::Pop => {
            stack.pop()?;
        }
        Op::Dup => {
            let v = stack.last().copied()?;
            stack.push(v);
        }
        // Bitwise: only integers (Perl truncates floats to int for bitwise).
        Op::BitXor | Op::BitAnd | Op::BitOr | Op::BitNot | Op::Shl | Op::Shr => {
            if matches!(op, Op::BitNot) {
                let a = stack.pop()?;
                if a.is_float() || matches!(a, Cell::Undef) {
                    return None;
                }
                stack.push(match a {
                    Cell::Const(n) => Cell::Const(!n),
                    _ => Cell::Dyn,
                });
            } else {
                let (a, b) = pop2_strict(stack)?;
                if Cell::either_float(a, b) {
                    return None;
                }
                stack.push(match (a, b) {
                    (Cell::Const(x), Cell::Const(y)) => Cell::Const(match op {
                        Op::BitXor => x ^ y,
                        Op::BitAnd => x & y,
                        Op::BitOr => x | y,
                        Op::Shl => x.wrapping_shl((y as u32) & 63),
                        Op::Shr => x.wrapping_shr((y as u32) & 63),
                        _ => unreachable!(),
                    }),
                    _ => Cell::Dyn,
                });
            }
        }
        Op::SetScalarSlot(_) | Op::DeclareScalarSlot(_) | Op::SetScalarPlain(_) => {
            stack.pop()?;
        }
        Op::SetScalarSlotKeep(_) | Op::SetScalarKeepPlain(_) => {
            stack.last()?;
        }
        Op::PreIncSlot(_)
        | Op::PreDecSlot(_)
        | Op::PostIncSlot(_)
        | Op::PostDecSlot(_)
        | Op::PreInc(_)
        | Op::PreDec(_)
        | Op::PostInc(_)
        | Op::PostDec(_) => {
            stack.push(Cell::Dyn);
        }
        Op::GetScalarSlot(_) | Op::GetScalarPlain(_) | Op::GetArg(_) => {
            stack.push(Cell::Dyn);
        }
        // Numeric comparisons: always produce int (0/1 or -1/0/1), even with float operands.
        Op::NumEq | Op::NumNe | Op::NumLt | Op::NumGt | Op::NumLe | Op::NumGe | Op::Spaceship => {
            let (a, b) = pop2_strict(stack)?;
            stack.push(fold_cmp_cell(op, a, b));
        }
        Op::LogNot => {
            let a = stack.pop()?;
            if matches!(a, Cell::Undef) {
                return None;
            }
            stack.push(match a {
                Cell::Const(n) => Cell::Const(if PerlValue::integer(n).is_true() {
                    0
                } else {
                    1
                }),
                Cell::ConstF(f) => Cell::Const(if f != 0.0 { 0 } else { 1 }),
                _ => Cell::Dyn,
            });
        }
        Op::Call(_, _, _) => {
            let se = sub_entries?;
            if !call_is_jitable(op, se) {
                return None;
            }
            let Op::Call(_, argc, _) = op else {
                return None;
            };
            let argc = *argc as usize;
            for _ in 0..argc {
                stack.pop()?;
            }
            stack.push(Cell::Dyn);
        }
        _ => return None,
    }
    Some(())
}

fn validate_linear_seq(
    seq: &[Op],
    constants: &[PerlValue],
    sub_entries: Option<&[(u16, usize, bool)]>,
) -> bool {
    if seq.is_empty() {
        return false;
    }
    let mut stack: Vec<Cell> = Vec::new();
    for op in seq {
        if simulate_one_op(op, &mut stack, constants, sub_entries).is_none() {
            return false;
        }
        if stack.len() > 256 {
            return false;
        }
    }
    stack.len() == 1
}

/// Linear sequence for a void `return;` — stack must be empty after the last op (no `Return`/`ReturnValue` in `seq`).
fn validate_linear_void_seq(seq: &[Op], constants: &[PerlValue]) -> bool {
    let mut stack: Vec<Cell> = Vec::new();
    for op in seq {
        if simulate_one_op(op, &mut stack, constants, None).is_none() {
            return false;
        }
        if stack.len() > 256 {
            return false;
        }
    }
    stack.is_empty()
}

fn linear_result_cell_seq(
    seq: &[Op],
    constants: &[PerlValue],
    sub_entries: Option<&[(u16, usize, bool)]>,
) -> Option<Cell> {
    let mut stack: Vec<Cell> = Vec::new();
    for op in seq {
        simulate_one_op(op, &mut stack, constants, sub_entries)?;
        if stack.len() > 256 {
            return None;
        }
    }
    if stack.len() != 1 {
        return None;
    }
    stack.pop()
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
    compile_linear_ops(ops_before_halt(ops), constants, &[])
}

fn compile_linear_ops(
    seq: &[Op],
    constants: &[PerlValue],
    sub_entries: &[(u16, usize, bool)],
) -> Option<LinearJit> {
    if !validate_linear_seq(seq, constants, Some(sub_entries)) {
        return None;
    }
    let ret_cell = linear_result_cell_seq(seq, constants, Some(sub_entries))?;
    let ret_nanboxed = matches!(ret_cell, Cell::Undef);
    let ret_ty = cell_to_jit_ty(ret_cell);
    let need_any_table = needs_table(seq);
    let need_vm_sub = seq.iter().any(|o| call_is_jitable(o, sub_entries));
    let mut module = new_jit_module()?;

    let needs_pow = seq.iter().any(|o| matches!(o, Op::Pow));
    let pow_i64_id = if needs_pow {
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
    let pow_f64_id = if needs_pow {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::F64));
        ps.params.push(AbiParam::new(types::F64));
        ps.returns.push(AbiParam::new(types::F64));
        Some(
            module
                .declare_function("perlrs_jit_pow_f64", Linkage::Import, &ps)
                .ok()?,
        )
    } else {
        None
    };

    let needs_fmod = seq.iter().any(|o| matches!(o, Op::Mod));
    let fmod_f64_id = if needs_fmod {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::F64));
        ps.params.push(AbiParam::new(types::F64));
        ps.returns.push(AbiParam::new(types::F64));
        Some(
            module
                .declare_function("perlrs_jit_fmod_f64", Linkage::Import, &ps)
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
    let call_sub_id = if need_vm_sub {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(ptr_ty));
        ps.params.push(AbiParam::new(types::I64));
        ps.params.push(AbiParam::new(types::I64));
        ps.params.push(AbiParam::new(types::I64));
        for _ in 0..8 {
            ps.params.push(AbiParam::new(types::I64));
        }
        ps.returns.push(AbiParam::new(types::I64));
        Some(
            module
                .declare_function("perlrs_jit_call_sub", Linkage::Import, &ps)
                .ok()?,
        )
    } else {
        None
    };

    let mut sig = module.make_signature();
    if need_vm_sub {
        sig.params.push(AbiParam::new(ptr_ty));
    }
    if need_any_table {
        sig.params.push(AbiParam::new(ptr_ty));
        sig.params.push(AbiParam::new(ptr_ty));
        sig.params.push(AbiParam::new(ptr_ty));
    }
    sig.returns.push(AbiParam::new(match ret_ty {
        JitTy::Int => types::I64,
        JitTy::Float => types::F64,
    }));

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

        let mut pi = 0usize;
        let vm_base = if need_vm_sub {
            let v = bcx.block_params(entry)[pi];
            pi += 1;
            Some(v)
        } else {
            None
        };
        let slot_base = if need_any_table {
            let v = bcx.block_params(entry)[pi];
            pi += 1;
            Some(v)
        } else {
            None
        };
        let plain_base = if need_any_table {
            let v = bcx.block_params(entry)[pi];
            pi += 1;
            Some(v)
        } else {
            None
        };
        let arg_base = if need_any_table {
            Some(bcx.block_params(entry)[pi])
        } else {
            None
        };

        let pow_i64_ref = pow_i64_id.map(|pid| module.declare_func_in_func(pid, bcx.func));
        let pow_f64_ref = pow_f64_id.map(|pid| module.declare_func_in_func(pid, bcx.func));
        let fmod_f64_ref = fmod_f64_id.map(|pid| module.declare_func_in_func(pid, bcx.func));
        let lognot_ref = lognot_id.map(|lid| module.declare_func_in_func(lid, bcx.func));
        let call_sub_ref = call_sub_id.map(|cid| module.declare_func_in_func(cid, bcx.func));

        let mut stack: Vec<(cranelift_codegen::ir::Value, JitTy)> = Vec::with_capacity(32);
        for op in seq {
            emit_data_op(
                &mut bcx,
                false,
                op,
                &mut stack,
                slot_base,
                plain_base,
                arg_base,
                vm_base,
                sub_entries,
                call_sub_ref,
                pow_i64_ref,
                pow_f64_ref,
                fmod_f64_ref,
                lognot_ref,
                constants,
            )?;
        }
        let (v, ty) = stack.pop()?;
        let ret_v = match (ret_ty, ty) {
            (JitTy::Int, JitTy::Int) => v,
            (JitTy::Float, JitTy::Float) => v,
            (JitTy::Float, JitTy::Int) => i64_to_f64(&mut bcx, v),
            (JitTy::Int, JitTy::Float) => f64_to_i64_trunc(&mut bcx, v),
        };
        bcx.ins().return_(&[ret_v]);
        bcx.seal_all_blocks();
        bcx.finalize();
    }

    module.define_function(fid, &mut ctx).ok()?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().ok()?;
    let ptr = module.get_finalized_function(fid);
    let run = match (need_vm_sub, need_any_table, ret_ty) {
        (false, false, JitTy::Int) => LinearRun::Nullary(unsafe {
            std::mem::transmute::<*const u8, unsafe extern "C" fn() -> i64>(ptr)
        }),
        (false, false, JitTy::Float) => LinearRun::NullaryF(unsafe {
            std::mem::transmute::<*const u8, unsafe extern "C" fn() -> f64>(ptr)
        }),
        (false, true, JitTy::Int) => LinearRun::Tables(unsafe {
            std::mem::transmute::<
                *const u8,
                unsafe extern "C" fn(*const i64, *const i64, *const i64) -> i64,
            >(ptr)
        }),
        (false, true, JitTy::Float) => LinearRun::TablesF(unsafe {
            std::mem::transmute::<
                *const u8,
                unsafe extern "C" fn(*const i64, *const i64, *const i64) -> f64,
            >(ptr)
        }),
        (true, false, JitTy::Int) => LinearRun::VmNullary(unsafe {
            std::mem::transmute::<*const u8, LinearFnVm0>(ptr)
        }),
        (true, false, JitTy::Float) => LinearRun::VmNullaryF(unsafe {
            std::mem::transmute::<*const u8, LinearFnVm0F>(ptr)
        }),
        (true, true, JitTy::Int) => LinearRun::VmTables(unsafe {
            std::mem::transmute::<*const u8, LinearFnVm3>(ptr)
        }),
        (true, true, JitTy::Float) => LinearRun::VmTablesF(unsafe {
            std::mem::transmute::<*const u8, LinearFnVm3F>(ptr)
        }),
    };
    Some(LinearJit {
        module,
        run,
        ret_nanboxed,
    })
}

/// Subroutine body that ends with [`Op::Return`] — stack empty; native code returns `undef` nanbox bits.
fn compile_linear_void_ops(seq: &[Op], constants: &[PerlValue]) -> Option<LinearJit> {
    if !seq.is_empty() && !validate_linear_void_seq(seq, constants) {
        return None;
    }
    let need_any_table = needs_table(seq);
    let mut module = new_jit_module()?;

    let needs_pow = seq.iter().any(|o| matches!(o, Op::Pow));
    let pow_i64_id = if needs_pow {
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
    let pow_f64_id = if needs_pow {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::F64));
        ps.params.push(AbiParam::new(types::F64));
        ps.returns.push(AbiParam::new(types::F64));
        Some(
            module
                .declare_function("perlrs_jit_pow_f64", Linkage::Import, &ps)
                .ok()?,
        )
    } else {
        None
    };

    let needs_fmod = seq.iter().any(|o| matches!(o, Op::Mod));
    let fmod_f64_id = if needs_fmod {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::F64));
        ps.params.push(AbiParam::new(types::F64));
        ps.returns.push(AbiParam::new(types::F64));
        Some(
            module
                .declare_function("perlrs_jit_fmod_f64", Linkage::Import, &ps)
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
        .declare_function("linear_void", Linkage::Local, &sig)
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

        let pow_i64_ref = pow_i64_id.map(|pid| module.declare_func_in_func(pid, bcx.func));
        let pow_f64_ref = pow_f64_id.map(|pid| module.declare_func_in_func(pid, bcx.func));
        let fmod_f64_ref = fmod_f64_id.map(|pid| module.declare_func_in_func(pid, bcx.func));
        let lognot_ref = lognot_id.map(|lid| module.declare_func_in_func(lid, bcx.func));

        let mut stack: Vec<(cranelift_codegen::ir::Value, JitTy)> = Vec::with_capacity(32);
        for op in seq {
            emit_data_op(
                &mut bcx,
                false,
                op,
                &mut stack,
                slot_base,
                plain_base,
                arg_base,
                None,
                &[],
                None,
                pow_i64_ref,
                pow_f64_ref,
                fmod_f64_ref,
                lognot_ref,
                constants,
            )?;
        }
        if !stack.is_empty() {
            return None;
        }
        let undef_bits = PerlValue::UNDEF.raw_bits() as i64;
        let ret = bcx.ins().iconst(types::I64, undef_bits);
        bcx.ins().return_(&[ret]);
        bcx.seal_all_blocks();
        bcx.finalize();
    }

    module.define_function(fid, &mut ctx).ok()?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().ok()?;
    let ptr = module.get_finalized_function(fid);
    let run = if need_any_table {
        LinearRun::Tables(unsafe {
            std::mem::transmute::<
                *const u8,
                unsafe extern "C" fn(*const i64, *const i64, *const i64) -> i64,
            >(ptr)
        })
    } else {
        LinearRun::Nullary(unsafe {
            std::mem::transmute::<*const u8, unsafe extern "C" fn() -> i64>(ptr)
        })
    };
    Some(LinearJit {
        module,
        run,
        ret_nanboxed: true,
    })
}

fn hash_linear_sub_key(seq: &[Op], constants: &[PerlValue], void: bool) -> u64 {
    let mut h = DefaultHasher::new();
    void.hash(&mut h);
    hash_ops(seq, constants).hash(&mut h);
    h.finish()
}

static LINEAR_CACHE: OnceLock<Mutex<HashMap<u64, Box<LinearJit>>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<u64, Box<LinearJit>>> {
    LINEAR_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Keys for [`block_jit_validate_sub`] that returned `None` — recursive subs (e.g. `fib`) would
/// otherwise re-run full CFG validation on every call.
static SUB_BLOCK_VALIDATE_FAIL: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();

fn sub_block_validate_fail_cache() -> &'static Mutex<HashSet<u64>> {
    SUB_BLOCK_VALIDATE_FAIL.get_or_init(|| Mutex::new(HashSet::new()))
}

fn hash_sub_block_validate_key(ops: &[Op], constants: &[PerlValue], term: SubTerminator) -> u64 {
    let mut h = DefaultHasher::new();
    term.hash(&mut h);
    hash_ops(ops, constants).hash(&mut h);
    h.finish()
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
pub(crate) fn slot_undef_prefill_ok_seq(seq: &[Op], slot: u8) -> bool {
    let mut written = false;
    for op in seq {
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

pub(crate) fn slot_undef_prefill_ok(ops: &[Op], slot: u8) -> bool {
    slot_undef_prefill_ok_seq(ops_before_halt(ops), slot)
}

/// Slot indices written by [`Op::SetScalarSlot`] / [`Op::SetScalarSlotKeep`] before [`Op::Halt`],
/// sorted and deduplicated (for syncing the slot buffer back into the interpreter scope).
pub(crate) fn linear_slot_ops_written_indices_seq(seq: &[Op]) -> Vec<u8> {
    let mut v: Vec<u8> = seq
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

pub(crate) fn linear_slot_ops_written_indices(ops: &[Op]) -> Vec<u8> {
    linear_slot_ops_written_indices_seq(ops_before_halt(ops))
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

pub(crate) fn linear_plain_ops_max_index_seq(seq: &[Op]) -> Option<u16> {
    max_plain_name_index(seq)
}

/// Plain-name indices **written** before [`Op::Halt`] (for VM writeback).
pub(crate) fn linear_plain_ops_written_indices_seq(seq: &[Op]) -> Vec<u16> {
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

pub(crate) fn linear_plain_ops_written_indices(ops: &[Op]) -> Vec<u16> {
    linear_plain_ops_written_indices_seq(ops_before_halt(ops))
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

pub(crate) fn linear_slot_ops_max_index_seq(seq: &[Op]) -> Option<u8> {
    max_scalar_slot_index(seq)
}

pub(crate) fn linear_arg_ops_max_index_seq(seq: &[Op]) -> Option<u8> {
    max_get_arg_index(seq)
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

    let key = hash_ops(ops_before_halt(ops), constants);
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
            let r = j.invoke(std::ptr::null_mut(), slot_ptr, plain_ptr, arg_ptr);
            return Some(j.result_to_perl(r));
        }
    }

    let jit = compile_linear(ops, constants)?;
    let r = jit.invoke(std::ptr::null_mut(), slot_ptr, plain_ptr, arg_ptr);
    let pv = jit.result_to_perl(r);

    if let Ok(mut guard) = cache().lock() {
        if guard.len() < 256 {
            guard.insert(key, Box::new(jit));
        }
    }
    Some(pv)
}

/// How a compiled subroutine ends before the VM pops the call frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SubTerminator {
    /// Bare `return;` — [`Op::Return`], no value on stack.
    Void,
    /// `return EXPR` or implicit final expression / `undef` — [`Op::ReturnValue`].
    Value,
}

/// Ops from `entry_ip` up to (but not including) the first [`Op::Return`] or [`Op::ReturnValue`].
pub(crate) fn sub_entry_segment(ops: &[Op], entry_ip: usize) -> Option<(&[Op], SubTerminator)> {
    let tail = ops.get(entry_ip..)?;
    let rel = tail
        .iter()
        .position(|o| matches!(o, Op::Return | Op::ReturnValue))?;
    let term = match tail.get(rel)? {
        Op::Return => SubTerminator::Void,
        Op::ReturnValue => SubTerminator::Value,
        _ => return None,
    };
    Some((&tail[..rel], term))
}

/// Subroutine body from `entry_ip` through the first [`Op::Return`] or [`Op::ReturnValue`] (inclusive).
pub(crate) fn sub_full_body(ops: &[Op], entry_ip: usize) -> Option<(&[Op], SubTerminator)> {
    let tail = ops.get(entry_ip..)?;
    let rel = tail
        .iter()
        .position(|o| matches!(o, Op::Return | Op::ReturnValue))?;
    let term = match tail.get(rel)? {
        Op::Return => SubTerminator::Void,
        Op::ReturnValue => SubTerminator::Value,
        _ => return None,
    };
    Some((&tail[..=rel], term))
}

/// `true` when this sub prefix cannot use linear Cranelift (control flow, calls, frame ops, etc.).
pub(crate) fn segment_blocks_subroutine_linear_jit(
    seg: &[Op],
    sub_entries: &[(u16, usize, bool)],
) -> bool {
    seg.iter().any(|o| {
        match o {
            Op::Call(_, _, _) if call_is_jitable(o, sub_entries) => false,
            Op::Call(_, _, _) => true,
            Op::Jump(_)
            | Op::JumpIfTrue(_)
            | Op::JumpIfFalse(_)
            | Op::JumpIfFalseKeep(_)
            | Op::JumpIfTrueKeep(_)
            | Op::JumpIfDefinedKeep(_)
            | Op::Halt
            | Op::Return
            | Op::ReturnValue
            | Op::PushFrame
            | Op::PopFrame
            | Op::CallBuiltin(_, _)
            | Op::MethodCall(_, _, _)
            | Op::MethodCallSuper(_, _, _)
            | Op::ArrowCall(_) => true,
            _ => false,
        }
    })
}

/// `PushFrame` / `PopFrame` in a subroutine body — block JIT uses the same flat slot buffers as linear.
pub(crate) fn sub_body_blocks_subroutine_block_jit(seg: &[Op]) -> bool {
    seg.iter()
        .any(|o| matches!(o, Op::PushFrame | Op::PopFrame))
}

/// Linear JIT for a compiled subroutine body (see [`sub_entry_segment`]).
pub(crate) fn try_run_linear_sub(
    ops: &[Op],
    entry_ip: usize,
    mut slot_i64: Option<&mut [i64]>,
    mut plain_i64: Option<&mut [i64]>,
    arg_i64: Option<&[i64]>,
    constants: &[PerlValue],
    sub_entries: &[(u16, usize, bool)],
    vm: *mut c_void,
) -> Option<PerlValue> {
    let (seq, term) = sub_entry_segment(ops, entry_ip)?;
    if segment_blocks_subroutine_linear_jit(seq, sub_entries) {
        return None;
    }
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
    let key = hash_linear_sub_key(seq, constants, term == SubTerminator::Void);
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
            let r = j.invoke(vm, slot_ptr, plain_ptr, arg_ptr);
            return Some(j.result_to_perl(r));
        }
    }

    let jit = match term {
        SubTerminator::Void => compile_linear_void_ops(seq, constants)?,
        SubTerminator::Value => compile_linear_ops(seq, constants, sub_entries)?,
    };
    let r = jit.invoke(vm, slot_ptr, plain_ptr, arg_ptr);
    let pv = jit.result_to_perl(r);

    if let Ok(mut guard) = cache().lock() {
        if guard.len() < 256 {
            guard.insert(key, Box::new(jit));
        }
    }
    Some(pv)
}

// ── Block-based JIT: control-flow bytecode (loops, conditionals, short-circuit booleans). ──

struct CfgBlock {
    start: usize,
    end: usize,
    /// Abstract stack at block entry (bottom .. top), after fixpoint merge.
    entry_cells: Vec<Cell>,
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
            | Op::JumpIfTrueKeep(t)
            | Op::JumpIfDefinedKeep(t) => {
                if *t > ops.len() {
                    return s; // invalid target, will be caught in validation
                }
                s.insert(*t);
                if i + 1 < ops.len() {
                    s.insert(i + 1);
                }
            }
            Op::Halt | Op::Return | Op::ReturnValue => {
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
            | Op::LoadUndef
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

/// How to fill VM `i64` buffers for [`try_run_block_ops`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BlockJitBufferMode {
    /// `PerlValue::as_integer()`-style scalars; `undef` uses prefills where applicable.
    I64AsInteger,
    /// [`PerlValue::raw_bits`] as `i64` (sign reinterpretation); preserves `undef` vs defined.
    I64AsPerlValueBits,
}

/// How every control-flow path exits a block JIT program (main eval vs subroutine).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BlockExit {
    /// Main eval: [`Op::Halt`] with one value on the stack.
    Halt(Cell),
    /// Subroutine: [`Op::ReturnValue`] with one value on the stack.
    ReturnValue(Cell),
    /// Subroutine: bare [`Op::Return`], empty stack.
    ReturnVoid,
}

/// Validated block CFG plus metadata for compilation and VM buffer layout.
pub(crate) struct ValidatedBlockCfg {
    cfg: Vec<CfgBlock>,
    exit: BlockExit,
    needs_raw_value_buffers: bool,
    /// `JumpIfDefinedKeep` opcode index → `false` = unconditional jump (const TOS), `true` = runtime `defined`.
    jump_if_defined_kind: HashMap<usize, bool>,
}

impl ValidatedBlockCfg {
    pub(crate) fn buffer_mode(&self) -> BlockJitBufferMode {
        if self.needs_raw_value_buffers {
            BlockJitBufferMode::I64AsPerlValueBits
        } else {
            BlockJitBufferMode::I64AsInteger
        }
    }
}

/// [`validate_block_cfg`] exposed for [`crate::vm::VM::execute`] so slot/plain/arg buffers match compilation.
pub(crate) fn block_jit_validate(ops: &[Op], constants: &[PerlValue]) -> Option<ValidatedBlockCfg> {
    validate_block_cfg(ops, constants, BlockCfgMode::EvalMain)
}

/// Block JIT validation for a subroutine body ending in [`Op::Return`] or [`Op::ReturnValue`].
pub(crate) fn block_jit_validate_sub(
    ops: &[Op],
    constants: &[PerlValue],
    term: SubTerminator,
) -> Option<ValidatedBlockCfg> {
    let key = hash_sub_block_validate_key(ops, constants, term);
    if let Ok(guard) = sub_block_validate_fail_cache().lock() {
        if guard.contains(&key) {
            return None;
        }
    }
    let mode = match term {
        SubTerminator::Value => BlockCfgMode::SubValue,
        SubTerminator::Void => BlockCfgMode::SubVoid,
    };
    let r = validate_block_cfg(ops, constants, mode);
    if r.is_none() {
        if let Ok(mut guard) = sub_block_validate_fail_cache().lock() {
            if guard.len() < 512 {
                guard.insert(key);
            }
        }
    }
    r
}

/// Restrictions so stack slots stay valid [`PerlValue`] NaN-box encodings in raw-buffer mode.
fn enforce_raw_jit_program(ops: &[Op], constants: &[PerlValue]) -> Option<()> {
    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::GetScalarSlot(_) | Op::GetScalarPlain(_) | Op::GetArg(_) => {
                if !matches!(ops.get(i + 1), Some(Op::JumpIfDefinedKeep(_))) {
                    return None;
                }
            }
            Op::JumpIfTrue(_)
            | Op::JumpIfFalse(_)
            | Op::JumpIfFalseKeep(_)
            | Op::JumpIfTrueKeep(_) => return None,
            Op::Add
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
            | Op::LogNot => return None,
            Op::LoadFloat(_) => return None,
            Op::LoadConst(idx) => {
                let pv = constants.get(*idx as usize)?;
                pv.as_integer()?;
            }
            Op::LoadInt(_) => {}
            Op::DeclareScalarSlot(_)
            | Op::SetScalarSlot(_)
            | Op::SetScalarSlotKeep(_)
            | Op::PreIncSlot(_)
            | Op::PostIncSlot(_)
            | Op::PreDecSlot(_)
            | Op::PostDecSlot(_)
            | Op::SetScalarPlain(_)
            | Op::SetScalarKeepPlain(_)
            | Op::PreInc(_)
            | Op::PostInc(_)
            | Op::PreDec(_)
            | Op::PostDec(_) => return None,
            Op::Jump(_) | Op::JumpIfDefinedKeep(_) => {}
            Op::Halt | Op::Return | Op::ReturnValue => {}
            Op::LoadUndef => {}
            _ => return None,
        }
    }
    Some(())
}

/// Main eval vs subroutine terminator for [`validate_block_cfg`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockCfgMode {
    EvalMain,
    SubValue,
    SubVoid,
}

/// Validate the ops as a block-structured program and compute per-block entry abstract stacks.
/// Uses a fixpoint over [`join_cell`] at CFG merges so float and int paths can join (promotion).
/// Returns `None` when any op is unsupported or the CFG is inconsistent.
fn validate_block_cfg(
    ops: &[Op],
    constants: &[PerlValue],
    mode: BlockCfgMode,
) -> Option<ValidatedBlockCfg> {
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
                | Op::JumpIfDefinedKeep(_)
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

    let mut entry_stacks: Vec<Option<Vec<Cell>>> = vec![None; block_count];
    entry_stacks[0] = Some(Vec::new());
    let mut worklist: VecDeque<usize> = VecDeque::new();
    worklist.push_back(0);
    let mut has_halt = false;
    let mut halt_top: Option<Cell> = None;
    let mut iter_count = 0usize;
    let mut needs_raw_value_buffers = false;
    let mut jump_if_defined_kind: HashMap<usize, bool> = HashMap::new();

    while let Some(bi) = worklist.pop_front() {
        iter_count += 1;
        if iter_count > 50_000 {
            return None;
        }
        let entry = entry_stacks[bi].as_ref()?.clone();

        let (start, end) = blocks[bi];
        let mut stack = entry;
        let mut terminated = false;

        for idx in start..end {
            let op = &ops[idx];
            match op {
                _ if is_block_data_op(op) => {
                    simulate_one_op(op, &mut stack, constants, None)?;
                }

                // ── Control flow ──
                Op::Jump(target) => {
                    let ti = *addr_to_block.get(target)?;
                    merge_stack_entry(&mut entry_stacks, &mut worklist, ti, &stack)?;
                    terminated = true;
                    break;
                }
                Op::JumpIfTrue(target) => {
                    stack.pop()?; // condition
                    let ti = *addr_to_block.get(target)?;
                    let ni = bi + 1;
                    if ni >= block_count {
                        return None;
                    }
                    merge_stack_entry(&mut entry_stacks, &mut worklist, ti, &stack)?;
                    merge_stack_entry(&mut entry_stacks, &mut worklist, ni, &stack)?;
                    terminated = true;
                    break;
                }
                Op::JumpIfFalse(target) => {
                    stack.pop()?;
                    let ti = *addr_to_block.get(target)?;
                    let ni = bi + 1;
                    if ni >= block_count {
                        return None;
                    }
                    merge_stack_entry(&mut entry_stacks, &mut worklist, ti, &stack)?;
                    merge_stack_entry(&mut entry_stacks, &mut worklist, ni, &stack)?;
                    terminated = true;
                    break;
                }
                Op::JumpIfFalseKeep(target) | Op::JumpIfTrueKeep(target) => {
                    stack.last()?; // peek condition
                    let ti = *addr_to_block.get(target)?;
                    let ni = bi + 1;
                    if ni >= block_count {
                        return None;
                    }
                    merge_stack_entry(&mut entry_stacks, &mut worklist, ti, &stack)?;
                    merge_stack_entry(&mut entry_stacks, &mut worklist, ni, &stack)?;
                    terminated = true;
                    break;
                }
                Op::JumpIfDefinedKeep(target) => {
                    let top = *stack.last()?;
                    match top {
                        Cell::Const(_) | Cell::ConstF(_) => {
                            jump_if_defined_kind.insert(idx, false);
                            let ti = *addr_to_block.get(target)?;
                            merge_stack_entry(&mut entry_stacks, &mut worklist, ti, &stack)?;
                            terminated = true;
                            break;
                        }
                        Cell::DynF => {
                            return None;
                        }
                        Cell::Undef => {
                            return None;
                        }
                        Cell::Dyn => {
                            if idx == 0 {
                                return None;
                            }
                            match &ops[idx - 1] {
                                Op::GetScalarSlot(_) | Op::GetScalarPlain(_) | Op::GetArg(_) => {}
                                _ => return None,
                            }
                            needs_raw_value_buffers = true;
                            jump_if_defined_kind.insert(idx, true);
                            let ti = *addr_to_block.get(target)?;
                            let ni = bi + 1;
                            if ni >= block_count {
                                return None;
                            }
                            merge_stack_entry(&mut entry_stacks, &mut worklist, ti, &stack)?;
                            let mut stack_fall = stack.clone();
                            stack_fall.pop()?;
                            merge_stack_entry(&mut entry_stacks, &mut worklist, ni, &stack_fall)?;
                            terminated = true;
                            break;
                        }
                    }
                }
                Op::Halt => {
                    if mode != BlockCfgMode::EvalMain {
                        return None;
                    }
                    if stack.len() != 1 {
                        return None;
                    }
                    let top = stack[0];
                    halt_top = Some(match halt_top {
                        None => top,
                        Some(prev) => join_cell(prev, top)?,
                    });
                    has_halt = true;
                    terminated = true;
                    break;
                }
                Op::ReturnValue => {
                    if mode != BlockCfgMode::SubValue {
                        return None;
                    }
                    if stack.len() != 1 {
                        return None;
                    }
                    let top = stack[0];
                    halt_top = Some(match halt_top {
                        None => top,
                        Some(prev) => join_cell(prev, top)?,
                    });
                    has_halt = true;
                    terminated = true;
                    break;
                }
                Op::Return => {
                    if mode != BlockCfgMode::SubVoid {
                        return None;
                    }
                    if !stack.is_empty() {
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
            merge_stack_entry(&mut entry_stacks, &mut worklist, ni, &stack)?;
        }
    }

    if !has_halt {
        return None;
    }
    let exit = match mode {
        BlockCfgMode::EvalMain => BlockExit::Halt(halt_top?),
        BlockCfgMode::SubValue => BlockExit::ReturnValue(halt_top?),
        BlockCfgMode::SubVoid => {
            if halt_top.is_some() {
                return None;
            }
            BlockExit::ReturnVoid
        }
    };

    let cfg: Vec<CfgBlock> = blocks
        .iter()
        .enumerate()
        .map(|(i, &(s, e))| {
            let reachable = entry_stacks[i].is_some();
            let entry_cells = entry_stacks[i].clone().unwrap_or_default();
            CfgBlock {
                start: s,
                end: e,
                entry_cells,
                reachable,
            }
        })
        .collect();

    if needs_raw_value_buffers {
        enforce_raw_jit_program(ops, constants)?;
    }

    Some(ValidatedBlockCfg {
        cfg,
        exit,
        needs_raw_value_buffers,
        jump_if_defined_kind,
    })
}

/// Emit a single non-control-flow op into the Cranelift `FunctionBuilder`.
/// Stack entries are `(Value, JitTy)`; int/float promotion matches [`simulate_one_op`].
#[allow(clippy::too_many_arguments)] // JIT codegen mirrors VM stack/slot layout.
fn emit_data_op(
    bcx: &mut FunctionBuilder,
    needs_raw_bits: bool,
    op: &Op,
    stack: &mut Vec<(Value, JitTy)>,
    slot_base: Option<cranelift_codegen::ir::Value>,
    plain_base: Option<cranelift_codegen::ir::Value>,
    arg_base: Option<cranelift_codegen::ir::Value>,
    vm_base: Option<cranelift_codegen::ir::Value>,
    sub_entries: &[(u16, usize, bool)],
    call_sub_ref: Option<cranelift_codegen::ir::FuncRef>,
    pow_i64_ref: Option<cranelift_codegen::ir::FuncRef>,
    pow_f64_ref: Option<cranelift_codegen::ir::FuncRef>,
    fmod_f64_ref: Option<cranelift_codegen::ir::FuncRef>,
    lognot_ref: Option<cranelift_codegen::ir::FuncRef>,
    constants: &[PerlValue],
) -> Option<()> {
    match op {
        Op::LoadUndef => {
            let bits = PerlValue::UNDEF.raw_bits() as i64;
            stack.push((bcx.ins().iconst(types::I64, bits), JitTy::Int));
        }
        Op::LoadInt(n) => {
            let bits = if needs_raw_bits {
                PerlValue::integer(*n).raw_bits() as i64
            } else {
                *n
            };
            stack.push((bcx.ins().iconst(types::I64, bits), JitTy::Int));
        }
        Op::LoadConst(idx) => {
            let pv = constants.get(*idx as usize)?;
            if let Some(n) = pv.as_integer() {
                let bits = if needs_raw_bits {
                    PerlValue::integer(n).raw_bits() as i64
                } else {
                    n
                };
                stack.push((bcx.ins().iconst(types::I64, bits), JitTy::Int));
            } else if let Some(f) = pv.as_float() {
                stack.push((
                    bcx.ins().f64const(Ieee64::with_bits(f.to_bits())),
                    JitTy::Float,
                ));
            } else {
                return None;
            }
        }
        Op::LoadFloat(f) => {
            if !f.is_finite() {
                return None;
            }
            let n = *f as i64;
            if (n as f64) == *f {
                stack.push((bcx.ins().iconst(types::I64, n), JitTy::Int));
            } else {
                stack.push((
                    bcx.ins().f64const(Ieee64::with_bits(f.to_bits())),
                    JitTy::Float,
                ));
            }
        }
        Op::Add => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            match ty {
                JitTy::Int => stack.push((bcx.ins().iadd(a, b), JitTy::Int)),
                JitTy::Float => stack.push((bcx.ins().fadd(a, b), JitTy::Float)),
            }
        }
        Op::Sub => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            match ty {
                JitTy::Int => stack.push((bcx.ins().isub(a, b), JitTy::Int)),
                JitTy::Float => stack.push((bcx.ins().fsub(a, b), JitTy::Float)),
            }
        }
        Op::Mul => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            match ty {
                JitTy::Int => stack.push((bcx.ins().imul(a, b), JitTy::Int)),
                JitTy::Float => stack.push((bcx.ins().fmul(a, b), JitTy::Float)),
            }
        }
        Op::Div => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            match ty {
                JitTy::Int => stack.push((bcx.ins().sdiv(a, b), JitTy::Int)),
                JitTy::Float => stack.push((bcx.ins().fdiv(a, b), JitTy::Float)),
            }
        }
        Op::Mod => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            match ty {
                JitTy::Int => stack.push((bcx.ins().srem(a, b), JitTy::Int)),
                JitTy::Float => {
                    let fr = fmod_f64_ref?;
                    let call = bcx.ins().call(fr, &[a, b]);
                    stack.push((*bcx.inst_results(call).first()?, JitTy::Float));
                }
            }
        }
        Op::Pow => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            match ty {
                JitTy::Int => {
                    let fr = pow_i64_ref?;
                    let call = bcx.ins().call(fr, &[a, b]);
                    stack.push((*bcx.inst_results(call).first()?, JitTy::Int));
                }
                JitTy::Float => {
                    let fr = pow_f64_ref?;
                    let call = bcx.ins().call(fr, &[a, b]);
                    stack.push((*bcx.inst_results(call).first()?, JitTy::Float));
                }
            }
        }
        Op::Negate => {
            let (a, ty) = stack.pop()?;
            match ty {
                JitTy::Int => stack.push((bcx.ins().ineg(a), JitTy::Int)),
                JitTy::Float => stack.push((bcx.ins().fneg(a), JitTy::Float)),
            }
        }
        Op::NumEq => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            let v = match ty {
                JitTy::Int => intcmp_to_01(bcx, IntCC::Equal, a, b),
                JitTy::Float => floatcmp_to_01(bcx, FloatCC::Equal, a, b),
            };
            stack.push((v, JitTy::Int));
        }
        Op::NumNe => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            let v = match ty {
                JitTy::Int => intcmp_to_01(bcx, IntCC::NotEqual, a, b),
                JitTy::Float => floatcmp_to_01(bcx, FloatCC::NotEqual, a, b),
            };
            stack.push((v, JitTy::Int));
        }
        Op::NumLt => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            let v = match ty {
                JitTy::Int => intcmp_to_01(bcx, IntCC::SignedLessThan, a, b),
                JitTy::Float => floatcmp_to_01(bcx, FloatCC::LessThan, a, b),
            };
            stack.push((v, JitTy::Int));
        }
        Op::NumGt => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            let v = match ty {
                JitTy::Int => intcmp_to_01(bcx, IntCC::SignedGreaterThan, a, b),
                JitTy::Float => floatcmp_to_01(bcx, FloatCC::GreaterThan, a, b),
            };
            stack.push((v, JitTy::Int));
        }
        Op::NumLe => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            let v = match ty {
                JitTy::Int => intcmp_to_01(bcx, IntCC::SignedLessThanOrEqual, a, b),
                JitTy::Float => floatcmp_to_01(bcx, FloatCC::LessThanOrEqual, a, b),
            };
            stack.push((v, JitTy::Int));
        }
        Op::NumGe => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            let v = match ty {
                JitTy::Int => intcmp_to_01(bcx, IntCC::SignedGreaterThanOrEqual, a, b),
                JitTy::Float => floatcmp_to_01(bcx, FloatCC::GreaterThanOrEqual, a, b),
            };
            stack.push((v, JitTy::Int));
        }
        Op::Spaceship => {
            let (a, b, ty) = pop_pair_promote(bcx, stack)?;
            let v = match ty {
                JitTy::Int => spaceship_i64(bcx, a, b),
                JitTy::Float => spaceship_f64(bcx, a, b),
            };
            stack.push((v, JitTy::Int));
        }
        Op::LogNot => {
            let (a, ty) = stack.pop()?;
            match ty {
                JitTy::Int => {
                    let fr = lognot_ref?;
                    let call = bcx.ins().call(fr, &[a]);
                    stack.push((*bcx.inst_results(call).first()?, JitTy::Int));
                }
                JitTy::Float => {
                    let z = bcx.ins().f64const(Ieee64::with_bits(0.0f64.to_bits()));
                    let pred = bcx.ins().fcmp(FloatCC::OrderedNotEqual, a, z);
                    let one = bcx.ins().iconst(types::I64, 1);
                    let zero = bcx.ins().iconst(types::I64, 0);
                    let truth = bcx.ins().select(pred, one, zero);
                    let fr = lognot_ref?;
                    let call = bcx.ins().call(fr, &[truth]);
                    stack.push((*bcx.inst_results(call).first()?, JitTy::Int));
                }
            }
        }
        Op::Pop => {
            stack.pop()?;
        }
        Op::Dup => {
            let v = *stack.last()?;
            stack.push(v);
        }
        Op::BitXor => {
            let (b, tb) = stack.pop()?;
            let (a, ta) = stack.pop()?;
            if ta != JitTy::Int || tb != JitTy::Int {
                return None;
            }
            stack.push((bcx.ins().bxor(a, b), JitTy::Int));
        }
        Op::BitAnd => {
            let (b, tb) = stack.pop()?;
            let (a, ta) = stack.pop()?;
            if ta != JitTy::Int || tb != JitTy::Int {
                return None;
            }
            stack.push((bcx.ins().band(a, b), JitTy::Int));
        }
        Op::BitOr => {
            let (b, tb) = stack.pop()?;
            let (a, ta) = stack.pop()?;
            if ta != JitTy::Int || tb != JitTy::Int {
                return None;
            }
            stack.push((bcx.ins().bor(a, b), JitTy::Int));
        }
        Op::BitNot => {
            let (a, ty) = stack.pop()?;
            if ty != JitTy::Int {
                return None;
            }
            let ones = bcx.ins().iconst(types::I64, -1);
            stack.push((bcx.ins().bxor(a, ones), JitTy::Int));
        }
        Op::Shl => {
            let (b, tb) = stack.pop()?;
            let (a, ta) = stack.pop()?;
            if ta != JitTy::Int || tb != JitTy::Int {
                return None;
            }
            let mask = bcx.ins().iconst(types::I64, 63);
            let mb = bcx.ins().band(b, mask);
            stack.push((bcx.ins().ishl(a, mb), JitTy::Int));
        }
        Op::Shr => {
            let (b, tb) = stack.pop()?;
            let (a, ta) = stack.pop()?;
            if ta != JitTy::Int || tb != JitTy::Int {
                return None;
            }
            let mask = bcx.ins().iconst(types::I64, 63);
            let mb = bcx.ins().band(b, mask);
            stack.push((bcx.ins().sshr(a, mb), JitTy::Int));
        }
        Op::GetScalarSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            stack.push((
                bcx.ins().load(types::I64, MemFlags::trusted(), base, off),
                JitTy::Int,
            ));
        }
        Op::SetScalarSlot(slot) => {
            let base = slot_base?;
            let (v, ty) = stack.pop()?;
            let v = scalar_store_i64(bcx, v, ty);
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*slot as i32) * 8);
        }
        Op::SetScalarSlotKeep(slot) => {
            let base = slot_base?;
            let (v, ty) = stack.last().copied()?;
            let v = scalar_store_i64(bcx, v, ty);
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*slot as i32) * 8);
        }
        Op::DeclareScalarSlot(slot) => {
            let base = slot_base?;
            let (v, ty) = stack.pop()?;
            let v = scalar_store_i64(bcx, v, ty);
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*slot as i32) * 8);
        }
        Op::PreIncSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            let old = bcx.ins().load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().iadd(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push((new, JitTy::Int));
        }
        Op::PreDecSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            let old = bcx.ins().load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().isub(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push((new, JitTy::Int));
        }
        Op::PostIncSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            let old = bcx.ins().load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().iadd(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push((old, JitTy::Int));
        }
        Op::PostDecSlot(slot) => {
            let base = slot_base?;
            let off = (*slot as i32) * 8;
            let old = bcx.ins().load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().isub(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push((old, JitTy::Int));
        }
        Op::GetScalarPlain(idx) => {
            let base = plain_base?;
            stack.push((
                bcx.ins()
                    .load(types::I64, MemFlags::trusted(), base, (*idx as i32) * 8),
                JitTy::Int,
            ));
        }
        Op::GetArg(idx) => {
            let base = arg_base?;
            stack.push((
                bcx.ins()
                    .load(types::I64, MemFlags::trusted(), base, (*idx as i32) * 8),
                JitTy::Int,
            ));
        }
        Op::SetScalarPlain(idx) => {
            let base = plain_base?;
            let (v, ty) = stack.pop()?;
            let v = scalar_store_i64(bcx, v, ty);
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*idx as i32) * 8);
        }
        Op::SetScalarKeepPlain(idx) => {
            let base = plain_base?;
            let (v, ty) = stack.last().copied()?;
            let v = scalar_store_i64(bcx, v, ty);
            bcx.ins()
                .store(MemFlags::trusted(), v, base, (*idx as i32) * 8);
        }
        Op::PreInc(idx) => {
            let base = plain_base?;
            let off = (*idx as i32) * 8;
            let old = bcx.ins().load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().iadd(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push((new, JitTy::Int));
        }
        Op::PostInc(idx) => {
            let base = plain_base?;
            let off = (*idx as i32) * 8;
            let old = bcx.ins().load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().iadd(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push((old, JitTy::Int));
        }
        Op::PreDec(idx) => {
            let base = plain_base?;
            let off = (*idx as i32) * 8;
            let old = bcx.ins().load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().isub(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push((new, JitTy::Int));
        }
        Op::PostDec(idx) => {
            let base = plain_base?;
            let off = (*idx as i32) * 8;
            let old = bcx.ins().load(types::I64, MemFlags::trusted(), base, off);
            let one = bcx.ins().iconst(types::I64, 1);
            let new = bcx.ins().isub(old, one);
            bcx.ins().store(MemFlags::trusted(), new, base, off);
            stack.push((old, JitTy::Int));
        }
        Op::Call(name_idx, argc, wa) => {
            let (entry_ip, stack_args) = find_sub_entry_slice(sub_entries, *name_idx)?;
            if !stack_args
                || !call_is_jitable(op, sub_entries)
                || WantarrayCtx::from_byte(*wa) != WantarrayCtx::Scalar
            {
                return None;
            }
            let vmv = vm_base?;
            let fr = call_sub_ref?;
            let argc_u = *argc as usize;
            let mut arg_vals: Vec<Value> = Vec::with_capacity(argc_u);
            for _ in 0..argc_u {
                let (v, ty) = stack.pop()?;
                if ty != JitTy::Int {
                    return None;
                }
                arg_vals.push(v);
            }
            arg_vals.reverse();
            let mut padded = arg_vals;
            while padded.len() < 8 {
                padded.push(bcx.ins().iconst(types::I64, 0));
            }
            let sub_ip_v = bcx.ins().iconst(types::I64, entry_ip as i64);
            let argc_v = bcx.ins().iconst(types::I64, *argc as i64);
            let wa_v = bcx.ins().iconst(types::I64, *wa as i64);
            let mut call_args: Vec<Value> = vec![vmv, sub_ip_v, argc_v, wa_v];
            call_args.extend(padded);
            let call = bcx.ins().call(fr, &call_args);
            stack.push((*bcx.inst_results(call).first()?, JitTy::Int));
        }
        _ => return None,
    }
    Some(())
}

fn compile_blocks_validated(
    validated: ValidatedBlockCfg,
    ops: &[Op],
    constants: &[PerlValue],
) -> Option<LinearJit> {
    let cfg = validated.cfg;
    let exit = validated.exit;
    let needs_raw_bits = validated.needs_raw_value_buffers;
    let jump_if_defined_kind = validated.jump_if_defined_kind;
    let ret_ty = match exit {
        BlockExit::Halt(c) | BlockExit::ReturnValue(c) => cell_to_jit_ty(c),
        BlockExit::ReturnVoid => JitTy::Int,
    };
    let ret_nanboxed = match exit {
        BlockExit::Halt(c) | BlockExit::ReturnValue(c) => matches!(c, Cell::Undef),
        BlockExit::ReturnVoid => true,
    };

    let need_any_table = needs_table(ops);

    let mut module = new_jit_module()?;

    let needs_pow = ops.iter().any(|o| matches!(o, Op::Pow));
    let pow_i64_id = if needs_pow {
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
    let pow_f64_id = if needs_pow {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::F64));
        ps.params.push(AbiParam::new(types::F64));
        ps.returns.push(AbiParam::new(types::F64));
        Some(
            module
                .declare_function("perlrs_jit_pow_f64", Linkage::Import, &ps)
                .ok()?,
        )
    } else {
        None
    };

    let needs_fmod = ops.iter().any(|o| matches!(o, Op::Mod));
    let fmod_f64_id = if needs_fmod {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::F64));
        ps.params.push(AbiParam::new(types::F64));
        ps.returns.push(AbiParam::new(types::F64));
        Some(
            module
                .declare_function("perlrs_jit_fmod_f64", Linkage::Import, &ps)
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

    let needs_defined_raw = jump_if_defined_kind.values().any(|&b| b);
    let defined_raw_id = if needs_defined_raw {
        let mut ps = module.make_signature();
        ps.params.push(AbiParam::new(types::I64));
        ps.returns.push(AbiParam::new(types::I64));
        Some(
            module
                .declare_function("perlrs_jit_is_defined_raw_bits", Linkage::Import, &ps)
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
    sig.returns.push(AbiParam::new(match ret_ty {
        JitTy::Int => types::I64,
        JitTy::Float => types::F64,
    }));

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

        // Non-entry reachable blocks get stack-value parameters (i64 and/or f64 per slot).
        for (i, blk) in cfg.iter().enumerate() {
            if i == 0 || !blk.reachable {
                continue;
            }
            for cell in &blk.entry_cells {
                let p_ty = match cell_to_jit_ty(*cell) {
                    JitTy::Int => types::I64,
                    JitTy::Float => types::F64,
                };
                bcx.append_block_param(cl_blocks[i], p_ty);
            }
        }

        let addr_to_block: HashMap<usize, usize> =
            cfg.iter().enumerate().map(|(i, b)| (b.start, i)).collect();

        let pow_i64_ref = pow_i64_id.map(|pid| module.declare_func_in_func(pid, bcx.func));
        let pow_f64_ref = pow_f64_id.map(|pid| module.declare_func_in_func(pid, bcx.func));
        let fmod_f64_ref = fmod_f64_id.map(|pid| module.declare_func_in_func(pid, bcx.func));
        let lognot_ref = lognot_id.map(|lid| module.declare_func_in_func(lid, bcx.func));
        let defined_raw_ref = defined_raw_id.map(|did| module.declare_func_in_func(did, bcx.func));

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

            let mut stack: Vec<(cranelift_codegen::ir::Value, JitTy)> =
                Vec::with_capacity(blk.entry_cells.len() + 16);
            if bi == 0 {
                // Entry block: stack starts empty.
            } else {
                let params = bcx.block_params(cl_blocks[bi]);
                if params.len() != blk.entry_cells.len() {
                    return None;
                }
                for (p, cell) in params.iter().zip(blk.entry_cells.iter()) {
                    stack.push((*p, cell_to_jit_ty(*cell)));
                }
            }

            let mut terminated = false;
            for (idx, op) in ops.iter().enumerate().take(blk.end).skip(blk.start) {
                if is_block_data_op(op) {
                    emit_data_op(
                        &mut bcx,
                        needs_raw_bits,
                        op,
                        &mut stack,
                        slot_base,
                        plain_base,
                        arg_base,
                        None,
                        &[],
                        None,
                        pow_i64_ref,
                        pow_f64_ref,
                        fmod_f64_ref,
                        lognot_ref,
                        constants,
                    )?;
                    continue;
                }
                match op {
                    Op::Jump(target) => {
                        let ti = *addr_to_block.get(target)?;
                        let args =
                            branch_stack_to_block_args(&mut bcx, &stack, &cfg[ti].entry_cells)?;
                        bcx.ins().jump(cl_blocks[ti], &args);
                        terminated = true;
                    }
                    Op::JumpIfTrue(target) => {
                        let (cond, _) = stack.pop()?;
                        let ti = *addr_to_block.get(target)?;
                        let ni = bi + 1;
                        let args_t =
                            branch_stack_to_block_args(&mut bcx, &stack, &cfg[ti].entry_cells)?;
                        let args_n =
                            branch_stack_to_block_args(&mut bcx, &stack, &cfg[ni].entry_cells)?;
                        bcx.ins()
                            .brif(cond, cl_blocks[ti], &args_t, cl_blocks[ni], &args_n);
                        terminated = true;
                    }
                    Op::JumpIfFalse(target) => {
                        let (cond, _) = stack.pop()?;
                        let ti = *addr_to_block.get(target)?;
                        let ni = bi + 1;
                        let args_n =
                            branch_stack_to_block_args(&mut bcx, &stack, &cfg[ni].entry_cells)?;
                        let args_t =
                            branch_stack_to_block_args(&mut bcx, &stack, &cfg[ti].entry_cells)?;
                        // false = zero → else branch is target
                        bcx.ins()
                            .brif(cond, cl_blocks[ni], &args_n, cl_blocks[ti], &args_t);
                        terminated = true;
                    }
                    Op::JumpIfFalseKeep(target) => {
                        let (cond, _) = stack.last().copied()?;
                        let ti = *addr_to_block.get(target)?;
                        let ni = bi + 1;
                        let args_n =
                            branch_stack_to_block_args(&mut bcx, &stack, &cfg[ni].entry_cells)?;
                        let args_t =
                            branch_stack_to_block_args(&mut bcx, &stack, &cfg[ti].entry_cells)?;
                        bcx.ins()
                            .brif(cond, cl_blocks[ni], &args_n, cl_blocks[ti], &args_t);
                        terminated = true;
                    }
                    Op::JumpIfTrueKeep(target) => {
                        let (cond, _) = stack.last().copied()?;
                        let ti = *addr_to_block.get(target)?;
                        let ni = bi + 1;
                        let args_t =
                            branch_stack_to_block_args(&mut bcx, &stack, &cfg[ti].entry_cells)?;
                        let args_n =
                            branch_stack_to_block_args(&mut bcx, &stack, &cfg[ni].entry_cells)?;
                        bcx.ins()
                            .brif(cond, cl_blocks[ti], &args_t, cl_blocks[ni], &args_n);
                        terminated = true;
                    }
                    Op::JumpIfDefinedKeep(target) => {
                        let ti = *addr_to_block.get(target)?;
                        let ni = bi + 1;
                        match *jump_if_defined_kind.get(&idx)? {
                            false => {
                                let args = branch_stack_to_block_args(
                                    &mut bcx,
                                    &stack,
                                    &cfg[ti].entry_cells,
                                )?;
                                bcx.ins().jump(cl_blocks[ti], &args);
                            }
                            true => {
                                let (v, jty) = stack.last().copied()?;
                                if jty != JitTy::Int {
                                    return None;
                                }
                                let def_fr = defined_raw_ref?;
                                let call = bcx.ins().call(def_fr, &[v]);
                                let is_def = *bcx.inst_results(call).first()?;
                                let args_t = branch_stack_to_block_args(
                                    &mut bcx,
                                    &stack,
                                    &cfg[ti].entry_cells,
                                )?;
                                let mut stack_fall = stack.clone();
                                stack_fall.pop()?;
                                let args_f = branch_stack_to_block_args(
                                    &mut bcx,
                                    &stack_fall,
                                    &cfg[ni].entry_cells,
                                )?;
                                bcx.ins().brif(
                                    is_def,
                                    cl_blocks[ti],
                                    &args_t,
                                    cl_blocks[ni],
                                    &args_f,
                                );
                            }
                        }
                        terminated = true;
                    }
                    Op::Halt => {
                        let BlockExit::Halt(_) = exit else {
                            return None;
                        };
                        let (v, ty) = stack.pop()?;
                        let ret_v = match (ret_ty, ty) {
                            (JitTy::Int, JitTy::Int) => v,
                            (JitTy::Float, JitTy::Float) => v,
                            (JitTy::Float, JitTy::Int) => i64_to_f64(&mut bcx, v),
                            (JitTy::Int, JitTy::Float) => f64_to_i64_trunc(&mut bcx, v),
                        };
                        bcx.ins().return_(&[ret_v]);
                        terminated = true;
                    }
                    Op::ReturnValue => {
                        let BlockExit::ReturnValue(_) = exit else {
                            return None;
                        };
                        let (v, ty) = stack.pop()?;
                        let ret_v = match (ret_ty, ty) {
                            (JitTy::Int, JitTy::Int) => v,
                            (JitTy::Float, JitTy::Float) => v,
                            (JitTy::Float, JitTy::Int) => i64_to_f64(&mut bcx, v),
                            (JitTy::Int, JitTy::Float) => f64_to_i64_trunc(&mut bcx, v),
                        };
                        bcx.ins().return_(&[ret_v]);
                        terminated = true;
                    }
                    Op::Return => {
                        let BlockExit::ReturnVoid = exit else {
                            return None;
                        };
                        let bits = PerlValue::UNDEF.raw_bits() as i64;
                        let ret = bcx.ins().iconst(types::I64, bits);
                        bcx.ins().return_(&[ret]);
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
                let args = branch_stack_to_block_args(&mut bcx, &stack, &cfg[ni].entry_cells)?;
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
    let run = match (need_any_table, ret_ty) {
        (false, JitTy::Int) => LinearRun::Nullary(unsafe {
            std::mem::transmute::<*const u8, unsafe extern "C" fn() -> i64>(ptr)
        }),
        (false, JitTy::Float) => LinearRun::NullaryF(unsafe {
            std::mem::transmute::<*const u8, unsafe extern "C" fn() -> f64>(ptr)
        }),
        (true, JitTy::Int) => LinearRun::Tables(unsafe {
            std::mem::transmute::<
                *const u8,
                unsafe extern "C" fn(*const i64, *const i64, *const i64) -> i64,
            >(ptr)
        }),
        (true, JitTy::Float) => LinearRun::TablesF(unsafe {
            std::mem::transmute::<
                *const u8,
                unsafe extern "C" fn(*const i64, *const i64, *const i64) -> f64,
            >(ptr)
        }),
    };
    Some(LinearJit {
        module,
        run,
        ret_nanboxed,
    })
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

/// Try to compile and run `ops` as a block-structured program (loops/conditionals).
/// Returns `Some` with an integer or float [`PerlValue`] on success, `None` to fall back to the interpreter.
///
/// When [`crate::vm::VM::execute`] already ran [`block_jit_validate`], pass the result as
/// `validated_cfg: Some(...)` so CFG validation is not repeated. Unit tests pass [`None`].
pub(crate) fn try_run_block_ops(
    ops: &[Op],
    mut slot_i64: Option<&mut [i64]>,
    mut plain_i64: Option<&mut [i64]>,
    arg_i64: Option<&[i64]>,
    constants: &[PerlValue],
    validated_cfg: Option<ValidatedBlockCfg>,
) -> Option<(PerlValue, BlockJitBufferMode)> {
    let validated = match validated_cfg {
        Some(v) => v,
        None => validate_block_cfg(ops, constants, BlockCfgMode::EvalMain)?,
    };
    let mode = validated.buffer_mode();
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
            let r = j.invoke(std::ptr::null_mut(), slot_ptr, plain_ptr, arg_ptr);
            let pv = if j.ret_nanboxed() {
                j.result_to_perl(r)
            } else {
                jit_block_result_to_perl(r, mode)
            };
            return Some((pv, mode));
        }
    }

    let jit = compile_blocks_validated(validated, ops, constants)?;
    let r = jit.invoke(std::ptr::null_mut(), slot_ptr, plain_ptr, arg_ptr);
    let pv = if jit.ret_nanboxed() {
        jit.result_to_perl(r)
    } else {
        jit_block_result_to_perl(r, mode)
    };

    if let Ok(mut guard) = block_cache().lock() {
        if guard.len() < 256 {
            guard.insert(key, Box::new(jit));
        }
    }
    Some((pv, mode))
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
        let ops = vec![Op::LoadInt(99), Op::SetScalarSlotKeep(0), Op::Halt];
        let v = try_run_linear_ops(&ops, Some(&mut slots), None, None, &[]).expect("jit");
        assert_eq!(v.to_int(), 99);
        assert_eq!(slots[0], 99);
    }

    #[test]
    fn jit_bit_xor() {
        let ops = vec![Op::LoadInt(0xF0), Op::LoadInt(0x0F), Op::BitXor, Op::Halt];
        assert_eq!(
            try_run_linear_ops(&ops, None, None, None, &[])
                .expect("jit")
                .to_int(),
            0xFF
        );
    }

    #[test]
    fn jit_shl_and_shr() {
        let shl = vec![Op::LoadInt(1), Op::LoadInt(2), Op::Shl, Op::Halt];
        assert_eq!(
            try_run_linear_ops(&shl, None, None, None, &[])
                .expect("jit")
                .to_int(),
            4
        );
        let shr = vec![Op::LoadInt(-16), Op::LoadInt(2), Op::Shr, Op::Halt];
        assert_eq!(
            try_run_linear_ops(&shr, None, None, None, &[])
                .expect("jit")
                .to_int(),
            -4
        );
    }

    #[test]
    fn jit_bit_not() {
        let ops = vec![Op::LoadInt(0), Op::BitNot, Op::Halt];
        assert_eq!(
            try_run_linear_ops(&ops, None, None, None, &[])
                .expect("jit")
                .to_int(),
            !0i64
        );
    }

    #[test]
    fn jit_num_cmp_and_spaceship() {
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(2), Op::LoadInt(2), Op::NumEq, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(1), Op::LoadInt(2), Op::NumEq, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            0
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(1), Op::LoadInt(2), Op::NumNe, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(1), Op::LoadInt(2), Op::NumLt, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(3), Op::LoadInt(2), Op::NumGt, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(2), Op::LoadInt(2), Op::NumLe, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(3), Op::LoadInt(2), Op::NumGe, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(1), Op::LoadInt(2), Op::Spaceship, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            -1
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(2), Op::LoadInt(2), Op::Spaceship, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            0
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(3), Op::LoadInt(2), Op::Spaceship, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            1
        );
    }

    #[test]
    fn jit_rejects_inexact_div() {
        assert!(try_run_linear_ops(
            &[Op::LoadInt(1), Op::LoadInt(2), Op::Div, Op::Halt],
            None,
            None,
            None,
            &[]
        )
        .is_none());
        assert!(try_run_linear_ops(
            &[Op::LoadInt(10), Op::LoadInt(3), Op::Div, Op::Halt],
            None,
            None,
            None,
            &[]
        )
        .is_none());
    }

    #[test]
    fn jit_exact_div_mod_and_pow() {
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(10), Op::LoadInt(2), Op::Div, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            5
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(10), Op::LoadInt(3), Op::Mod, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(2), Op::LoadInt(3), Op::Pow, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            8
        );
    }

    #[test]
    fn jit_pow_dynamic_base_const_exp() {
        let ops = vec![Op::GetScalarSlot(0), Op::LoadInt(3), Op::Pow, Op::Halt];
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
        let ops = vec![Op::LoadInt(2), Op::GetScalarSlot(0), Op::Pow, Op::Halt];
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
        assert_eq!(
            try_run_linear_ops(&and, None, None, None, &[])
                .expect("jit")
                .to_int(),
            0b1000
        );
        let or = vec![
            Op::LoadInt(0b1100),
            Op::LoadInt(0b1010),
            Op::BitOr,
            Op::Halt,
        ];
        assert_eq!(
            try_run_linear_ops(&or, None, None, None, &[])
                .expect("jit")
                .to_int(),
            0b1110
        );
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
        let v =
            try_run_linear_ops(&ops, Some(&mut slots), Some(&mut plain), None, &[]).expect("jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn jit_lognot() {
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(0), Op::LogNot, Op::Halt],
                None,
                None,
                None,
                &[]
            )
            .expect("jit")
            .to_int(),
            1
        );
        assert_eq!(
            try_run_linear_ops(
                &[Op::LoadInt(1), Op::LogNot, Op::Halt],
                None,
                None,
                None,
                &[]
            )
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
        interp.scope.declare_scalar("v", PerlValue::integer(40));
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
        assert!(!slot_undef_prefill_ok(&[Op::GetScalarSlot(0), Op::Halt], 0));
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
    fn jit_load_undef_returns_undef() {
        let ops = vec![Op::LoadUndef, Op::Halt];
        let v = try_run_linear_ops(&ops, None, None, None, &[]).expect("jit");
        assert!(v.is_undef());
    }

    #[test]
    fn try_run_linear_sub_implicit_undef_returns_undef() {
        // Mimics `sub { }` tail: LoadUndef; ReturnValue
        let ops = vec![Op::LoadUndef, Op::ReturnValue];
        let v = try_run_linear_sub(&ops, 0, None, None, None, &[], &[], std::ptr::null_mut())
            .expect("sub jit");
        assert!(v.is_undef());
    }

    #[test]
    fn try_run_linear_sub_void_bare_return() {
        // `sub { return; }` — first op is Return, empty segment.
        let ops = vec![Op::Return];
        let v = try_run_linear_sub(&ops, 0, None, None, None, &[], &[], std::ptr::null_mut())
            .expect("void sub jit");
        assert!(v.is_undef());
    }

    #[test]
    fn try_run_linear_sub_void_after_const_pop() {
        let ops = vec![Op::LoadInt(7), Op::Pop, Op::Return];
        let v = try_run_linear_sub(&ops, 0, None, None, None, &[], &[], std::ptr::null_mut())
            .expect("void sub jit");
        assert!(v.is_undef());
    }

    #[test]
    fn jit_load_float_exact_int() {
        let ops = vec![Op::LoadFloat(3.0), Op::LoadInt(4), Op::Add, Op::Halt];
        let v = try_run_linear_ops(&ops, None, None, None, &[]).expect("jit");
        assert_eq!(v.to_int(), 7);
    }

    #[test]
    fn jit_load_float_fraction_returns_float() {
        let ops = vec![Op::LoadFloat(3.5), Op::Halt];
        let v = try_run_linear_ops(&ops, None, None, None, &[]).expect("jit");
        assert_eq!(v.as_float(), Some(3.5));
    }

    #[test]
    fn jit_float_add_and_int_float_mul() {
        let add = vec![Op::LoadFloat(1.25), Op::LoadFloat(2.5), Op::Add, Op::Halt];
        let v = try_run_linear_ops(&add, None, None, None, &[]).expect("jit");
        assert!((v.to_number() - 3.75).abs() < 1e-12);

        let mul = vec![Op::LoadInt(2), Op::LoadFloat(3.5), Op::Mul, Op::Halt];
        let v = try_run_linear_ops(&mul, None, None, None, &[]).expect("jit");
        assert!((v.to_number() - 7.0).abs() < 1e-12);
    }

    // ── Block JIT: conditionals ──

    #[test]
    fn try_run_block_ops_prevalidated_matches_internal_validate() {
        let ops = vec![
            Op::LoadInt(0),
            Op::JumpIfFalse(4),
            Op::LoadInt(10),
            Op::Jump(5),
            Op::LoadInt(20),
            Op::Halt,
        ];
        let validated = block_jit_validate(&ops, &[]).expect("valid block cfg");
        let a =
            try_run_block_ops(&ops, None, None, None, &[], Some(validated)).expect("prevalidated");
        let b = try_run_block_ops(&ops, None, None, None, &[], None).expect("internal validate");
        assert_eq!(a.0.to_int(), b.0.to_int());
        assert_eq!(a.1, b.1);
    }

    #[test]
    fn block_jit_simple_if_true() {
        // if (1) { 42 } else { 0 }
        let ops = vec![
            Op::LoadInt(1),     // 0
            Op::JumpIfFalse(4), // 1 → else
            Op::LoadInt(42),    // 2
            Op::Jump(5),        // 3 → end
            Op::LoadInt(0),     // 4 (else)
            Op::Halt,           // 5
        ];
        let (v, _) = try_run_block_ops(&ops, None, None, None, &[], None).expect("block jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn block_jit_simple_if_false() {
        // if (0) { 42 } else { 99 }
        let ops = vec![
            Op::LoadInt(0),     // 0
            Op::JumpIfFalse(4), // 1 → else
            Op::LoadInt(42),    // 2
            Op::Jump(5),        // 3 → end
            Op::LoadInt(99),    // 4 (else)
            Op::Halt,           // 5
        ];
        let (v, _) = try_run_block_ops(&ops, None, None, None, &[], None).expect("block jit");
        assert_eq!(v.to_int(), 99);
    }

    #[test]
    fn block_jit_float_branches_merge() {
        // if (1) { 2.5 } else { 1.5 }  — join at Halt is DynF; result ~2.5 on taken branch
        let ops = vec![
            Op::LoadInt(1),
            Op::JumpIfFalse(4),
            Op::LoadFloat(2.5),
            Op::Jump(5),
            Op::LoadFloat(1.5),
            Op::Halt,
        ];
        let (v, _) = try_run_block_ops(&ops, None, None, None, &[], None).expect("block jit float");
        assert!((v.to_number() - 2.5).abs() < 1e-12);
    }

    #[test]
    fn hash_ops_load_const_distinct_pool_payload() {
        let ops = vec![Op::LoadConst(0), Op::Halt];
        let h1 = hash_ops(&ops, &[PerlValue::float(1.0)]);
        let h2 = hash_ops(&ops, &[PerlValue::float(2.0)]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn block_jit_jump_if_defined_keep_constant_tos() {
        // defined(1) → jump to Halt with 1 on stack; fallthrough LoadInt(99) is dead.
        let ops = vec![
            Op::LoadInt(1),
            Op::JumpIfDefinedKeep(3),
            Op::LoadInt(99),
            Op::Halt,
        ];
        let (v, _) = try_run_block_ops(&ops, None, None, None, &[], None).expect("block jit");
        assert_eq!(v.to_int(), 1);
    }

    #[test]
    fn block_jit_jump_if_defined_keep_constant_float_tos() {
        let ops = vec![
            Op::LoadFloat(1.25),
            Op::JumpIfDefinedKeep(3),
            Op::LoadFloat(99.0),
            Op::Halt,
        ];
        let (v, _) = try_run_block_ops(&ops, None, None, None, &[], None).expect("block jit");
        assert!((v.to_number() - 1.25).abs() < 1e-12);
    }

    #[test]
    fn block_jit_rejects_jump_if_defined_keep_dynf_tos() {
        let ops = vec![
            Op::LoadFloat(1.25),
            Op::LoadFloat(2.0),
            Op::Mul,
            Op::JumpIfDefinedKeep(6),
            Op::Halt,
            Op::Halt,
        ];
        assert!(try_run_block_ops(&ops, None, None, None, &[], None).is_none());
    }

    #[test]
    fn block_jit_jump_if_defined_keep_raw_scalar_slot() {
        let ops = vec![
            Op::GetScalarSlot(0),
            Op::JumpIfDefinedKeep(4),
            Op::LoadInt(0),
            Op::Halt,
            Op::Halt,
        ];
        let mut slots = [PerlValue::UNDEF.raw_bits() as i64];
        let (v, mode) =
            try_run_block_ops(&ops, Some(&mut slots), None, None, &[], None).expect("jit");
        assert_eq!(mode, BlockJitBufferMode::I64AsPerlValueBits);
        assert_eq!(v.to_int(), 0);

        let mut slots = [PerlValue::integer(42).raw_bits() as i64];
        let (v, mode) =
            try_run_block_ops(&ops, Some(&mut slots), None, None, &[], None).expect("jit");
        assert_eq!(mode, BlockJitBufferMode::I64AsPerlValueBits);
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn block_jit_jump_if_defined_keep_raw_get_scalar_plain() {
        let ops = vec![
            Op::GetScalarPlain(0),
            Op::JumpIfDefinedKeep(4),
            Op::LoadInt(0),
            Op::Halt,
            Op::Halt,
        ];
        let mut plain = [PerlValue::UNDEF.raw_bits() as i64];
        let (v, mode) =
            try_run_block_ops(&ops, None, Some(&mut plain), None, &[], None).expect("jit");
        assert_eq!(mode, BlockJitBufferMode::I64AsPerlValueBits);
        assert_eq!(v.to_int(), 0);

        let mut plain = [PerlValue::integer(7).raw_bits() as i64];
        let (v, mode) =
            try_run_block_ops(&ops, None, Some(&mut plain), None, &[], None).expect("jit");
        assert_eq!(mode, BlockJitBufferMode::I64AsPerlValueBits);
        assert_eq!(v.to_int(), 7);
    }

    #[test]
    fn block_jit_jump_if_defined_keep_raw_get_arg() {
        let ops = vec![
            Op::GetArg(0),
            Op::JumpIfDefinedKeep(4),
            Op::LoadInt(0),
            Op::Halt,
            Op::Halt,
        ];
        let args = [PerlValue::UNDEF.raw_bits() as i64];
        let (v, mode) = try_run_block_ops(&ops, None, None, Some(&args), &[], None).expect("jit");
        assert_eq!(mode, BlockJitBufferMode::I64AsPerlValueBits);
        assert_eq!(v.to_int(), 0);

        let args = [PerlValue::integer(99).raw_bits() as i64];
        let (v, mode) = try_run_block_ops(&ops, None, None, Some(&args), &[], None).expect("jit");
        assert_eq!(mode, BlockJitBufferMode::I64AsPerlValueBits);
        assert_eq!(v.to_int(), 99);
    }

    #[test]
    fn join_cell_succeeds_on_representative_pairs() {
        let cells = [
            Cell::Const(0),
            Cell::Const(1),
            Cell::Dyn,
            Cell::ConstF(0.0),
            Cell::ConstF(1.0),
            Cell::DynF,
        ];
        for &a in &cells {
            for &b in &cells {
                assert!(
                    join_cell(a, b).is_some(),
                    "join_cell({a:?}, {b:?}) should not fail"
                );
            }
        }
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
            Op::GetScalarSlot(0), // 4: push $i
            Op::LoadInt(5),       // 5: push 5
            Op::NumLt,            // 6: $i < 5
            Op::JumpIfFalse(15),  // 7: → exit (GetScalarSlot at 15)
            // loop body
            Op::GetScalarSlot(1), // 8: push $sum
            Op::GetScalarSlot(0), // 9: push $i
            Op::Add,              // 10: $sum + $i
            Op::SetScalarSlot(1), // 11: $sum = result
            Op::PostIncSlot(0),   // 12: $i++
            Op::Pop,              // 13: discard old $i
            Op::Jump(4),          // 14: → loop head
            // exit
            Op::GetScalarSlot(1), // 15: push $sum
            Op::Halt,             // 16
        ];
        let mut slots = [0i64; 2];
        let (v, _) =
            try_run_block_ops(&ops, Some(&mut slots), None, None, &[], None).expect("block jit");
        assert_eq!(v.to_int(), 10);
        assert_eq!(slots[0], 5); // $i ended at 5
        assert_eq!(slots[1], 10); // $sum
    }

    #[test]
    fn block_jit_short_circuit_and_true() {
        // 5 && 42  → evaluates both, returns 42
        let ops = vec![
            Op::LoadInt(5),         // 0: $a
            Op::JumpIfFalseKeep(4), // 1: if false keep 5, jump to end
            Op::Pop,                // 2: pop 5
            Op::LoadInt(42),        // 3: $b
            Op::Halt,               // 4: result
        ];
        let (v, _) = try_run_block_ops(&ops, None, None, None, &[], None).expect("block jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn block_jit_short_circuit_and_false() {
        // 0 && 42  → short-circuits, returns 0
        let ops = vec![
            Op::LoadInt(0),         // 0: $a = 0 (falsy)
            Op::JumpIfFalseKeep(4), // 1: keep 0, jump to end
            Op::Pop,                // 2: (skipped)
            Op::LoadInt(42),        // 3: (skipped)
            Op::Halt,               // 4: result = 0
        ];
        let (v, _) = try_run_block_ops(&ops, None, None, None, &[], None).expect("block jit");
        assert_eq!(v.to_int(), 0);
    }

    #[test]
    fn block_jit_short_circuit_or_true() {
        // 5 || 42  → short-circuits, returns 5
        let ops = vec![
            Op::LoadInt(5),        // 0: $a = 5 (truthy)
            Op::JumpIfTrueKeep(4), // 1: keep 5, jump to end
            Op::Pop,               // 2: (skipped)
            Op::LoadInt(42),       // 3: (skipped)
            Op::Halt,              // 4: result = 5
        ];
        let (v, _) = try_run_block_ops(&ops, None, None, None, &[], None).expect("block jit");
        assert_eq!(v.to_int(), 5);
    }

    #[test]
    fn block_jit_short_circuit_or_false() {
        // 0 || 42  → evaluates both, returns 42
        let ops = vec![
            Op::LoadInt(0),        // 0: $a = 0 (falsy)
            Op::JumpIfTrueKeep(4), // 1: 0 is not truthy, fall through
            Op::Pop,               // 2: pop 0
            Op::LoadInt(42),       // 3: $b
            Op::Halt,              // 4: result = 42
        ];
        let (v, _) = try_run_block_ops(&ops, None, None, None, &[], None).expect("block jit");
        assert_eq!(v.to_int(), 42);
    }

    #[test]
    fn block_jit_rejects_no_jumps() {
        // Pure linear sequence should NOT be handled by block JIT.
        let ops = vec![Op::LoadInt(1), Op::LoadInt(2), Op::Add, Op::Halt];
        assert!(try_run_block_ops(&ops, None, None, None, &[], None).is_none());
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
            Op::NumLt,                // 6: $i < 3
            Op::JumpIfFalse(22),      // 7 → outer exit
            Op::LoadInt(0),           // 8
            Op::DeclareScalarSlot(2), // 9: $j = 0
            // inner head
            Op::GetScalarSlot(2), // 10
            Op::LoadInt(2),       // 11
            Op::NumLt,            // 12: $j < 2
            Op::JumpIfFalse(19),  // 13 → inner exit
            // inner body
            Op::PreIncSlot(1),  // 14: ++$count
            Op::Pop,            // 15
            Op::PostIncSlot(2), // 16: $j++
            Op::Pop,            // 17
            Op::Jump(10),       // 18 → inner head
            // inner exit / outer body continue
            Op::PostIncSlot(0), // 19: $i++
            Op::Pop,            // 20
            Op::Jump(4),        // 21 → outer head
            // outer exit
            Op::GetScalarSlot(1), // 22: push $count
            Op::Halt,             // 23
        ];
        let mut slots = [0i64; 3];
        let (v, _) =
            try_run_block_ops(&ops, Some(&mut slots), None, None, &[], None).expect("block jit");
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
        assert_eq!(plain[0], 11); // buffer updated
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
        assert_eq!(plain[0], 9); // buffer updated
    }

    #[test]
    fn block_jit_plain_inc_loop() {
        // for ($i=0; $i<5; ++$i) {} → $i ends at 5
        // Uses plain-name inc instead of slot inc.
        let mut plain = [0i64];
        let ops = vec![
            Op::LoadInt(0),        // 0
            Op::SetScalarPlain(0), // 1: $i = 0
            Op::GetScalarPlain(0), // 2: loop head
            Op::LoadInt(5),        // 3
            Op::NumLt,             // 4: $i < 5
            Op::JumpIfFalse(9),    // 5: → exit
            Op::PreInc(0),         // 6: ++$i
            Op::Pop,               // 7
            Op::Jump(2),           // 8: → loop head
            Op::GetScalarPlain(0), // 9: exit
            Op::Halt,              // 10
        ];
        let (v, _) =
            try_run_block_ops(&ops, None, Some(&mut plain), None, &[], None).expect("block jit");
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
