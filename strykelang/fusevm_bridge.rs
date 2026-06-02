//! Bridge between strykelang bytecode (`crate::bytecode::Op`) and the shared
//! [`fusevm`] runtime.
//!
//! strykelang is a [`fusevm`] frontend: a conservative, universal-integer subset
//! of its bytecode is translated to `fusevm::Op` and executed on `fusevm::VM`,
//! reusing fusevm's interpreter and three-tier Cranelift JIT for the numeric hot
//! path. Everything fusevm cannot represent (strings, arrays, hashes, regex,
//! closures, calls, AOP, …) stays in strykelang's own VM in [`crate::vm`].
//!
//! Mirrors the awkrs/zshrs integration pattern: each frontend owns its own
//! `Extended(u16, u8)` ID space (see [`ext_ops`]); the universal ops translate
//! 1:1, so jump targets remap by a fixed preamble offset with no re-indexing.
//!
//! This tier sits ahead of strykelang's own linear/block sub-JIT in the dispatch
//! loop: it only fires on segments that sub-JIT would also accept, restricted
//! further to the strict 1:1 universal-integer/slot subset below, so its semantics
//! match the interpreter on the values it accepts.

use crate::bytecode::Op;
use crate::jit::SubTerminator;
use crate::value::StrykeValue;

/// strykelang's reserved `fusevm::Op::Extended(u16, u8)` extension-op ID space.
///
/// fusevm dispatches language-specific opcodes through a per-frontend handler
/// table keyed on the `u16` ID; strykelang owns the `0x0000..` block listed here
/// (zshrs and awkrs own disjoint blocks in their own bridges, so no IDs collide).
/// IDs are reserved here as strykelang's frontend surface; the universal subset
/// that this module currently executes needs none of them, but they pin the ID
/// space so future language-specific ops can be lowered to fusevm without
/// renumbering.
pub mod ext_ops {
    /// Perl-style regex match (`=~`).
    pub const STK_REGEX_MATCH: u16 = 0x0000;
    /// Perl-style regex substitution (`s///`).
    pub const STK_REGEX_SUBST: u16 = 0x0001;
    /// Hash slice (`@h{...}`).
    pub const STK_HASH_SLICE: u16 = 0x0002;
    /// Array slice (`@a[...]`).
    pub const STK_ARRAY_SLICE: u16 = 0x0003;
    /// String interpolation join.
    pub const STK_INTERP_JOIN: u16 = 0x0004;
    /// Wantarray context query.
    pub const STK_WANTARRAY: u16 = 0x0005;
    /// Perl/strykelang *floored* modulo (`%`). Lowered to `fusevm::Op::Extended`
    /// and JIT-compiled by [`super::StrykeJitExt`]; the floored semantics (result
    /// takes the sign of the divisor) differ from fusevm's truncated `Op::Mod`,
    /// which is why `%` is its own extension op rather than the universal `Mod`.
    pub const STK_MOD_FLOOR: u16 = 0x0006;

    /// String comparison ops (`eq ne lt gt le ge cmp`). Each lowers to an
    /// `Extended` op whose JIT/interpreter handler reconstructs the two operand
    /// `StrykeValue`s from their NaN-boxed bits (passed as i64 slot handles) and
    /// defers to strykelang's native [`crate::value::StrykeValue::str_eq`] /
    /// [`crate::value::StrykeValue::str_cmp`], so grapheme/Unicode semantics are
    /// preserved exactly. Results are `i64` (0/1, or -1/0/1 for `cmp`), which keeps
    /// the chunk on fusevm's integer block JIT + on-disk native cache.
    pub const STK_STR_EQ: u16 = 0x0007;
    /// String inequality (`ne`).
    pub const STK_STR_NE: u16 = 0x0008;
    /// String less-than (`lt`).
    pub const STK_STR_LT: u16 = 0x0009;
    /// String greater-than (`gt`).
    pub const STK_STR_GT: u16 = 0x000A;
    /// String less-or-equal (`le`).
    pub const STK_STR_LE: u16 = 0x000B;
    /// String greater-or-equal (`ge`).
    pub const STK_STR_GE: u16 = 0x000C;
    /// String three-way compare (`cmp`); yields -1/0/1.
    pub const STK_STR_CMP: u16 = 0x000D;

    /// String concatenation (`.`). Unlike the compare ops (which return an `i64`
    /// boolean/ordering), this *allocates* a new heap string and returns its raw
    /// NaN-boxed bits as an `i64` handle. The freshly-built `StrykeValue` is
    /// `mem::forget`-ed by the helper so its `Arc` ownership transfers into that
    /// handle; the bridge reconstructs an owning `StrykeValue` from the returned
    /// bits (see [`super::run_linear_segment`]). Only routed when both operands are
    /// plain strings (`is_string_like`), so the byte concatenation is identical to
    /// the interpreter and never triggers `use overload`/stringify side effects.
    pub const STK_STR_CONCAT: u16 = 0x000E;

    /// First ID reserved for strykelang; frontends must keep their blocks disjoint.
    pub const STK_ID_BASE: u16 = STK_REGEX_MATCH;
}

/// `PushFrame` + `slot_count` × (`LoadInt` + `SetSlot`) — the preamble emitted
/// before a translated body so fusevm slots start from the marshaled values.
/// When `seed_slots` is false (string-comparison segments) the marshaled slot
/// values are supplied to the block JIT through its runtime `slots` buffer rather
/// than baked into the chunk, so the only preamble op is `PushFrame`. This keeps
/// the chunk's `op_hash` independent of the (per-run, non-deterministic) operand
/// pointer handles, which is what makes those chunks safely disk-cacheable.
#[inline]
fn preamble_len(slot_count: usize, seed_slots: bool) -> usize {
    if seed_slots {
        1 + slot_count * 2
    } else {
        1
    }
}

/// Perl/strykelang *floored* modulo: the result takes the sign of the divisor
/// (`-7 % 3 == 2`), unlike Rust/fusevm's truncated `%`. `b ∈ {0, -1}` is guarded
/// to avoid a divide trap; both yield `0` (mathematically for `-1`, and a safe
/// non-trapping placeholder for the erroneous `% 0` path, which the strykelang
/// interpreter rejects separately on the non-JIT path).
#[inline]
pub(crate) fn floored_mod(a: i64, b: i64) -> i64 {
    if b == 0 || b == -1 {
        return 0;
    }
    let r = a % b;
    if r != 0 && ((r ^ b) < 0) {
        r + b
    } else {
        r
    }
}

/// Reconstruct a borrowed view of a `StrykeValue` from its raw NaN-boxed bits
/// (passed across the JIT/interpreter boundary as an i64 slot handle) **without
/// taking ownership**: the value is owned by the caller's scope, so it must not
/// be dropped here (that would decrement the shared `Arc`). `ManuallyDrop`
/// guarantees no drop runs.
#[inline]
unsafe fn sv_borrow(bits: i64) -> std::mem::ManuallyDrop<StrykeValue> {
    std::mem::ManuallyDrop::new(StrykeValue(bits as u64))
}

/// Shared comparator for every `STK_STR_*` op. Reconstructs both operands from
/// their NaN-boxed bits and defers to strykelang's native string comparison so
/// grapheme/Unicode and stringification semantics match the interpreter exactly.
/// Returns 0/1 for the boolean ops and -1/0/1 for `cmp`.
#[inline]
fn stryke_str_compare_op(ext_id: u16, a_bits: i64, b_bits: i64) -> i64 {
    use std::cmp::Ordering;
    let (a, b) = unsafe { (sv_borrow(a_bits), sv_borrow(b_bits)) };
    match ext_id {
        ext_ops::STK_STR_EQ => a.str_eq(&b) as i64,
        ext_ops::STK_STR_NE => (!a.str_eq(&b)) as i64,
        ext_ops::STK_STR_LT => (a.str_cmp(&b) == Ordering::Less) as i64,
        ext_ops::STK_STR_GT => (a.str_cmp(&b) == Ordering::Greater) as i64,
        ext_ops::STK_STR_LE => {
            matches!(a.str_cmp(&b), Ordering::Less | Ordering::Equal) as i64
        }
        ext_ops::STK_STR_GE => {
            matches!(a.str_cmp(&b), Ordering::Greater | Ordering::Equal) as i64
        }
        ext_ops::STK_STR_CMP => match a.str_cmp(&b) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        },
        _ => 0,
    }
}

macro_rules! stryke_str_helper {
    ($name:ident, $id:expr) => {
        /// `extern "C"` host helper invoked from JIT-compiled code via
        /// `ExtJitCtx::call_host`. ABI: `(i64 a_bits, i64 b_bits) -> i64`.
        extern "C" fn $name(a: i64, b: i64) -> i64 {
            stryke_str_compare_op($id, a, b)
        }
    };
}
stryke_str_helper!(stryke_h_str_eq, ext_ops::STK_STR_EQ);
stryke_str_helper!(stryke_h_str_ne, ext_ops::STK_STR_NE);
stryke_str_helper!(stryke_h_str_lt, ext_ops::STK_STR_LT);
stryke_str_helper!(stryke_h_str_gt, ext_ops::STK_STR_GT);
stryke_str_helper!(stryke_h_str_le, ext_ops::STK_STR_LE);
stryke_str_helper!(stryke_h_str_ge, ext_ops::STK_STR_GE);
stryke_str_helper!(stryke_h_str_cmp, ext_ops::STK_STR_CMP);

/// Concatenate two plain-string operands (`$a . $b`), returning the raw bits of a
/// **newly allocated, owned** `StrykeValue::string`. The two operands are borrowed
/// (never dropped — they stay owned by the caller's slots); the result's `Arc`
/// ownership is transferred into the returned handle via `mem::forget`, so the
/// caller must reconstitute exactly one owning `StrykeValue` from these bits
/// (`StrykeValue::from_raw_bits`) — which [`run_linear_segment`] does. Eligibility
/// guarantees both operands are plain strings, matching the interpreter's
/// byte-level `push_str` without any `use overload`/stringify side effects.
#[inline]
fn stryke_str_concat_op(a_bits: i64, b_bits: i64) -> i64 {
    let (a, b) = unsafe { (sv_borrow(a_bits), sv_borrow(b_bits)) };
    let mut s = a.as_str().unwrap_or_default();
    s.push_str(&b.as_str().unwrap_or_default());
    let new = StrykeValue::string(s);
    let bits = new.raw_bits() as i64;
    // Transfer the new value's Arc ownership into the returned handle; the caller
    // reconstructs a single owning StrykeValue, so there is no leak/double-free.
    std::mem::forget(new);
    bits
}

/// `extern "C"` host helper for `STK_STR_CONCAT`. ABI matches the compare helpers
/// (`(i64, i64) -> i64`) so it shares the same `call_host`/relocation machinery.
extern "C" fn stryke_h_str_concat(a: i64, b: i64) -> i64 {
    stryke_str_concat_op(a, b)
}

