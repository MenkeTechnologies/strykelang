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

    /// First ID reserved for strykelang; frontends must keep their blocks disjoint.
    pub const STK_ID_BASE: u16 = STK_REGEX_MATCH;
}

/// `PushFrame` + `slot_count` × (`LoadInt` + `SetSlot`) — the preamble emitted
/// before a translated body so fusevm slots start from the marshaled values.
#[inline]
fn preamble_len(slot_count: usize) -> usize {
    1 + slot_count * 2
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
            | Op::PreIncSlot(_) => continue,
            Op::Jump(t) | Op::JumpIfTrue(t) | Op::JumpIfFalse(t) => {
                if *t < seg_start || *t > seg_end {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// Translate a single eligible op to its `fusevm::Op`. Jump targets are absolute
/// strykelang IPs; they remap to `preamble + (target - seg_start)` because the body
/// is a 1:1 copy after the slot-init preamble.
#[inline]
fn translate_op(op: &Op, seg_start: usize, preamble: usize) -> Option<fusevm::Op> {
    use fusevm::Op as F;
    let remap = |t: usize| preamble + (t - seg_start);
    Some(match op {
        Op::LoadInt(n) => F::LoadInt(*n),
        Op::Pop => F::Pop,
        Op::Dup => F::Dup,
        Op::Dup2 => F::Dup2,
        Op::Swap => F::Swap,
        Op::Add => F::Add,
        Op::Sub => F::Sub,
        Op::Mul => F::Mul,
        Op::Div => F::Div,
        Op::Mod => F::Mod,
        Op::Pow => F::Pow,
        Op::Negate => F::Negate,
        Op::NumEq => F::NumEq,
        Op::NumNe => F::NumNe,
        Op::NumLt => F::NumLt,
        Op::NumGt => F::NumGt,
        Op::NumLe => F::NumLe,
        Op::NumGe => F::NumGe,
        Op::Spaceship => F::Spaceship,
        Op::LogNot => F::LogNot,
        Op::Inc => F::Inc,
        Op::Dec => F::Dec,
        Op::BitAnd => F::BitAnd,
        Op::BitOr => F::BitOr,
        Op::BitXor => F::BitXor,
        Op::BitNot => F::BitNot,
        Op::Shl => F::Shl,
        Op::Shr => F::Shr,
        Op::GetScalarSlot(s) => F::GetSlot(*s as u16),
        Op::SetScalarSlot(s) => F::SetSlot(*s as u16),
        Op::PreIncSlot(s) => F::PreIncSlot(*s as u16),
        Op::Jump(t) => F::Jump(remap(*t)),
        Op::JumpIfTrue(t) => F::JumpIfTrue(remap(*t)),
        Op::JumpIfFalse(t) => F::JumpIfFalse(remap(*t)),
        _ => return None,
    })
}

/// Build the fusevm chunk for `seg`: a `PushFrame` + slot-init preamble seeded
/// from `slot_buf`, followed by the 1:1-translated body.
fn build_chunk(seg: &[Op], seg_start: usize, slot_buf: &[i64]) -> Option<fusevm::Chunk> {
    let preamble = preamble_len(slot_buf.len());
    let mut b = fusevm::ChunkBuilder::new();
    b.emit(fusevm::Op::PushFrame, 0);
    for (i, v) in slot_buf.iter().enumerate() {
        b.emit(fusevm::Op::LoadInt(*v), 0);
        b.emit(fusevm::Op::SetSlot(i as u16), 0);
    }
    for op in seg {
        b.emit(translate_op(op, seg_start, preamble)?, 0);
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
pub(crate) fn run_linear_segment(
    seg: &[Op],
    seg_start: usize,
    slot_buf: &mut [i64],
    term: SubTerminator,
) -> Option<StrykeValue> {
    if !matches!(term, SubTerminator::Value) {
        return None;
    }
    if !segment_is_fusevm_eligible(seg, seg_start) {
        return None;
    }
    let chunk = build_chunk(seg, seg_start, slot_buf)?;
    let mut vm = fusevm::VM::new(chunk);
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
}