/// Table mapping each string-compare `Extended` id to the stable host-helper
/// symbol name and its function pointer. The name is hashed into a JIT helper id
/// (`fusevm::jit::jit_helper_id`) so cached native code re-resolves the helper at
/// load time; the pointer is registered once via `register_jit_helper`.
const STRYKE_STR_HELPERS: &[(u16, &str, extern "C" fn(i64, i64) -> i64)] = &[
    (ext_ops::STK_STR_EQ, "stryke_str_eq", stryke_h_str_eq),
    (ext_ops::STK_STR_NE, "stryke_str_ne", stryke_h_str_ne),
    (ext_ops::STK_STR_LT, "stryke_str_lt", stryke_h_str_lt),
    (ext_ops::STK_STR_GT, "stryke_str_gt", stryke_h_str_gt),
    (ext_ops::STK_STR_LE, "stryke_str_le", stryke_h_str_le),
    (ext_ops::STK_STR_GE, "stryke_str_ge", stryke_h_str_ge),
    (ext_ops::STK_STR_CMP, "stryke_str_cmp", stryke_h_str_cmp),
    (ext_ops::STK_STR_CONCAT, "stryke_str_concat", stryke_h_str_concat),
];

/// True for any `STK_STR_*` extension id (comparisons + concatenation).
#[inline]
fn is_stryke_str_ext(ext_id: u16) -> bool {
    (ext_ops::STK_STR_EQ..=ext_ops::STK_STR_CONCAT).contains(&ext_id)
}

/// The registered JIT helper id for a string-compare extension op, if any.
#[inline]
fn stryke_str_helper_id(ext_id: u16) -> Option<u32> {
    STRYKE_STR_HELPERS
        .iter()
        .find(|(id, _, _)| *id == ext_id)
        .map(|(_, name, _)| fusevm::jit::jit_helper_id(name))
}

/// fusevm JIT extension that lowers strykelang's `Op::Mod` (re-emitted as
/// `fusevm::Op::Extended(STK_MOD_FLOOR, 0)`) to native floored-modulo code, plus
/// the `STK_STR_*` string-comparison ops (which call host helpers, kept cacheable
/// by fusevm's relocation-replay), keeping them on the block JIT + on-disk cache.
pub(crate) struct StrykeJitExt;
impl fusevm::jit::JitExtension for StrykeJitExt {
    fn can_jit(&self, ext_id: u16) -> bool {
        ext_id == ext_ops::STK_MOD_FLOOR || is_stryke_str_ext(ext_id)
    }
    fn op_count(&self) -> usize {
        1 + STRYKE_STR_HELPERS.len()
    }
    fn name(&self) -> &str {
        "strykelang"
    }
    fn emit_extended(&self, ext_id: u16, _arg: u8, cx: &mut fusevm::jit::ExtJitCtx) -> bool {
        if is_stryke_str_ext(ext_id) {
            // String compares: pop the two operand handles (raw StrykeValue bits)
            // and emit a call to the registered host helper, which defers to
            // strykelang's native str_eq/str_cmp. `call_host` records the helper
            // relocation so the chunk stays on-disk cacheable.
            let Some(helper_id) = stryke_str_helper_id(ext_id) else {
                return false;
            };
            let (Some(b), Some(a)) = (cx.pop_i64(), cx.pop_i64()) else {
                return false;
            };
            let Some(result) = cx.call_host(helper_id, &[a, b]) else {
                return false;
            };
            cx.push_i64(result);
            return true;
        }
        if ext_id != ext_ops::STK_MOD_FLOOR {
            return false;
        }
        // floored(a, b): truncated remainder, adjusted by +b when non-zero and
        // its sign differs from the divisor; b ∈ {0, -1} guarded to dodge traps.
        let (Some(b), Some(a)) = (cx.pop_i64(), cx.pop_i64()) else {
            return false;
        };
        let zero = cx.iconst(0);
        let one = cx.iconst(1);
        let neg1 = cx.iconst(-1);
        let is_zero = cx.icmp_eq(b, zero);
        let is_neg1 = cx.icmp_eq(b, neg1);
        let special = cx.bor(is_zero, is_neg1);
        let safe_b = cx.select(special, one, b);
        let t = cx.srem(a, safe_b);
        let xor = cx.bxor(t, safe_b);
        let signs_differ = cx.icmp_slt(xor, zero);
        let t_nonzero = cx.icmp_ne(t, zero);
        let need = cx.band(signs_differ, t_nonzero);
        let adj = cx.select(need, safe_b, zero);
        let floored = cx.iadd(t, adj);
        let result = cx.select(special, zero, floored);
        cx.push_i64(result);
        true
    }
}

/// Register strykelang's JIT extension (idempotent, process-wide) so the block
/// JIT can compile `Op::Extended(STK_MOD_FLOOR, …)`. Must run before the first
/// modulo segment is judged for eligibility.
pub(crate) fn register_stryke_jit_ext() {
    fusevm::jit::register_global_extension(std::sync::Arc::new(StrykeJitExt));
    // Register the string-compare host helpers (idempotent, process-global) so
    // both freshly-JITted and disk-cache-reloaded chunks can resolve them.
    for (_, name, ptr) in STRYKE_STR_HELPERS {
        // SAFETY: each helper is `extern "C" fn(i64, i64) -> i64`, matching the
        // 2-arg / integer-return signature declared to the JIT.
        unsafe {
            fusevm::jit::register_jit_helper(name, *ptr as *const u8, 2, false);
        }
    }
}

/// fusevm interpreter handler for strykelang Extended ops, registered on the
/// fallback `fusevm::VM` so the cold (pre-warmup) path computes the same result
/// the JIT does.
fn stryke_ext_handler(vm: &mut fusevm::VM, id: u16, _arg: u8) {
    if id == ext_ops::STK_MOD_FLOOR {
        let b = vm.pop().to_int();
        let a = vm.pop().to_int();
        vm.push(fusevm::Value::Int(floored_mod(a, b)));
    } else if is_stryke_str_ext(id) {
        let b = vm.pop().to_int();
        let a = vm.pop().to_int();
        let r = if id == ext_ops::STK_STR_CONCAT {
            stryke_str_concat_op(a, b)
        } else {
            stryke_str_compare_op(id, a, b)
        };
        vm.push(fusevm::Value::Int(r));
    }
}


/// True when every op in `seg` is in the strict 1:1 universal-integer/slot subset
/// that translates to exactly one `fusevm::Op`, and every jump target stays inside
/// the segment `[seg_start, seg_start + seg.len()]`.
///
/// Restricting to a 1:1 mapping keeps jump remapping a fixed affine shift (the
/// preamble length) with no instruction-index rewriting. Anything outside the set
/// (floats, constants, name/arg reads, declarations, calls, arrays, hashes, fused
/// opcodes, keep-variant jumps, void-context slot mutations) makes the segment
/// ineligible and the caller falls back to strykelang's own JIT/interpreter.
///
/// `Op::Mod` is lowered to `fusevm::Op::Extended(STK_MOD_FLOOR, 0)` rather than
/// fusevm's universal `Op::Mod`, because strykelang/Perl `%` is *floored* (result
/// takes the sign of the divisor, e.g. `-7 % 3 == 2`) whereas fusevm's `Mod` is
/// truncated i64 `%` (`-7 % 3 == -1`). The extension ([`StrykeJitExt`]) JITs the
/// floored form, and [`stryke_ext_handler`] mirrors it on the interpreter
/// fallback. `Op::Div`/`Op::Pow` stay eligible but always yield a float, so
/// [`segment_result_is_integer`] routes any segment containing them through
/// fusevm's interpreter rather than the integer block JIT.
pub(crate) fn segment_is_fusevm_eligible(seg: &[Op], seg_start: usize) -> bool {
    if seg.is_empty() {
        return false;
    }
    let seg_end = seg_start + seg.len();
    for op in seg {
        match op {
            Op::LoadInt(_)
            | Op::Pop
            | Op::Dup
            | Op::Dup2
            | Op::Swap
            | Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::Mod
            | Op::Pow
            | Op::Negate
            | Op::NumEq
            | Op::NumNe
            | Op::NumLt
            | Op::NumGt
            | Op::NumLe
            | Op::NumGe
            | Op::Spaceship
            | Op::LogNot
            | Op::Inc
            | Op::Dec
            | Op::BitAnd
            | Op::BitOr
            | Op::BitXor
            | Op::BitNot
            | Op::Shl
            | Op::Shr
            | Op::GetScalarSlot(_)
            | Op::SetScalarSlot(_)
            | Op::SetScalarSlotKeep(_)
            | Op::DeclareScalarSlot(_, _)
            | Op::AddAssignSlotSlot(_, _)
            | Op::SubAssignSlotSlot(_, _)
            | Op::MulAssignSlotSlot(_, _)
            | Op::AddAssignSlotSlotVoid(_, _)
            | Op::PreIncSlot(_)
            | Op::PreDecSlot(_)
            | Op::PostIncSlot(_)
            | Op::PostDecSlot(_)
            | Op::PreIncSlotVoid(_)
            | Op::AccumSumLoop(_, _, _) => continue,
            Op::Jump(t) | Op::JumpIfTrue(t) | Op::JumpIfFalse(t) => {
                if *t < seg_start || *t > seg_end {
                    return false;
                }
            }
            // Fused loop ops carry an in-segment jump target (top check / backedge).
            Op::SlotLtIntJumpIfFalse(_, _, t) | Op::SlotIncLtIntJumpBack(_, _, t) => {
                if *t < seg_start || *t > seg_end {
                    return false;
                }
            }
            _ => return false,
        }
    }
    segment_block_stack_is_consistent(seg, seg_start)
}

/// Lightweight operand kind for the linear float-segment type propagation. fusevm
/// promotes mixed int/float arithmetic to float, so the bridge must know each
/// operand's kind to (a) refuse storing a float into an integer-marshaled slot and
/// (b) know whether the segment's result is an integer/bool (the only kind the
/// block JIT can return today — see [`segment_is_fusevm_float_eligible`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NumTy {
    Int,
    Float,
}

/// Apply a single straight-line (non-control-flow) op to the abstract operand-type
/// stack used by [`segment_is_fusevm_float_eligible`], returning `false` if the op is
/// not float-safe or underflows. Control-flow ops are handled by the caller.
fn float_apply_op(op: &Op, stack: &mut Vec<NumTy>) -> bool {
    use Op::*;
    match op {
        LoadInt(_) => stack.push(NumTy::Int),
        LoadFloat(_) => stack.push(NumTy::Float),
        // Runtime slots are marshaled as `i64` integers (a float arg bails to the
        // interpreter before we get here), so a slot read is always an integer.
        GetScalarSlot(_) => stack.push(NumTy::Int),
        Pop => {
            if stack.pop().is_none() {
                return false;
            }
        }
        Dup => match stack.last() {
            Some(&t) => stack.push(t),
            None => return false,
        },
        Dup2 => {
            let n = stack.len();
            if n < 2 {
                return false;
            }
            let (a, b) = (stack[n - 2], stack[n - 1]);
            stack.push(a);
            stack.push(b);
        }
        Swap => {
            let n = stack.len();
            if n < 2 {
                return false;
            }
            stack.swap(n - 1, n - 2);
        }
        Add | Sub | Mul => {
            let (b, a) = match (stack.pop(), stack.pop()) {
                (Some(b), Some(a)) => (b, a),
                _ => return false,
            };
            stack.push(if a == NumTy::Float || b == NumTy::Float {
                NumTy::Float
            } else {
                NumTy::Int
            });
        }
        // strykelang `/` is *always* float division (even `4 / 2 == 2.0`), and it
        // lowers to fusevm `Op::AwkDivJit`, which divides in floating point in both
        // the block JIT (`fdiv` over operands coerced to f64) and the interpreter,
        // and persists to the native disk cache. The result is therefore always a
        // float, independent of operand kinds. A zero divisor is handled out of
        // band (the JIT traps and the caller declines so strykelang's own
        // "Illegal division by zero" is raised), so it does not affect typing here.
        Div => {
            if stack.pop().is_none() || stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Float);
        }
        // strykelang `**` is *always* float exponentiation (even `2 ** 10 ==
        // 1024.0`), and it lowers to fusevm `Op::PowFloat`, which raises to a
        // power in floating point (operands coerced to f64 via the `pow_f64`
        // host helper) in both the block JIT and the interpreter, and persists
        // to the native disk cache. The result is therefore always a float,
        // independent of operand kinds.
        Pow => {
            if stack.pop().is_none() || stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Float);
        }
        // Unary negate preserves the operand's kind.
        Negate => {
            if stack.last().is_none() {
                return false;
            }
        }
        NumEq | NumNe | NumLt | NumGt | NumLe | NumGe | Spaceship => {
            if stack.pop().is_none() || stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Int);
        }
        LogNot => {
            if stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Int);
        }
        // Storing into an `i64`-marshaled slot: the stored value must be an integer,
        // else it would be silently truncated on the next read.
        SetScalarSlot(_) | DeclareScalarSlot(_, _) => {
            if stack.pop() != Some(NumTy::Int) {
                return false;
            }
        }
        SetScalarSlotKeep(_) => {
            if stack.last() != Some(&NumTy::Int) {
                return false;
            }
        }
        // Integer-only slot superinstructions: they neither consume nor produce a
        // float operand (slots are integers).
        AddAssignSlotSlotVoid(_, _) | PreIncSlotVoid(_) | AccumSumLoop(_, _, _) => {}
        AddAssignSlotSlot(_, _) | SubAssignSlotSlot(_, _) | MulAssignSlotSlot(_, _)
        | PreIncSlot(_) | PreDecSlot(_) | PostIncSlot(_) | PostDecSlot(_) => {
            stack.push(NumTy::Int)
        }
        // Everything else — `Mod`, bit/shift ops, `Inc`/`Dec`, fused-loop
        // ops, string ops, plain-slot reads — is not float-safe here.
        _ => return false,
    }
    true
}

/// Whether `seg` is eligible for the *float-operand* block-JIT path: it contains at
/// least one `LoadFloat` (otherwise the plain integer path in
/// [`segment_is_fusevm_eligible`] already covers it), every op's fusevm lowering is
/// semantically identical to strykelang for both integer **and** float operands,
/// **and** the segment's single result is an integer/bool.
///
/// The integer-result restriction is the key correctness constraint: fusevm's block
/// JIT returns its top-of-stack as an `i64`, truncating a float result to an integer
/// (`scalar_store_i64`). So only segments whose result is an integer/bool — e.g. a
/// float comparison `$x < 0.5`, `$a >= 1.5`, a `!`/logical test over them, or a
/// ternary `$x < 0.5 ? 1 : 0` whose branches both yield integers — may take this
/// path; the comparison itself runs in floating point (fusevm promotes the operands)
/// and yields the exact 0/1 the interpreter would.
///
/// This propagates the abstract operand *type* stack along every control-flow edge
/// (`Jump`/`JumpIfTrue`/`JumpIfFalse`, targets resolved relative to `seg_start`) and
/// bails on any disagreement at a control-flow merge — both the stack *depth* and the
/// per-slot Int/Float *kind* must match across all incoming edges — so the type the
/// block JIT actually computes is exactly the one analysed here. (fusevm independently
/// bails to the interpreter on a block-param type mismatch, so a missed disagreement
/// degrades to the interpreter rather than miscompiling.)
///
/// Admitted: `LoadInt`/`LoadFloat`, stack shuffles, `+ - *`, unary negate, the six
/// numeric comparisons, `Spaceship`, `LogNot`, the integer-only slot ops (slots are
/// marshaled as `i64`), and `Jump`/`JumpIf{True,False}`. Rejected: `Div`/`Pow`
/// (strykelang's `/` and `**` are always float, but fusevm's `Op::Div` is integer
/// division on two ints), `Mod`, every bit/shift op and `Inc`/`Dec` (unsafe on a
/// float operand), the fused-loop ops, and storing a float into a slot.
pub(crate) fn segment_is_fusevm_float_eligible(seg: &[Op], seg_start: usize) -> bool {
    segment_fusevm_float_result_kind(seg, seg_start).is_some()
}

/// Core of [`segment_is_fusevm_float_eligible`]: returns the segment's single
/// numeric result kind (`Int` or `Float`) when the float-operand block-JIT path
/// applies, or `None` if the segment is ineligible (not exactly one result, an
/// unmodelable/float-unsafe op, a stack underflow, or a control-flow merge
/// disagreement).
///
/// A `Float` result is now JIT-eligible: fusevm's block tier returns it as the
/// raw `f64` bit pattern (via [`fusevm::JitCompiler::try_run_block_typed_kinded`])
/// rather than truncating it, so the caller reconstructs the exact float. Float
/// stores into `i64`-marshaled slots remain rejected by `float_apply_op`.
pub(crate) fn segment_fusevm_float_result_kind(seg: &[Op], seg_start: usize) -> Option<NumTy> {
    use Op::*;
    // The float-operand path applies to a segment that either carries an explicit
    // float literal (`LoadFloat`), performs strykelang's always-float division
    // (`Div`, lowered to fusevm `Op::AwkDivJit`), or always-float exponentiation
    // (`Pow`, lowered to fusevm `Op::PowFloat`). A segment with none of these is
    // pure integer and is already covered by `segment_is_fusevm_eligible`.
    if !seg.iter().any(|o| matches!(o, LoadFloat(_) | Div | Pow)) {
        return None;
    }
    let n = seg.len();
    // Resolve an absolute jump target to a segment-relative index in `0..=n`
    // (`n` is the implicit end / return point).
    let rel = |t: usize| -> Option<usize> { t.checked_sub(seg_start).filter(|r| *r <= n) };

    // Abstract operand-type stack at the *entry* of each ip (0..=n); `None` = not yet
    // reached. A merge requires the incoming stack to equal the recorded one.
    let mut state: Vec<Option<Vec<NumTy>>> = vec![None; n + 1];
    state[0] = Some(Vec::new());
    let mut work: Vec<usize> = vec![0];

    // Merge an incoming type-stack into `target`, queueing it on first arrival and
    // rejecting any later mismatch.
    let merge = |state: &mut Vec<Option<Vec<NumTy>>>,
                     work: &mut Vec<usize>,
                     target: usize,
                     incoming: &[NumTy]|
     -> bool {
        match &state[target] {
            None => {
                state[target] = Some(incoming.to_vec());
                work.push(target);
                true
            }
            Some(existing) => existing.as_slice() == incoming,
        }
    };

    while let Some(ip) = work.pop() {
        if ip >= n {
            continue;
        }
        let mut stk = state[ip].clone().expect("queued ip has known state");
        match &seg[ip] {
            Jump(t) => {
                let Some(tr) = rel(*t) else { return None };
                if !merge(&mut state, &mut work, tr, &stk) {
                    return None;
                }
            }
            JumpIfTrue(t) | JumpIfFalse(t) => {
                // Both branches pop the condition; the kind is irrelevant.
                if stk.pop().is_none() {
                    return None;
                }
                let Some(tr) = rel(*t) else { return None };
                if !merge(&mut state, &mut work, tr, &stk)
                    || !merge(&mut state, &mut work, ip + 1, &stk)
                {
                    return None;
                }
            }
            other => {
                if !float_apply_op(other, &mut stk) {
                    return None;
                }
                if !merge(&mut state, &mut work, ip + 1, &stk) {
                    return None;
                }
            }
        }
    }

    // The result is the operand-stack state at the implicit return point: exactly
    // one value, of either kind (the block JIT can now return both).
    match &state[n] {
        Some(s) if s.len() == 1 => Some(s[0]),
        _ => None,
    }
}

/// Per-op operand-stack depth change for the ops that may appear in an eligible
/// integer/string segment. `None` for any op we don't model (forces the caller to
/// treat the segment as ineligible). Mirrors the lowering in [`translate_op_into`].
fn op_stack_delta(op: &Op) -> Option<i32> {
    use Op::*;
    Some(match op {
        LoadInt(_) | Dup | GetScalarSlot(_) | GetScalarPlain(_) => 1,
        Dup2 => 2,
        Pop | SetScalarSlot(_) | DeclareScalarSlot(_, _) => -1,
        Swap | SetScalarSlotKeep(_) | Negate | BitNot | LogNot | Inc | Dec
        | PreIncSlotVoid(_) | AddAssignSlotSlotVoid(_, _) | AccumSumLoop(_, _, _)
        | SlotLtIntJumpIfFalse(_, _, _) | SlotIncLtIntJumpBack(_, _, _) | Jump(_) => 0,
        Add | Sub | Mul | Div | Mod | Pow | BitAnd | BitOr | BitXor | Shl | Shr
        | NumEq | NumNe | NumLt | NumGt | NumLe | NumGe | Spaceship
        | StrEq | StrNe | StrLt | StrGt | StrLe | StrGe | StrCmp | Concat
        | JumpIfTrue(_) | JumpIfFalse(_) => -1,
        AddAssignSlotSlot(_, _) | SubAssignSlotSlot(_, _) | MulAssignSlotSlot(_, _)
        | PreIncSlot(_) | PreDecSlot(_) | PostIncSlot(_) | PostDecSlot(_) => 1,
        _ => return None,
    })
}

/// fusevm's block JIT carries operand-stack values that are live across a basic-block
/// boundary as Cranelift block parameters (typed by the value), so a ternary `?:` /
/// `if`-expression result that merges at a join point — or is carried by a
/// value-bearing jump to the implicit return point — survives the merge and is JIT
/// (and disk-cache) eligible. The one requirement is that the operand-stack **depth**
/// at every control-flow merge is consistent across all incoming edges (fusevm fixes
/// each block's parameter arity at its first predecessor; a divergent arity would make
/// it bail to the interpreter). Counted/accumulator loops trivially satisfy this (their
/// cross-iteration state lives in slots and every boundary is reached at depth 0).
///
/// This propagates operand-stack depth along every control-flow edge and returns
/// `false` only on a genuinely malformed segment: an inconsistent merge depth, an
/// unmodelable op, or a stack underflow. (Type consistency across a merge is enforced
/// by fusevm itself, which safely bails to the interpreter on a mismatch rather than
/// miscompiling, so it need not be re-checked here.)
fn segment_block_stack_is_consistent(seg: &[Op], seg_start: usize) -> bool {
    let n = seg.len();
    // Resolve an absolute jump target to a segment-relative index in `0..=n`
    // (`n` is the implicit end / return point).
    let rel = |t: usize| -> Option<usize> { t.checked_sub(seg_start).filter(|r| *r <= n) };

    let mut depth_in: Vec<Option<i32>> = vec![None; n + 1];
    depth_in[0] = Some(0);
    let mut work: Vec<usize> = vec![0];
    while let Some(ip) = work.pop() {
        if ip >= n {
            continue;
        }
        let d = depth_in[ip].expect("queued ip has known depth");
        let delta = match op_stack_delta(&seg[ip]) {
            Some(v) => v,
            None => return false,
        };
        let after = d + delta;
        if after < 0 {
            return false;
        }
        // Successors: (target_index, reached_via_jump).
        let mut succs: Vec<(usize, bool)> = Vec::new();
        match &seg[ip] {
            Op::Jump(t) => match rel(*t) {
                Some(r) => succs.push((r, true)),
                None => return false,
            },
            Op::JumpIfTrue(t)
            | Op::JumpIfFalse(t)
            | Op::SlotLtIntJumpIfFalse(_, _, t)
            | Op::SlotIncLtIntJumpBack(_, _, t) => {
                match rel(*t) {
                    Some(r) => succs.push((r, true)),
                    None => return false,
                }
                succs.push((ip + 1, false));
            }
            _ => succs.push((ip + 1, false)),
        }
        for (s, _via_jump) in succs {
            // Every merge (including the implicit end point `n`) must be reached at a
            // single consistent operand-stack depth across all incoming edges.
            match depth_in[s] {
                Some(prev) => {
                    if prev != after {
                        return false;
                    }
                }
                None => {
                    depth_in[s] = Some(after);
                    work.push(s);
                }
            }
        }
    }
    true
}

/// True when `seg` is a pure string-comparison body: only slot reads feeding the
/// `eq ne lt gt le ge cmp` ops (plus boolean `LogNot` / stack shuffles), with at
/// least one such op present. These segments are marshaled with **raw NaN-boxed
/// `StrykeValue` bits** as slot handles (rather than unboxed integers), so they
/// are kept strictly disjoint from the integer subset: no arithmetic or numeric
/// op may appear, since those would misinterpret the handle bits.
pub(crate) fn segment_is_string_compare_eligible(seg: &[Op], seg_start: usize) -> bool {
    if seg.is_empty() {
        return false;
    }
    let seg_end = seg_start + seg.len();
    let mut has_str_op = false;
    for op in seg {
        match op {
            Op::StrEq | Op::StrNe | Op::StrLt | Op::StrGt | Op::StrLe | Op::StrGe
            | Op::StrCmp => has_str_op = true,
            Op::GetScalarSlot(_) | Op::LogNot | Op::Pop | Op::Dup => {}
            Op::Jump(t) | Op::JumpIfTrue(t) | Op::JumpIfFalse(t) => {
                if *t < seg_start || *t > seg_end {
                    return false;
                }
            }
            _ => return false,
        }
    }
    has_str_op && segment_block_stack_is_consistent(seg, seg_start)
}

/// True when `seg` is exactly a single string concatenation `$x . $y` of two slot
/// operands: `[GetScalarSlot, GetScalarSlot, Concat]`. This deliberately strict
/// shape is what makes the allocating concat safe to lower:
///
/// * Both operands are borrowed slot handles (never dropped by the JIT body), so
///   the caller's `Arc`s are untouched.
/// * The single `Concat` produces exactly one *owned* result handle, returned as
///   the segment value and reconstituted into one owning `StrykeValue` by
///   [`run_linear_segment`]. There are no stack shuffles (`Dup`/`Pop`/`Swap`) and
///   no chained concat, so there is never an owned *intermediate* handle that
///   could leak or be double-freed.
///
/// Like the compare segments, operands are marshaled as raw NaN-boxed bits and the
/// chunk is built unseeded, so its `op_hash` (the disk-cache key) is independent of
/// the per-run operand pointers.
pub(crate) fn segment_is_string_concat_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(
        seg,
        [Op::GetScalarSlot(_), Op::GetScalarSlot(_), Op::Concat]
    )
}

/// Translate a single eligible strykelang op, appending the equivalent
/// `fusevm::Op`(s) to `body`. Most ops are 1:1; strykelang's fused slot ops are
/// decomposed into fusevm's primitive slot/arith ops (fusevm has no slot-slot
/// arith-assign other than `AddAssignSlotVoid`). Jump ops are emitted with a
/// placeholder target and recorded in `fixups` as `(body_index, src_target_idx)`
/// where `src_target_idx = absolute_target - seg_start`; [`build_chunk`] resolves
/// them after the body length per source op is known.
///
/// Returns `false` if `op` is unsupported.
fn translate_op_into(
    op: &Op,
    seg_start: usize,
    body: &mut Vec<fusevm::Op>,
    fixups: &mut Vec<(usize, usize)>,
) -> bool {
    use fusevm::Op as F;
    match op {
        Op::LoadInt(n) => body.push(F::LoadInt(*n)),
        Op::LoadFloat(f) => body.push(F::LoadFloat(*f)),
        Op::Pop => body.push(F::Pop),
        Op::Dup => body.push(F::Dup),
        Op::Dup2 => body.push(F::Dup2),
        Op::Swap => body.push(F::Swap),
        Op::Add => body.push(F::Add),
        Op::Sub => body.push(F::Sub),
        Op::Mul => body.push(F::Mul),
        Op::Div => body.push(F::AwkDivJit),
        Op::Mod => body.push(F::Extended(ext_ops::STK_MOD_FLOOR, 0)),
        // String comparisons lower to host-helper-backed Extended ops; operands
        // are raw StrykeValue bit-handles (see `segment_is_string_compare_eligible`).
        Op::StrEq => body.push(F::Extended(ext_ops::STK_STR_EQ, 0)),
        Op::StrNe => body.push(F::Extended(ext_ops::STK_STR_NE, 0)),
        Op::StrLt => body.push(F::Extended(ext_ops::STK_STR_LT, 0)),
        Op::StrGt => body.push(F::Extended(ext_ops::STK_STR_GT, 0)),
        Op::StrLe => body.push(F::Extended(ext_ops::STK_STR_LE, 0)),
        Op::StrGe => body.push(F::Extended(ext_ops::STK_STR_GE, 0)),
        Op::StrCmp => body.push(F::Extended(ext_ops::STK_STR_CMP, 0)),
        // String concatenation allocates a new owned string handle (see
        // `segment_is_string_concat_eligible` / `stryke_str_concat_op`).
        Op::Concat => body.push(F::Extended(ext_ops::STK_STR_CONCAT, 0)),
        Op::Pow => body.push(F::PowFloat),
        Op::Negate => body.push(F::Negate),
        Op::NumEq => body.push(F::NumEq),
        Op::NumNe => body.push(F::NumNe),
        Op::NumLt => body.push(F::NumLt),
        Op::NumGt => body.push(F::NumGt),
        Op::NumLe => body.push(F::NumLe),
        Op::NumGe => body.push(F::NumGe),
        Op::Spaceship => body.push(F::Spaceship),
        Op::LogNot => body.push(F::LogNot),
        Op::Inc => body.push(F::Inc),
        Op::Dec => body.push(F::Dec),
        Op::BitAnd => body.push(F::BitAnd),
        Op::BitOr => body.push(F::BitOr),
        Op::BitXor => body.push(F::BitXor),
        Op::BitNot => body.push(F::BitNot),
        Op::Shl => body.push(F::Shl),
        Op::Shr => body.push(F::Shr),
        Op::GetScalarSlot(s) => body.push(F::GetSlot(*s as u16)),
        Op::SetScalarSlot(s) => body.push(F::SetSlot(*s as u16)),
        // `my $x = EXPR`: the init value is already on the stack; storing it is
        // exactly `SetSlot`. The name-pool index is only for closure capture,
        // which never occurs in the pure-integer subset.
        Op::DeclareScalarSlot(s, _) => body.push(F::SetSlot(*s as u16)),
        // Store but leave the value on the stack: store, then reload.
        Op::SetScalarSlotKeep(s) => {
            body.push(F::SetSlot(*s as u16));
            body.push(F::GetSlot(*s as u16));
        }
        // `$d += $s` (void): fusevm has the exact fused op.
        Op::AddAssignSlotSlotVoid(d, s) => {
            body.push(F::AddAssignSlotVoid(*d as u16, *s as u16))
        }
        // `$d += $s` / `-=` / `*=` (push result): compute into the slot and
        // reload the new value onto the stack.
        Op::AddAssignSlotSlot(d, s) => emit_slot_assign(body, *d, *s, F::Add),
        Op::SubAssignSlotSlot(d, s) => emit_slot_assign(body, *d, *s, F::Sub),
        Op::MulAssignSlotSlot(d, s) => emit_slot_assign(body, *d, *s, F::Mul),
        Op::PreIncSlot(s) => body.push(F::PreIncSlot(*s as u16)),
        Op::PreDecSlot(s) => body.push(F::PreDecSlot(*s as u16)),
        Op::PostIncSlot(s) => body.push(F::PostIncSlot(*s as u16)),
        Op::PostDecSlot(s) => body.push(F::PostDecSlot(*s as u16)),
        Op::PreIncSlotVoid(s) => body.push(F::PreIncSlotVoid(*s as u16)),
        // `while $i < limit { $sum += $i; $i += 1 }` — fully fused; no jump target.
        Op::AccumSumLoop(sum, i, limit) => {
            body.push(F::AccumSumLoop(*sum as u16, *i as u16, *limit))
        }
        // Fused loop ops with an in-segment jump target (resolved in pass 2).
        Op::SlotLtIntJumpIfFalse(s, limit, t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::SlotLtIntJumpIfFalse(*s as u16, *limit, 0));
        }
        Op::SlotIncLtIntJumpBack(s, limit, t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::SlotIncLtIntJumpBack(*s as u16, *limit, 0));
        }
        Op::Jump(t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::Jump(0));
        }
        Op::JumpIfTrue(t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::JumpIfTrue(0));
        }
        Op::JumpIfFalse(t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::JumpIfFalse(0));
        }
        _ => return false,
    }
    true
}

/// Emit `$d = $d <arith> $s` followed by a reload of the new value (so the
/// op leaves its result on the stack, matching strykelang's push-variant
/// `*AssignSlotSlot` ops).
#[inline]
fn emit_slot_assign(body: &mut Vec<fusevm::Op>, d: u8, s: u8, arith: fusevm::Op) {
    use fusevm::Op as F;
    body.push(F::GetSlot(d as u16));
    body.push(F::GetSlot(s as u16));
    body.push(arith);
    body.push(F::SetSlot(d as u16));
    body.push(F::GetSlot(d as u16));
}

/// Build the fusevm chunk for `seg`: a `PushFrame` + slot-init preamble seeded
/// from `slot_buf`, followed by the translated body. Because some strykelang ops
/// expand to several fusevm ops, jump targets are remapped through a per-source-op
/// offset table rather than a fixed 1:1 stride.
fn build_chunk(
    seg: &[Op],
    seg_start: usize,
    slot_buf: &[i64],
    seed_slots: bool,
) -> Option<fusevm::Chunk> {
    let preamble = preamble_len(slot_buf.len(), seed_slots);

    // Pass 1: translate the body, recording where each source op starts and
    // any jump ops that need their targets resolved.
    let mut body: Vec<fusevm::Op> = Vec::with_capacity(seg.len());
    let mut src_off: Vec<usize> = Vec::with_capacity(seg.len() + 1);
    let mut fixups: Vec<(usize, usize)> = Vec::new();
    for op in seg {
        src_off.push(body.len());
        if !translate_op_into(op, seg_start, &mut body, &mut fixups) {
            return None;
        }
    }
    // A jump to `seg_end` (one past the last op) lands here.
    src_off.push(body.len());

    // Pass 2: resolve jump targets to absolute chunk positions.
    for (body_idx, src_target) in fixups {
        let target = preamble + src_off.get(src_target).copied()?;
        match &mut body[body_idx] {
            fusevm::Op::Jump(t) | fusevm::Op::JumpIfTrue(t) | fusevm::Op::JumpIfFalse(t) => {
                *t = target;
            }
            fusevm::Op::SlotLtIntJumpIfFalse(_, _, t)
            | fusevm::Op::SlotIncLtIntJumpBack(_, _, t) => {
                *t = target;
            }
            _ => {}
        }
    }

    let mut b = fusevm::ChunkBuilder::new();
    b.emit(fusevm::Op::PushFrame, 0);
    // Integer segments bake their marshaled slot seeds into the chunk (stable
    // because eligible slots are write-before-read, seeded 0). String-comparison
    // segments must NOT bake their operand handles (live pointers vary per run and
    // would both poison the disk-cache key and persist a stale pointer into cached
    // native code); the block JIT reads them from the runtime `slots` buffer.
    if seed_slots {
        for (i, v) in slot_buf.iter().enumerate() {
            b.emit(fusevm::Op::LoadInt(*v), 0);
            b.emit(fusevm::Op::SetSlot(i as u16), 0);
        }
    }
    for op in body {
        b.emit(op, 0);
    }
    Some(b.build())
}

/// Execute an eligible linear segment on `fusevm::VM`.
///
/// `slot_buf` is pre-filled by the caller with the integer values of the slots
/// the segment reads; on success it is overwritten in place with the slots'
/// post-execution values (caller writes the changed ones back into scope).
///
/// Returns `Some(return_value)` when the segment is eligible and fusevm ran it
/// (the value is the body's top-of-stack result, used by a `ReturnValue`
/// terminator); returns `None` when the segment is ineligible or fusevm errored,
/// in which case the caller falls back to strykelang's own JIT/interpreter and
/// `slot_buf` must be treated as unchanged.
/// Whether `seg`'s computed result is always an integer/bool — i.e. it contains
/// no `Div`/`Pow`, the only eligible ops that yield a `fusevm::Value::Float`.
///
/// fusevm's block JIT returns an `i64`, so only integer-result segments may take
/// the JIT path; float-result segments fall back to the interpreter, which
/// preserves the float through [`value_from_fusevm`].
#[inline]
fn segment_result_is_integer(seg: &[Op]) -> bool {
    !seg.iter().any(|o| matches!(o, Op::Div | Op::Pow))
}

/// Configure fusevm's per-thread block-JIT warmup so the first invocation of a
/// chunk compiles (or loads from the on-disk cache) immediately, instead of
/// interpreting once to warm up. strykelang's "re-run the same script" workload
/// runs each top-level segment once per process, so eager compilation is what
/// makes the disk cache populate and subsequent runs skip codegen.
///
/// Respects an explicit `FUSEVM_JIT_BLOCK_THRESHOLD` override if the user set one.
fn configure_block_jit_eager() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    DONE.with(|d| {
        if d.get() {
            return;
        }
        if std::env::var_os("FUSEVM_JIT_BLOCK_THRESHOLD").is_none() {
            let jit = fusevm::JitCompiler::new();
            let mut cfg = jit.get_config();
            cfg.block_threshold = 0;
            jit.set_config(cfg);
        }
        register_stryke_jit_ext();
        d.set(true);
    });
}

pub(crate) fn run_linear_segment(
    seg: &[Op],
    seg_start: usize,
    slot_buf: &mut [i64],
    term: SubTerminator,
) -> Option<StrykeValue> {
    if !matches!(term, SubTerminator::Value) {
        return None;
    }
    let int_ok = segment_is_fusevm_eligible(seg, seg_start);
    // The float-operand path also reports the segment's *static* result kind
    // (`Int` for a comparison/bool, `Float` for arithmetic involving a float). We
    // trust this strykelang-side prediction to decide how to reconstruct the
    // result, because fusevm's block JIT collapses whole-number float literals to
    // integers (e.g. `$x * 2.0` is computed in i64), so its runtime kind would
    // under-report floats. Integer arithmetic over whole-number operands is
    // bit-identical to the float computation, so coercing such an integer result
    // to a float is exact.
    //
    // A segment can be `int_ok` yet still yield a float when it contains an
    // always-float op (`Div`/`Pow`, lowered to fusevm `Op::AwkDivJit`/`PowFloat`):
    // `segment_result_is_integer` reports it as *not* integer-result, so we must
    // still compute `float_kind` for it — otherwise such a bare segment (e.g.
    // `$x / $y` or `$x ** $y`) would skip BOTH the integer block JIT (wrong
    // result kind) and the float block JIT, falling to the interpreter and never
    // persisting a native blob to the on-disk cache.
    let float_kind = if int_ok && segment_result_is_integer(seg) {
        None
    } else {
        segment_fusevm_float_result_kind(seg, seg_start)
    };
    let float_ok = float_kind.is_some();
    let concat_ok =
        !int_ok && !float_ok && segment_is_string_concat_eligible(seg, seg_start);
    let str_ok = !int_ok
        && !float_ok
        && !concat_ok
        && segment_is_string_compare_eligible(seg, seg_start);
    if !int_ok && !float_ok && !str_ok && !concat_ok {
        return None;
    }

    // String segments (comparison + concatenation): build an *unseeded* chunk
    // (operand handles flow through the runtime `slots` buffer, never baked into
    // the chunk) and run it strictly on the block JIT, which is configured to
    // compile eagerly. There is no fusevm-interpreter fallback here: that path
    // reads slots from the VM frame, which an unseeded chunk never populates. On
    // block-JIT decline we return None so strykelang falls back to its own
    // interpreter.
    //
    // Comparison results are plain `i64` outcomes (0/1, or -1/0/1 for `cmp`).
    // Concatenation returns the raw bits of a freshly allocated, *owned* string
    // handle, which we reconstitute into exactly one owning `StrykeValue` via
    // `from_raw_bits` (the helper `mem::forget`-ed it, transferring ownership).
    if str_ok || concat_ok {
        configure_block_jit_eager();
        let chunk = build_chunk(seg, seg_start, slot_buf, false)?;
        let jit = fusevm::JitCompiler::new();
        return jit.try_run_block(&chunk, slot_buf).map(|ret| {
            if concat_ok {
                StrykeValue::from_raw_bits(ret as u64)
            } else {
                StrykeValue::integer(ret)
            }
        });
    }

    // Fast path: pure-integer segments — and segments that compute with float
    // *operands* but yield an integer/bool result (`float_ok`, e.g. `$x < 0.5` or a
    // ternary `$x < 0.5 ? a : b`) — run on fusevm's block JIT, which persists native
    // code to the on-disk cache so repeated runs skip Cranelift codegen. The chunk is
    // built *unseeded* (just like the string segments): the marshaled operand/local
    // values flow through the runtime `slot_buf` that `try_run_block` reads via
    // `GetSlot`, instead of being baked into the chunk as `LoadInt` immediates. This
    // keeps the chunk's `op_hash` (the disk-cache key) independent of the actual
    // argument values, so a hot sub called with thousands of distinct arguments
    // reuses a SINGLE cached native blob rather than spilling one file per argument
    // combination.
    if float_ok || (int_ok && segment_result_is_integer(seg)) {
        configure_block_jit_eager();
        let chunk = build_chunk(seg, seg_start, slot_buf, false)?;
        let jit = fusevm::JitCompiler::new();
        // A segment containing strykelang `/` lowers to fusevm `Op::AwkDivJit`,
        // which traps (via a thread-local flag) on a zero divisor and returns a
        // sentinel. On a trap we must discard the JIT result and decline so
        // strykelang's interpreter re-runs and raises its own "Illegal division by
        // zero". Clear any stale flag before the run, then check it after.
        let has_div = seg.iter().any(|o| matches!(o, Op::Div));
        if has_div {
            jit.take_awk_div_trap();
        }
        match float_kind {
            // Float-result segment (e.g. `$x * 1.5` or `$x / $y`): use the typed
            // entry point so a genuinely fractional result comes back exactly.
            // fusevm may still report a whole-number result as `Int` (whole-float-
            // literal collapse); coerce it to a float since the static analysis
            // proved the value is a float.
            Some(NumTy::Float) => {
                if let Some(num) = jit.try_run_block_typed_kinded(&chunk, slot_buf, &[]) {
                    if has_div && jit.take_awk_div_trap() {
                        return None;
                    }
                    return Some(match num {
                        fusevm::BlockNum::Float(f) => StrykeValue::float(f),
                        fusevm::BlockNum::Int(n) => StrykeValue::float(n as f64),
                    });
                }
            }
            // Integer/bool-result segment (e.g. a float comparison `$x < 0.5`, or a
            // division feeding a comparison `$x / $y < 1`): the result is a genuine
            // integer, so the plain i64 entry point is exact.
            Some(NumTy::Int) | None => {
                if let Some(ret) = jit.try_run_block(&chunk, slot_buf) {
                    if has_div && jit.take_awk_div_trap() {
                        return None;
                    }
                    return Some(StrykeValue::integer(ret));
                }
            }
        }
    }

    // Fallback: fusevm's interpreter. Handles float/bool results and any chunk the
    // block JIT declines, reading the post-run slot values from the VM frame. This
    // path *does* need the slots seeded into the frame, so it uses a seeded chunk
    // (not disk-cached as native code, so the per-value `op_hash` churn is moot).
    // The strykelang Extended handler makes `%` (STK_MOD_FLOOR) correct on this
    // cold path too; without it fusevm would treat the unknown op as a no-op.
    let chunk = build_chunk(seg, seg_start, slot_buf, true)?;
    let mut vm = fusevm::VM::new(chunk);
    vm.set_extension_handler(Box::new(stryke_ext_handler));
    let result = vm.run();

    if let Some(frame) = vm.frames.last() {
        for (i, slot) in slot_buf.iter_mut().enumerate() {
            if let Some(v) = frame.slots.get(i) {
                *slot = v.to_int();
            }
        }
    }

    match result {
        fusevm::VMResult::Ok(val) => Some(value_from_fusevm(&val)),
        _ => None,
    }
}

/// Convert a `fusevm::Value` produced by the universal-integer subset back into a
/// `StrykeValue`. The subset only yields integers/bools/floats.
#[inline]
fn value_from_fusevm(v: &fusevm::Value) -> StrykeValue {
    match v {
        fusevm::Value::Int(n) => StrykeValue::integer(*n),
        fusevm::Value::Bool(b) => StrykeValue::integer(if *b { 1 } else { 0 }),
        fusevm::Value::Float(f) => StrykeValue::float(*f),
        other => StrykeValue::integer(other.to_int()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Op;

    // `Op::Mod` is now eligible: it lowers to `Extended(STK_MOD_FLOOR)` and runs
    // on fusevm with Perl *floored* semantics (sign of divisor). Cover both signs
    // and the divide-trap guards (b == 0 and b == -1).
    #[test]
    fn fusevm_runs_floored_modulo() {
        let cases = [
            (-7_i64, 3_i64, 2_i64),
            (7, 3, 1),
            (7, -3, -2),
            (-7, -3, -1),
            (-6, 3, 0),
            (10, 4, 2),
            (5, 0, 0),    // guarded: no trap
            (-8, -1, 0),  // guarded: no trap
        ];
        for (a, b, want) in cases {
            let seg = vec![Op::LoadInt(a), Op::LoadInt(b), Op::Mod];
            assert!(segment_is_fusevm_eligible(&seg, 0));
            let mut slots: [i64; 0] = [];
            let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
                .expect("modulo segment must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "{a} % {b}");
        }
    }

    // A ternary `?:` body computes its result in each branch and merges it on the
    // operand stack — at an interior join or via a value-carrying jump to the implicit
    // return point. fusevm's block JIT now carries such cross-block operand-stack values
    // as block parameters, so these segments are eligible (and disk-cacheable). The
    // stack-consistency check still rejects genuinely malformed segments (inconsistent
    // merge depth / underflow). A straight-line body and a counted loop stay eligible.
    #[test]
    fn ternary_merge_segment_is_eligible() {
        // `$neg ? 2*$v+1 : 2*$v` — JumpIfFalse to the else branch, then Jump to the
        // segment end carrying the result on the stack (both edges merge at depth 1).
        let ternary = vec![
            Op::GetScalarSlot(0),  // 0: $neg
            Op::JumpIfFalse(8),    // 1: -> else (index 8)
            Op::LoadInt(2),        // 2
            Op::GetScalarSlot(1),  // 3: $v
            Op::Mul,               // 4
            Op::LoadInt(1),        // 5
            Op::Add,               // 6
            Op::Jump(11),          // 7: -> end (len == 11)
            Op::LoadInt(2),        // 8: else
            Op::GetScalarSlot(1),  // 9: $v
            Op::Mul,               // 10
        ];
        assert!(segment_block_stack_is_consistent(&ternary, 0));
        assert!(segment_is_fusevm_eligible(&ternary, 0));

        // Straight-line arithmetic is fine.
        let linear = vec![Op::GetScalarSlot(0), Op::GetScalarSlot(1), Op::Add];
        assert!(segment_block_stack_is_consistent(&linear, 0));
        assert!(segment_is_fusevm_eligible(&linear, 0));

        // A fused counted loop reaches every leader with an empty stack (state is in
        // slots), so it stays eligible.
        let loop_seg = vec![
            Op::SlotLtIntJumpIfFalse(1, 100, 4), // 0: top: while i<100 -> exit(4)
            Op::PreIncSlotVoid(0),               // 1: body
            Op::SlotIncLtIntJumpBack(1, 100, 1), // 2: i++; if i<100 -> body(1)
            Op::Jump(4),                         // 3: -> exit(4)
            Op::GetScalarSlot(0),                // 4: exit: push result
        ];
        assert!(segment_block_stack_is_consistent(&loop_seg, 0));

        // An inconsistent merge — a leader reached at two different stack depths —
        // is still rejected.
        let inconsistent = vec![
            Op::GetScalarSlot(0), // 0: depth -> 1
            Op::JumpIfFalse(3),   // 1: pops -> 0; jump to 3 (depth 0) / fall to 2 (depth 0)
            Op::LoadInt(1),       // 2: depth -> 1; falls to 3 at depth 1
            Op::GetScalarSlot(1), // 3: reached at depth 0 (jump) AND depth 1 (fall) -> bad
        ];
        assert!(!segment_block_stack_is_consistent(&inconsistent, 0));
    }

    // A float comparison such as `$x < 0.5` computes in floating point (fusevm
    // promotes the integer slot to f64) but yields an integer/bool result, which the
    // block JIT can return as an exact i64. These segments are now eligible via the
    // float-operand path and produce the same 0/1 the interpreter would.
    #[test]
    fn float_compare_segment_runs_on_fusevm() {
        // `$x < 1.5` for an integer slot $x: true for 1, false for 2.
        let seg = vec![Op::GetScalarSlot(0), Op::LoadFloat(1.5), Op::NumLt];
        assert!(!segment_is_fusevm_eligible(&seg, 0));
        assert!(segment_is_fusevm_float_eligible(&seg, 0));

        for (x, want) in [(1_i64, 1_i64), (2, 0), (-3, 1)] {
            let mut slots = [x];
            let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
                .expect("float-compare segment must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "{x} < 1.5");
        }

        // A ternary whose float condition selects between two integers merges an
        // integer on both edges, so it stays eligible: `$x < 1.5 ? 10 : 20`.
        let ternary = vec![
            Op::GetScalarSlot(0), // 0: $x
            Op::LoadFloat(1.5),   // 1
            Op::NumLt,            // 2
            Op::JumpIfFalse(6),   // 3: -> else (index 6)
            Op::LoadInt(10),      // 4
            Op::Jump(7),          // 5: -> end (len == 7)
            Op::LoadInt(20),      // 6: else
        ];
        assert!(segment_is_fusevm_float_eligible(&ternary, 0));
        for (x, want) in [(1_i64, 10_i64), (2, 20)] {
            let mut slots = [x];
            let out = run_linear_segment(&ternary, 0, &mut slots, SubTerminator::Value)
                .expect("float-ternary segment must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "$x={x} ? 10 : 20");
        }

        // A float arithmetic result (`$x * 1.5`) is now eligible: fusevm's block
        // tier returns the float exactly (as f64 bits via the typed entry point),
        // so we reconstruct the precise value instead of truncating.
        let float_result = vec![Op::GetScalarSlot(0), Op::LoadFloat(1.5), Op::Mul];
        assert!(segment_is_fusevm_float_eligible(&float_result, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&float_result, 0),
            Some(NumTy::Float)
        );
        for x in [2_i64, 4, -3] {
            let mut slots = [x];
            let out = run_linear_segment(&float_result, 0, &mut slots, SubTerminator::Value)
                .expect("float-result segment must run on fusevm");
            assert_eq!(out.as_float(), Some(x as f64 * 1.5), "$x={x} * 1.5");
        }

        // A ternary whose branches yield a float result is also eligible now and
        // returns the selected float exactly.
        let float_ternary = vec![
            Op::GetScalarSlot(0),
            Op::LoadFloat(1.5),
            Op::NumLt,
            Op::JumpIfFalse(6),
            Op::LoadFloat(1.0),
            Op::Jump(7),
            Op::LoadFloat(2.0),
        ];
        assert!(segment_is_fusevm_float_eligible(&float_ternary, 0));
        for (x, want) in [(1_i64, 1.0_f64), (2, 2.0)] {
            let mut slots = [x];
            let out = run_linear_segment(&float_ternary, 0, &mut slots, SubTerminator::Value)
                .expect("float-ternary segment must run on fusevm");
            assert_eq!(out.as_float(), Some(want), "$x={x} ? 1.0 : 2.0");
        }

        // Storing a float into an integer-marshaled slot is rejected.
        let float_to_slot =
            vec![Op::LoadFloat(2.5), Op::SetScalarSlot(0), Op::LoadInt(0)];
        assert!(!segment_is_fusevm_float_eligible(&float_to_slot, 0));

        // Float division is now eligible: strykelang `/` lowers to fusevm
        // `Op::AwkDivJit`, which divides in floating point (operands coerced to
        // f64) consistently in the block JIT, the interpreter, and the native disk
        // cache. `($x / 2.0) < 1.0` yields a bool: true for x=1 (0.5 < 1.0), false
        // for x=3 (1.5 < 1.0).
        let float_div_cmp = vec![Op::GetScalarSlot(0), Op::LoadFloat(2.0), Op::Div, Op::LoadFloat(1.0), Op::NumLt];
        assert!(segment_is_fusevm_float_eligible(&float_div_cmp, 0));
        for (x, want) in [(1_i64, 1_i64), (3, 0)] {
            let mut slots = [x];
            let out = run_linear_segment(&float_div_cmp, 0, &mut slots, SubTerminator::Value)
                .expect("float-division compare must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "($x={x} / 2.0) < 1.0");
        }

        // A bare float division yields an exact float result (7 / 2 == 3.5), and is
        // eligible even without an explicit `LoadFloat` (division is always float).
        let float_div = vec![Op::GetScalarSlot(0), Op::GetScalarSlot(1), Op::Div];
        assert!(segment_is_fusevm_float_eligible(&float_div, 0));
        let mut slots = [7_i64, 2_i64];
        let out = run_linear_segment(&float_div, 0, &mut slots, SubTerminator::Value)
            .expect("bare float division must run on fusevm");
        assert_eq!(out.as_float(), Some(3.5), "7 / 2 == 3.5");

        // Division by zero: the JIT traps and the bridge declines (returns None) so
        // the caller's interpreter can raise strykelang's own "Illegal division by
        // zero", rather than silently returning the AwkDivJit sentinel.
        let mut zslots = [7_i64, 0_i64];
        let zout = run_linear_segment(&float_div, 0, &mut zslots, SubTerminator::Value);
        assert!(zout.is_none(), "division by zero must decline to the interpreter");

        // Always-float exponentiation: strykelang `**` lowers to fusevm
        // `Op::PowFloat`, which raises to a power in floating point (operands
        // coerced to f64 via the `pow_f64` host helper) consistently in the block
        // JIT, the interpreter, and the native disk cache. A bare `$x ** $y`
        // yields an exact float (2 ** 10 == 1024.0) and is eligible even without
        // an explicit `LoadFloat` (exponentiation is always float).
        let float_pow = vec![Op::GetScalarSlot(0), Op::GetScalarSlot(1), Op::Pow];
        assert!(segment_is_fusevm_float_eligible(&float_pow, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&float_pow, 0),
            Some(NumTy::Float)
        );
        let mut pslots = [2_i64, 10_i64];
        let out = run_linear_segment(&float_pow, 0, &mut pslots, SubTerminator::Value)
            .expect("bare float exponentiation must run on fusevm");
        assert_eq!(out.as_float(), Some(1024.0), "2 ** 10 == 1024.0");

        // `($x ** 2.0) < 5.0` yields a bool: true for x=2 (4.0 < 5.0), false for
        // x=3 (9.0 < 5.0).
        let float_pow_cmp = vec![
            Op::GetScalarSlot(0),
            Op::LoadFloat(2.0),
            Op::Pow,
            Op::LoadFloat(5.0),
            Op::NumLt,
        ];
        assert!(segment_is_fusevm_float_eligible(&float_pow_cmp, 0));
        for (x, want) in [(2_i64, 1_i64), (3, 0)] {
            let mut slots = [x];
            let out = run_linear_segment(&float_pow_cmp, 0, &mut slots, SubTerminator::Value)
                .expect("float-exponentiation compare must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "($x={x} ** 2.0) < 5.0");
        }

        // A segment with no float operand is not a float segment (the integer path
        // covers it).
        let pure_int = vec![Op::GetScalarSlot(0), Op::LoadInt(1), Op::NumLt];
        assert!(!segment_is_fusevm_float_eligible(&pure_int, 0));
    }

    // A string-compare segment that mixes in non-string ops (here `LoadInt`) is not a
    // pure string-comparison body and is rejected, regardless of control flow. (Cross-
    // block operand-stack merges are no longer themselves disqualifying — fusevm carries
    // them as block params — so the rejection here is purely the op-set gate.)
    #[test]
    fn string_compare_with_non_string_ops_is_ineligible() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::StrEq,
            Op::JumpIfFalse(7),
            Op::LoadInt(1),
            Op::Jump(8),
            Op::Pop,
            Op::LoadInt(0),
        ];
        assert!(!segment_is_string_compare_eligible(&seg, 0));
    }

    // String comparisons lower to host-helper-backed Extended ops and run on
    // fusevm's block JIT (result is i64 0/1, or -1/0/1 for cmp). Operands are the
    // raw NaN-boxed `StrykeValue` bits, marshaled as i64 slot handles; the helper
    // reconstructs them and defers to strykelang's native `str_eq`/`str_cmp`, so
    // results match the interpreter byte-for-byte (incl. multibyte UTF-8).
    #[test]
    fn fusevm_runs_string_comparisons() {
        // Keep operands alive for the duration of the call (the helper borrows the
        // Arc-backed string via its raw bits without taking ownership).
        let foo = StrykeValue::string("foo".to_string());
        let bar = StrykeValue::string("bar".to_string());
        let foo2 = StrykeValue::string("foo".to_string());
        // "café" with a multibyte grapheme vs an emoji — exercises non-ASCII bytes.
        let cafe = StrykeValue::string("café".to_string());
        let emoji = StrykeValue::string("🦀".to_string());

        // (a, b, op, expected) where op is the strykelang bytecode op.
        let cases: &[(&StrykeValue, &StrykeValue, Op, i64)] = &[
            (&foo, &foo2, Op::StrEq, 1),
            (&foo, &bar, Op::StrEq, 0),
            (&foo, &bar, Op::StrNe, 1),
            (&foo, &foo2, Op::StrNe, 0),
            (&bar, &foo, Op::StrLt, 1),  // "bar" < "foo"
            (&foo, &bar, Op::StrLt, 0),
            (&foo, &bar, Op::StrGt, 1),
            (&bar, &foo, Op::StrGt, 0),
            (&foo, &foo2, Op::StrLe, 1),
            (&foo, &bar, Op::StrLe, 0),
            (&foo, &foo2, Op::StrGe, 1),
            (&bar, &foo, Op::StrGe, 0),
            (&bar, &foo, Op::StrCmp, -1),
            (&foo, &foo2, Op::StrCmp, 0),
            (&foo, &bar, Op::StrCmp, 1),
            (&cafe, &emoji, Op::StrLt, 1), // 'c' (0x63) < first emoji byte (0xF0)
            (&cafe, &cafe, Op::StrEq, 1),
        ];
        for (a, b, op, want) in cases {
            let seg = vec![Op::GetScalarSlot(0), Op::GetScalarSlot(1), op.clone()];
            assert!(
                segment_is_string_compare_eligible(&seg, 0),
                "segment must be string-compare eligible for {op:?}"
            );
            let mut slots = [a.raw_bits() as i64, b.raw_bits() as i64];
            let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
                .expect("string-compare segment must run on fusevm");
            assert_eq!(out.as_integer(), Some(*want), "op {op:?}");
        }
    }

    // `$a . $b` lowered to fusevm allocates a NEW owned string handle whose bytes
    // equal the interpreter's `push_str`, for ASCII, multibyte UTF-8 and empty
    // operands. The operands themselves must survive (they are only borrowed), and
    // running many iterations would surface any use-after-free / double-free in the
    // forget-on-return ownership transfer.
    #[test]
    fn fusevm_runs_string_concat() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::Concat,
        ];
        assert!(segment_is_string_concat_eligible(&seg, 0));

        let cases: &[(&str, &str)] = &[
            ("foo", "bar"),
            ("", "tail"),
            ("head", ""),
            ("", ""),
            ("café", "🦀"),
            ("naïve", "façade"),
        ];
        for (a_s, b_s) in cases {
            let a = StrykeValue::string((*a_s).to_string());
            let b = StrykeValue::string((*b_s).to_string());
            let mut last = None;
            for _ in 0..256 {
                let mut slots = [a.raw_bits() as i64, b.raw_bits() as i64];
                let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
                    .expect("string-concat segment must run on fusevm");
                last = out.as_str();
            }
            let want = format!("{a_s}{b_s}");
            assert_eq!(last.as_deref(), Some(want.as_str()), "{a_s:?} . {b_s:?}");
            // Operands are only borrowed by the helper, so they remain intact.
            assert_eq!(a.as_str().as_deref(), Some(*a_s));
            assert_eq!(b.as_str().as_deref(), Some(*b_s));
        }
    }

    // The concat chunk, like the compare chunks, must not bake operand pointers
    // into its op_hash: it persists exactly one disk-cache blob that is reused
    // across different operand strings, and the result is still correct.
    #[test]
    fn string_concat_segment_produces_stable_disk_cache_entry() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::Concat,
        ];

        let dir = std::env::temp_dir().join(format!(
            "stryke_concatcache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FUSEVM_JIT_CACHE_DIR", &dir);

        let prefix = {
            let p1 = StrykeValue::string("p1".to_string());
            let p2 = StrykeValue::string("p2".to_string());
            let slots = [p1.raw_bits() as i64, p2.raw_bits() as i64];
            let chunk = build_chunk(&seg, 0, &slots, false).expect("probe chunk");
            format!("{:016x}.", chunk.op_hash)
        };
        let count_blobs = || -> usize {
            std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.to_string_lossy();
                    n.starts_with(&prefix) && n.ends_with(".blk.fjit")
                })
                .count()
        };

        let a1 = StrykeValue::string("alpha".to_string());
        let b1 = StrykeValue::string("-one".to_string());
        let mut slots1 = [a1.raw_bits() as i64, b1.raw_bits() as i64];
        let out1 = run_linear_segment(&seg, 0, &mut slots1, SubTerminator::Value);
        assert_eq!(out1.and_then(|v| v.as_str()).as_deref(), Some("alpha-one"));
        let after_first = count_blobs();

        let a2 = StrykeValue::string("gamma-distinct".to_string());
        let b2 = StrykeValue::string("-two-distinct".to_string());
        let mut slots2 = [a2.raw_bits() as i64, b2.raw_bits() as i64];
        let out2 = run_linear_segment(&seg, 0, &mut slots2, SubTerminator::Value);
        assert_eq!(
            out2.and_then(|v| v.as_str()).as_deref(),
            Some("gamma-distinct-two-distinct")
        );
        let after_second = count_blobs();

        std::env::remove_var("FUSEVM_JIT_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(after_first, 1, "concat segment must persist exactly one blob");
        assert_eq!(
            after_second, after_first,
            "different operands must reuse the same concat cache blob"
        );
    }

    // A pure-integer sub body seeded from `@_` (e.g. `sub addnums { my ($a,$b)=@_;
    // return $a+$b }`) must also persist exactly ONE native blob, no matter how many
    // DISTINCT argument values it is called with. The integer block-JIT path builds
    // an *unseeded* chunk so the marshaled args flow through `slot_buf` (via `GetSlot`)
    // instead of being baked in as `LoadInt` immediates; without this fix a hot sub
    // called with thousands of distinct args would spill one cache file per arg combo.
    #[test]
    fn integer_arg_segment_produces_stable_disk_cache_entry() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::Add,
        ];
        assert!(segment_is_fusevm_eligible(&seg, 0));

        let dir = std::env::temp_dir().join(format!(
            "stryke_intargcache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FUSEVM_JIT_CACHE_DIR", &dir);

        let prefix = {
            let slots = [3_i64, 4_i64];
            let chunk = build_chunk(&seg, 0, &slots, false).expect("probe chunk");
            format!("{:016x}.", chunk.op_hash)
        };
        let count_blobs = || -> usize {
            std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.to_string_lossy();
                    n.starts_with(&prefix) && n.ends_with(".blk.fjit")
                })
                .count()
        };

        let mut slots1 = [100_i64, 23_i64];
        let out1 = run_linear_segment(&seg, 0, &mut slots1, SubTerminator::Value);
        assert_eq!(out1.and_then(|v| v.as_integer()), Some(123));
        let after_first = count_blobs();

        // Thousands of distinct argument pairs must all reuse the same blob.
        for i in 0..2000_i64 {
            let mut slots = [i * 7 + 1, i * 13 + 2];
            let got = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
                .and_then(|v| v.as_integer());
            assert_eq!(got, Some((i * 7 + 1) + (i * 13 + 2)));
        }
        let after_many = count_blobs();

        std::env::remove_var("FUSEVM_JIT_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(after_first, 1, "integer-arg segment must persist exactly one blob");
        assert_eq!(
            after_many, after_first,
            "distinct argument values must reuse the same cache blob (no per-arg explosion)"
        );
    }

    // End-to-end: running a string-compare segment with an on-disk cache dir set
    // and re-running with DIFFERENT operand strings reuses that same blob (no new
    // file, no unbounded growth). This is the concrete proof that the safety fix
    // makes string compares genuinely disk-cacheable.
    #[test]
    fn string_compare_segment_produces_stable_disk_cache_entry() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::StrLt,
        ];

        // Unique temp cache dir so we only ever count OUR op_hash's blobs (other
        // concurrently-running tests may also write here once the env var is set,
        // but they use different op_hashes, so filtering by prefix stays race-safe).
        let dir = std::env::temp_dir().join(format!(
            "stryke_strcache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FUSEVM_JIT_CACHE_DIR", &dir);

        let prefix = {
            let probe = StrykeValue::string("probe-a".to_string());
            let probe2 = StrykeValue::string("probe-b".to_string());
            let slots = [probe.raw_bits() as i64, probe2.raw_bits() as i64];
            let chunk = build_chunk(&seg, 0, &slots, false).expect("probe chunk");
            format!("{:016x}.", chunk.op_hash)
        };
        let count_blobs = || -> usize {
            std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.to_string_lossy();
                    n.starts_with(&prefix) && n.ends_with(".blk.fjit")
                })
                .count()
        };

        // Run 1.
        let a1 = StrykeValue::string("apple".to_string());
        let b1 = StrykeValue::string("banana".to_string());
        let mut slots1 = [a1.raw_bits() as i64, b1.raw_bits() as i64];
        let out1 = run_linear_segment(&seg, 0, &mut slots1, SubTerminator::Value);
        assert_eq!(out1.and_then(|v| v.as_integer()), Some(1), "apple < banana");
        let after_first = count_blobs();

        // Run 2 with DIFFERENT operand strings (different pointers).
        let a2 = StrykeValue::string("cherry-distinct".to_string());
        let b2 = StrykeValue::string("date-distinct".to_string());
        let mut slots2 = [a2.raw_bits() as i64, b2.raw_bits() as i64];
        let out2 = run_linear_segment(&seg, 0, &mut slots2, SubTerminator::Value);
        assert_eq!(out2.and_then(|v| v.as_integer()), Some(1), "cherry < date");
        let after_second = count_blobs();

        std::env::remove_var("FUSEVM_JIT_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            after_first, 1,
            "string-compare segment must persist exactly one native blob"
        );
        assert_eq!(
            after_second, after_first,
            "different operand strings must REUSE the same cache blob (pointer-independent key)"
        );
    }

    // A string-compare chunk must NOT bake operand handles into its op_hash (the
    // disk-cache key): different live string pointers must yield byte-identical
    // chunk ops and therefore the SAME op_hash, so the on-disk native blob is
    // reused across runs/values instead of growing unbounded — and so no stale
    // pointer is ever frozen into cached native code.
    #[test]
    fn string_compare_chunk_op_hash_is_pointer_independent() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::StrEq,
        ];
        // Two completely different operand sets → different raw pointer handles.
        let a1 = StrykeValue::string("alpha".to_string());
        let b1 = StrykeValue::string("beta".to_string());
        let a2 = StrykeValue::string("gamma-distinct".to_string());
        let b2 = StrykeValue::string("delta-distinct".to_string());
        let slots1 = [a1.raw_bits() as i64, b1.raw_bits() as i64];
        let slots2 = [a2.raw_bits() as i64, b2.raw_bits() as i64];
        assert_ne!(slots1, slots2, "operand handles must actually differ");

        let chunk1 = build_chunk(&seg, 0, &slots1, false).expect("chunk 1");
        let chunk2 = build_chunk(&seg, 0, &slots2, false).expect("chunk 2");
        assert_eq!(
            chunk1.op_hash, chunk2.op_hash,
            "unseeded string-compare chunks must hash identically regardless of operand pointers"
        );

        // Sanity: the SEEDED build (numeric path) WOULD bake the values and thus
        // differ — this is exactly why string segments must use the unseeded form.
        let seeded1 = build_chunk(&seg, 0, &slots1, true).expect("seeded 1");
        let seeded2 = build_chunk(&seg, 0, &slots2, true).expect("seeded 2");
        assert_ne!(
            seeded1.op_hash, seeded2.op_hash,
            "seeded chunks bake operand values, so their hashes diverge"
        );
    }

    // The shared comparator matches strykelang's native byte-level semantics and
    // never drops its borrowed operands (Arc strong count is preserved).
    #[test]
    fn string_compare_op_preserves_operands() {
        let a = StrykeValue::string("hello".to_string());
        let b = StrykeValue::string("hello".to_string());
        for _ in 0..1000 {
            assert_eq!(
                stryke_str_compare_op(ext_ops::STK_STR_EQ, a.raw_bits() as i64, b.raw_bits() as i64),
                1
            );
        }
        // If the helper had dropped a borrow, this would have use-after-freed above.
        assert!(a.str_eq(&b));
    }

    // The `floored_mod` reference helper matches Perl `%` across signs.
    #[test]
    fn floored_mod_reference_semantics() {
        assert_eq!(floored_mod(-7, 3), 2);
        assert_eq!(floored_mod(7, -3), -2);
        assert_eq!(floored_mod(7, 3), 1);
        assert_eq!(floored_mod(-7, -3), -1);
        assert_eq!(floored_mod(i64::MIN, -1), 0); // guarded
        assert_eq!(floored_mod(5, 0), 0); // guarded
    }

    // Pure arithmetic with no slots: (40 + 2) * 3 - 6 == 120, executed on fusevm.
    #[test]
    fn fusevm_runs_translated_arithmetic() {
        let seg = vec![
            Op::LoadInt(40),
            Op::LoadInt(2),
            Op::Add,
            Op::LoadInt(3),
            Op::Mul,
            Op::LoadInt(6),
            Op::Sub,
        ];
        let mut slots: [i64; 0] = [];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
            .expect("segment must be fusevm-eligible and run");
        assert_eq!(out.as_integer(), Some(120));
    }

    // Slot round-trip: read slot 0 (=7), double it, write back; return value = 14.
    #[test]
    fn fusevm_reads_and_writes_slots() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::LoadInt(2),
            Op::Mul,
            Op::SetScalarSlot(0),
            Op::GetScalarSlot(0),
        ];
        let mut slots = [7_i64];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
            .expect("slot segment must run on fusevm");
        assert_eq!(slots[0], 14);
        assert_eq!(out.as_integer(), Some(14));
    }

    // In-segment forward jump remaps by the preamble offset and still lands right.
    #[test]
    fn fusevm_remaps_in_segment_jump() {
        // seg_start = 100; Jump(103) skips the LoadInt(999) at rel idx 1.
        let seg = vec![
            Op::Jump(103), // abs 100 -> skip to abs 103
            Op::LoadInt(999),
            Op::LoadInt(888),
            Op::LoadInt(5), // abs 103: landing pad
        ];
        let mut slots: [i64; 0] = [];
        let out = run_linear_segment(&seg, 100, &mut slots, SubTerminator::Value)
            .expect("jump segment must run on fusevm");
        assert_eq!(out.as_integer(), Some(5));
    }

    // Pre-increment a slot and stack Inc/Dec all lower onto fusevm: slot 0 = 10,
    // `++$slot` → 11 (written back), then push it, Inc → 12, Dec → 11.
    #[test]
    fn fusevm_runs_slot_preinc_and_stack_incdec() {
        let seg = vec![
            Op::PreIncSlot(0), // slot 0: 10 -> 11, pushes 11
            Op::Inc,           // 11 -> 12
            Op::Dec,           // 12 -> 11
        ];
        let mut slots = [10_i64];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
            .expect("preinc/inc/dec segment must run on fusevm");
        assert_eq!(slots[0], 11);
        assert_eq!(out.as_integer(), Some(11));
    }

    // A non-universal op (string concat) makes the segment ineligible: caller
    // falls back, signaled by None.
    #[test]
    fn ineligible_segment_returns_none() {
        let seg = vec![Op::LoadInt(1), Op::Concat];
        let mut slots: [i64; 0] = [];
        assert!(run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value).is_none());
    }

    // Void terminator is not handled by the universal tier.
    #[test]
    fn void_terminator_returns_none() {
        let seg = vec![Op::LoadInt(1)];
        let mut slots: [i64; 0] = [];
        assert!(run_linear_segment(&seg, 0, &mut slots, SubTerminator::Void).is_none());
    }

    // strykelang's fused slot-arith-assign ops decompose onto fusevm primitives.
    // `my $a=7; my $b=3; my $c=$a+$b; $c+=$a; $c-=$b; return $c` == 14.
    #[test]
    fn fusevm_runs_fused_slot_assign_ops() {
        let seg = vec![
            Op::LoadInt(7),
            Op::DeclareScalarSlot(0, 0), // $a = 7
            Op::LoadInt(3),
            Op::DeclareScalarSlot(1, 0), // $b = 3
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::Add,
            Op::DeclareScalarSlot(2, 0),      // $c = $a + $b = 10
            Op::AddAssignSlotSlotVoid(2, 0),  // $c += $a -> 17 (void)
            Op::SubAssignSlotSlot(2, 1),      // $c -= $b -> 14 (pushes 14)
            Op::Pop,
            Op::GetScalarSlot(2), // push $c = 14
        ];
        let mut slots = [0_i64; 3];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
            .expect("fused slot-assign segment must run on fusevm");
        assert_eq!(slots[2], 14);
        assert_eq!(out.as_integer(), Some(14));
    }

    // `MulAssignSlotSlot` push-variant: $x=6; $y=7; $x*=$y == 42, left on stack.
    #[test]
    fn fusevm_runs_mul_assign_slot() {
        let seg = vec![
            Op::LoadInt(6),
            Op::DeclareScalarSlot(0, 0), // $x = 6
            Op::LoadInt(7),
            Op::DeclareScalarSlot(1, 0),  // $y = 7
            Op::MulAssignSlotSlot(0, 1),  // $x *= $y -> 42 (pushes 42)
        ];
        let mut slots = [0_i64; 2];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
            .expect("mul-assign-slot segment must run on fusevm");
        assert_eq!(slots[0], 42);
        assert_eq!(out.as_integer(), Some(42));
    }

    // A counted loop built from the generic fused loop ops lowers to fusevm with
    // its in-segment jump targets remapped: sum of 0..4 == 10.
    #[test]
    fn fusevm_runs_counted_loop() {
        // idx: 0..=7 (seg_start = 0)
        let seg = vec![
            Op::LoadInt(0),
            Op::DeclareScalarSlot(0, 0), // 1: $sum = 0  (slot 0)
            Op::LoadInt(0),
            Op::DeclareScalarSlot(1, 0),        // 3: $i = 0    (slot 1)
            Op::SlotLtIntJumpIfFalse(1, 5, 7),  // 4: if !($i<5) goto 7 (exit)
            Op::AddAssignSlotSlotVoid(0, 1),    // 5: body: $sum += $i
            Op::SlotIncLtIntJumpBack(1, 5, 5),  // 6: $i++; if $i<5 goto 5
            Op::GetScalarSlot(0),               // 7: exit: push $sum
        ];
        let mut slots = [0_i64; 2];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
            .expect("counted-loop segment must run on fusevm");
        assert_eq!(out.as_integer(), Some(10));
    }

    // The fully-fused `AccumSumLoop` lowers 1:1: sum of 0..4 == 10.
    #[test]
    fn fusevm_runs_accum_sum_loop() {
        let seg = vec![
            Op::LoadInt(0),
            Op::DeclareScalarSlot(0, 0), // $sum = 0
            Op::LoadInt(0),
            Op::DeclareScalarSlot(1, 0), // $i = 0
            Op::AccumSumLoop(0, 1, 5),   // while $i<5 { $sum += $i; $i += 1 }
            Op::GetScalarSlot(0),        // push $sum
        ];
        let mut slots = [0_i64; 2];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value)
            .expect("accum-sum-loop segment must run on fusevm");
        assert_eq!(out.as_integer(), Some(10));
    }
}
