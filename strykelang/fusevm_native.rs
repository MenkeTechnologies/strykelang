//! Phase 1 of retiring strykelang's own VM (`crate::vm`) and JIT (`crate::jit`)
//! in favor of running *whole programs* on the shared [`fusevm`] runtime using
//! **native fusevm Values** (not the i64 bit-handles the numeric-segment bridge
//! in [`crate::fusevm_bridge`] uses).
//!
//! Today [`crate::fusevm_bridge`] offloads only eligible numeric segments to
//! fusevm; everything else runs on `crate::vm`. The end state is that the entire
//! program lowers to a fusevm `Chunk` (universal ops + Extended ops backed by a
//! stryke host) and runs on `fusevm::VM`, after which `vm.rs`/`jit.rs` are
//! deleted and stryke becomes a true fusevm frontend (so `--aot` works exactly
//! like zshrs/vimlrs/awkrs).
//!
//! This module is that path, built incrementally. It is opt-in via the
//! `STRYKE_FUSEVM_ONLY` environment variable and currently covers the
//! integer/float arithmetic subset, returning the program's value. Crucially,
//! [`try_run_native`] **aborts to `None` on any op outside the covered subset**,
//! so the caller falls back to `crate::vm` — the migration never silently
//! diverges; coverage only ever grows behind passing parity checks.

use crate::bytecode::{Chunk, Op};
use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;
use crate::vm_helper::VMHelper;
use std::sync::Arc;

/// strykelang Extended-op IDs handled by [`native_ext_handler`] on the
/// fusevm-only path. These operate on **native fusevm Values** (distinct from
/// the i64-handle Extended ops in [`crate::fusevm_bridge`]).
mod nops {
    /// Perl `.` string concatenation (pops 2, pushes the concatenated string).
    pub const CONCAT: u16 = 0;
    /// String comparisons (`eq ne lt gt le ge`); each pops 2, pushes Int 1/0.
    pub const STR_EQ: u16 = 1;
    pub const STR_NE: u16 = 2;
    pub const STR_LT: u16 = 3;
    pub const STR_GT: u16 = 4;
    pub const STR_LE: u16 = 5;
    pub const STR_GE: u16 = 6;
    /// Numeric comparisons (`== != < > <= >=`); each pops 2, pushes Int 1/0.
    pub const NUM_EQ: u16 = 7;
    pub const NUM_NE: u16 = 8;
    pub const NUM_LT: u16 = 9;
    pub const NUM_GT: u16 = 10;
    pub const NUM_LE: u16 = 11;
    pub const NUM_GE: u16 = 12;
    /// `<=>` numeric spaceship (pops 2, pushes Int -1/0/1).
    pub const SPACESHIP: u16 = 13;
    /// Perl `!` logical-not (pops 1, pushes Int 1/0).
    pub const LOG_NOT: u16 = 14;
    /// Normalize top-of-stack to Perl truthiness as Int 1/0, so fusevm's
    /// conditional jumps branch using strykelang's `is_true` (fusevm's native
    /// truthiness differs for values like the string "0").
    pub const TRUTHY: u16 = 15;
    /// Arithmetic that can fault or change type (`/ % **`); each pops 2, pushes
    /// the result (or records a runtime error via the error slot).
    pub const DIV: u16 = 16;
    pub const MOD: u16 = 17;
    pub const POW: u16 = 18;
}

thread_local! {
    /// A runtime error raised inside [`native_ext_handler`] (which cannot return
    /// a `Result`). [`try_run_native`] drains it after the run and propagates it.
    static NATIVE_ERR: std::cell::RefCell<Option<StrykeError>> =
        const { std::cell::RefCell::new(None) };
}

fn set_native_err(e: StrykeError) {
    NATIVE_ERR.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.is_none() {
            *slot = Some(e);
        }
    });
}

/// Convert a StrykeValue back to a native fusevm Value (scalar subset).
fn stryke_to_fusevm(v: &StrykeValue) -> fusevm::Value {
    if let Some(i) = v.as_integer() {
        fusevm::Value::Int(i)
    } else if let Some(f) = v.as_float() {
        fusevm::Value::Float(f)
    } else if let Some(s) = v.as_str() {
        fusevm::Value::Str(Arc::new(s))
    } else {
        fusevm::Value::Undef
    }
}

/// Numeric comparison matching strykelang's `int_cmp`: exact integer compare
/// when both operands are integers, else a float compare via `to_number`.
fn num_cmp(a: &StrykeValue, b: &StrykeValue, id: u16) -> bool {
    if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
        match id {
            nops::NUM_EQ => x == y,
            nops::NUM_NE => x != y,
            nops::NUM_LT => x < y,
            nops::NUM_GT => x > y,
            nops::NUM_LE => x <= y,
            _ => x >= y,
        }
    } else {
        let (x, y) = (a.to_number(), b.to_number());
        match id {
            nops::NUM_EQ => x == y,
            nops::NUM_NE => x != y,
            nops::NUM_LT => x < y,
            nops::NUM_GT => x > y,
            nops::NUM_LE => x <= y,
            _ => x >= y,
        }
    }
}

/// `<=>`: integer compare when both are integers, else float (matches vm.rs).
fn spaceship(a: &StrykeValue, b: &StrykeValue) -> i64 {
    if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
        (x > y) as i64 - (x < y) as i64
    } else {
        let (x, y) = (a.to_number(), b.to_number());
        if x < y {
            -1
        } else if x > y {
            1
        } else {
            0
        }
    }
}

/// Pop a fusevm Value and view it as a StrykeValue (scalars only on this path).
fn pop_stryke(vm: &mut fusevm::VM) -> StrykeValue {
    fusevm_to_stryke(&vm.pop()).unwrap_or(StrykeValue::UNDEF)
}

/// Stringify a fusevm Value the way strykelang stringifies a scalar, by routing
/// through `StrykeValue`'s `Display` (so float formatting etc. match vm.rs).
fn stryke_display(v: &fusevm::Value) -> String {
    match v {
        fusevm::Value::Int(n) => StrykeValue::integer(*n).to_string(),
        fusevm::Value::Float(f) => StrykeValue::float(*f).to_string(),
        fusevm::Value::Str(s) => (**s).clone(),
        other => fusevm_to_stryke(other)
            .map(|sv| sv.to_string())
            .unwrap_or_default(),
    }
}

/// Extension handler for strykelang's native-value Extended ops, installed on
/// the fusevm-only VM. Each op delegates to strykelang's own value semantics so
/// results match vm.rs.
fn native_ext_handler(vm: &mut fusevm::VM, id: u16, _arg: u8) {
    match id {
        nops::CONCAT => {
            let b = vm.pop();
            let a = vm.pop();
            let mut s = stryke_display(&a);
            s.push_str(&stryke_display(&b));
            vm.push(fusevm::Value::Str(Arc::new(s)));
        }
        // String comparisons delegate to StrykeValue::str_eq / str_cmp (the same
        // methods vm.rs uses), so results match exactly. Push Perl bool (Int 1/0).
        nops::STR_EQ | nops::STR_NE | nops::STR_LT | nops::STR_GT | nops::STR_LE
        | nops::STR_GE => {
            use std::cmp::Ordering;
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            let truth = match id {
                nops::STR_EQ => a.str_eq(&b),
                nops::STR_NE => !a.str_eq(&b),
                nops::STR_LT => a.str_cmp(&b) == Ordering::Less,
                nops::STR_GT => a.str_cmp(&b) == Ordering::Greater,
                nops::STR_LE => matches!(a.str_cmp(&b), Ordering::Less | Ordering::Equal),
                _ => matches!(a.str_cmp(&b), Ordering::Greater | Ordering::Equal),
            };
            vm.push(fusevm::Value::Int(if truth { 1 } else { 0 }));
        }
        nops::NUM_EQ | nops::NUM_NE | nops::NUM_LT | nops::NUM_GT | nops::NUM_LE
        | nops::NUM_GE => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            vm.push(fusevm::Value::Int(if num_cmp(&a, &b, id) { 1 } else { 0 }));
        }
        nops::SPACESHIP => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            vm.push(fusevm::Value::Int(spaceship(&a, &b)));
        }
        nops::LOG_NOT => {
            let a = pop_stryke(vm);
            vm.push(fusevm::Value::Int(if a.is_true() { 0 } else { 1 }));
        }
        nops::TRUTHY => {
            let a = pop_stryke(vm);
            vm.push(fusevm::Value::Int(if a.is_true() { 1 } else { 0 }));
        }
        // `/` — integer quotient when both are integers and divisible, else
        // float; matches vm.rs's Div closure. Records div-by-zero in the error
        // slot and pushes Undef (the run is aborted after dispatch).
        nops::DIV => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            let result = if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                if y == 0 {
                    set_native_err(StrykeError::division_by_zero("Illegal division by zero", 0));
                    fusevm::Value::Undef
                } else if x % y == 0 {
                    fusevm::Value::Int(x / y)
                } else {
                    fusevm::Value::Float(x as f64 / y as f64)
                }
            } else {
                let d = b.to_number();
                if d == 0.0 {
                    set_native_err(StrykeError::division_by_zero("Illegal division by zero", 0));
                    fusevm::Value::Undef
                } else {
                    fusevm::Value::Float(a.to_number() / d)
                }
            };
            vm.push(result);
        }
        nops::MOD => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            let bi = b.to_int();
            if bi == 0 {
                set_native_err(StrykeError::division_by_zero("Illegal modulus zero", 0));
                vm.push(fusevm::Value::Undef);
            } else {
                vm.push(fusevm::Value::Int(crate::value::perl_mod_i64(a.to_int(), bi)));
            }
        }
        nops::POW => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            vm.push(stryke_to_fusevm(&crate::value::compat_pow(&a, &b)));
        }
        _ => {}
    }
}

/// Convert a stryke constant to a native fusevm Value, if it is in the Phase-1
/// subset (integer/float). Heap values (strings/arrays/hashes/closures/…) return
/// `None` and are handled in later phases.
fn const_to_fusevm(v: &StrykeValue) -> Option<fusevm::Value> {
    if let Some(i) = v.as_integer() {
        Some(fusevm::Value::Int(i))
    } else if let Some(f) = v.as_float() {
        Some(fusevm::Value::Float(f))
    } else {
        v.as_str().map(|s| fusevm::Value::Str(Arc::new(s)))
    }
}

/// Convert a fusevm result Value back to a StrykeValue (Phase-1 subset).
fn fusevm_to_stryke(v: &fusevm::Value) -> Option<StrykeValue> {
    match v {
        fusevm::Value::Int(n) => Some(StrykeValue::integer(*n)),
        fusevm::Value::Float(f) => Some(StrykeValue::float(*f)),
        fusevm::Value::Str(s) => Some(StrykeValue::string((**s).clone())),
        _ => None,
    }
}

/// Attempt to run `chunk` entirely on `fusevm::VM` with native Values. Returns
/// `Some(result)` when the whole program is in the covered subset, else `None`
/// (the caller then runs it on `crate::vm`). `interp` is unused in Phase 1 (no
/// vars/closures/host yet) but threaded for later phases.
pub fn try_run_native(chunk: &Chunk, _interp: &mut VMHelper) -> Option<StrykeResult<StrykeValue>> {
    let mut b = fusevm::ChunkBuilder::new();
    // Map each source op index to the fusevm op index its lowering starts at, so
    // jump targets (absolute source indices) can be remapped after lowering.
    let mut src_to_dst: Vec<usize> = Vec::with_capacity(chunk.ops.len() + 1);
    // (fusevm jump op index, source target index) pairs, patched in pass 2.
    let mut jump_fixups: Vec<(usize, usize)> = Vec::new();
    for op in &chunk.ops {
        src_to_dst.push(b.current_pos());
        match op {
            Op::Nop => {
                b.emit(fusevm::Op::Nop, 0);
            }
            // Phase marker for BEGIN/END/AOP ordering; irrelevant to a pure
            // arithmetic result. Phase-dependent programs use AOP ops outside
            // this subset, which abort to `None` below.
            Op::SetGlobalPhase(_) => {}
            Op::LoadInt(n) => {
                b.emit(fusevm::Op::LoadInt(*n), 0);
            }
            Op::LoadFloat(f) => {
                b.emit(fusevm::Op::LoadFloat(*f), 0);
            }
            Op::LoadConst(idx) => {
                let sv = chunk.constants.get(*idx as usize)?;
                let fv = const_to_fusevm(sv)?;
                let fi = b.add_constant(fv);
                b.emit(fusevm::Op::LoadConst(fi), 0);
            }
            Op::Pop => {
                b.emit(fusevm::Op::Pop, 0);
            }
            Op::Add => {
                b.emit(fusevm::Op::Add, 0);
            }
            Op::Sub => {
                b.emit(fusevm::Op::Sub, 0);
            }
            Op::Mul => {
                b.emit(fusevm::Op::Mul, 0);
            }
            Op::Div => {
                b.emit(fusevm::Op::Extended(nops::DIV, 0), 0);
            }
            Op::Mod => {
                b.emit(fusevm::Op::Extended(nops::MOD, 0), 0);
            }
            Op::Pow => {
                b.emit(fusevm::Op::Extended(nops::POW, 0), 0);
            }
            Op::Negate => {
                b.emit(fusevm::Op::Negate, 0);
            }
            // Perl `.` concatenation → native-value Extended op (handler
            // stringifies scalars via strykelang's Display).
            Op::Concat => {
                b.emit(fusevm::Op::Extended(nops::CONCAT, 0), 0);
            }
            Op::StrEq => {
                b.emit(fusevm::Op::Extended(nops::STR_EQ, 0), 0);
            }
            Op::StrNe => {
                b.emit(fusevm::Op::Extended(nops::STR_NE, 0), 0);
            }
            Op::StrLt => {
                b.emit(fusevm::Op::Extended(nops::STR_LT, 0), 0);
            }
            Op::StrGt => {
                b.emit(fusevm::Op::Extended(nops::STR_GT, 0), 0);
            }
            Op::StrLe => {
                b.emit(fusevm::Op::Extended(nops::STR_LE, 0), 0);
            }
            Op::StrGe => {
                b.emit(fusevm::Op::Extended(nops::STR_GE, 0), 0);
            }
            Op::NumEq => {
                b.emit(fusevm::Op::Extended(nops::NUM_EQ, 0), 0);
            }
            Op::NumNe => {
                b.emit(fusevm::Op::Extended(nops::NUM_NE, 0), 0);
            }
            Op::NumLt => {
                b.emit(fusevm::Op::Extended(nops::NUM_LT, 0), 0);
            }
            Op::NumGt => {
                b.emit(fusevm::Op::Extended(nops::NUM_GT, 0), 0);
            }
            Op::NumLe => {
                b.emit(fusevm::Op::Extended(nops::NUM_LE, 0), 0);
            }
            Op::NumGe => {
                b.emit(fusevm::Op::Extended(nops::NUM_GE, 0), 0);
            }
            Op::Spaceship => {
                b.emit(fusevm::Op::Extended(nops::SPACESHIP, 0), 0);
            }
            Op::LogNot => {
                b.emit(fusevm::Op::Extended(nops::LOG_NOT, 0), 0);
            }
            // Scalar locals: strykelang stores them in `interp.scope` slots; on
            // the native path they live in the fusevm frame's slots instead
            // (self-consistent within the run — Declare/Set/Get use the same
            // storage). `set_slot` auto-grows, so no pre-sizing is needed. The
            // declared name is only symbolic and is dropped.
            Op::DeclareScalarSlot(slot, _name) => {
                b.emit(fusevm::Op::SetSlot(*slot as u16), 0);
            }
            Op::GetScalarSlot(slot) => {
                b.emit(fusevm::Op::GetSlot(*slot as u16), 0);
            }
            Op::SetScalarSlot(slot) => {
                b.emit(fusevm::Op::SetSlot(*slot as u16), 0);
            }
            // Keep variant peeks (assignment used as an expression): dup, store.
            Op::SetScalarSlotKeep(slot) => {
                b.emit(fusevm::Op::Dup, 0);
                b.emit(fusevm::Op::SetSlot(*slot as u16), 0);
            }
            // Fused superinstructions (loops/compound-assign). Lowered to their
            // unfused universal-op equivalents — correct, and fusevm's block JIT
            // re-fuses these very patterns to machine code.
            Op::AddAssignSlotSlotVoid(d, s) => {
                b.emit(fusevm::Op::GetSlot(*d as u16), 0);
                b.emit(fusevm::Op::GetSlot(*s as u16), 0);
                b.emit(fusevm::Op::Add, 0);
                b.emit(fusevm::Op::SetSlot(*d as u16), 0);
            }
            Op::PreIncSlotVoid(s) => {
                b.emit(fusevm::Op::GetSlot(*s as u16), 0);
                b.emit(fusevm::Op::LoadInt(1), 0);
                b.emit(fusevm::Op::Add, 0);
                b.emit(fusevm::Op::SetSlot(*s as u16), 0);
            }
            // `slot < int` then conditional jump. The NUM_LT Extended op yields
            // Int 0/1, so JumpIfFalse needs no TRUTHY normalization here.
            Op::SlotLtIntJumpIfFalse(slot, int, target) => {
                b.emit(fusevm::Op::GetSlot(*slot as u16), 0);
                b.emit(fusevm::Op::LoadInt(*int as i64), 0);
                b.emit(fusevm::Op::Extended(nops::NUM_LT, 0), 0);
                let pos = b.emit(fusevm::Op::JumpIfFalse(0), 0);
                jump_fixups.push((pos, *target));
            }
            // Top-level terminator: emit nothing and let the chunk end
            // naturally (fusevm returns the value left on the stack when
            // `ip >= len`). Emitting a fusevm `Return` here is wrong — at the
            // root frame it pops the frame and resets ip, re-running the whole
            // program. Jumps to the terminator resolve to body-end via
            // `src_to_dst`, which also ends the run.
            Op::Return | Op::Halt => {}
            // Control flow (pop variants). Emit the fusevm jump with a
            // placeholder target recorded for fixup. Conditional jumps first
            // normalize the condition to Perl truthiness (Int 0/1) via TRUTHY,
            // since fusevm's native truthiness differs from strykelang's.
            // The Keep variants (`&&`/`||`/ternary short-circuit) peek and are
            // not yet covered → they hit `_ => return None` and fall back.
            Op::Jump(t) => {
                let pos = b.emit(fusevm::Op::Jump(0), 0);
                jump_fixups.push((pos, *t));
            }
            Op::JumpIfFalse(t) => {
                b.emit(fusevm::Op::Extended(nops::TRUTHY, 0), 0);
                let pos = b.emit(fusevm::Op::JumpIfFalse(0), 0);
                jump_fixups.push((pos, *t));
            }
            Op::JumpIfTrue(t) => {
                b.emit(fusevm::Op::Extended(nops::TRUTHY, 0), 0);
                let pos = b.emit(fusevm::Op::JumpIfTrue(0), 0);
                jump_fixups.push((pos, *t));
            }
            // Anything else is outside the covered subset → fall back to vm.rs.
            _ => return None,
        }
    }
    // End sentinel so a jump to "one past the last op" resolves to program end.
    src_to_dst.push(b.current_pos());
    for (pos, target) in jump_fixups {
        let dst = *src_to_dst.get(target)?;
        b.patch_jump(pos, dst);
    }

    let fchunk = b.build();
    let mut vm = fusevm::VM::new(fchunk);
    vm.set_extension_handler(Box::new(native_ext_handler));
    NATIVE_ERR.with(|c| *c.borrow_mut() = None);
    let outcome = vm.run();
    // A runtime error raised inside the handler (e.g. division by zero) takes
    // precedence over the VM's own result.
    if let Some(e) = NATIVE_ERR.with(|c| c.borrow_mut().take()) {
        return Some(Err(e));
    }
    match outcome {
        fusevm::VMResult::Ok(v) => match fusevm_to_stryke(&v) {
            Some(sv) => Some(Ok(sv)),
            // Result type outside the subset; let vm.rs handle it (the covered
            // ops have no side effects, so re-running is safe).
            None => None,
        },
        fusevm::VMResult::Halted => Some(Ok(StrykeValue::UNDEF)),
        fusevm::VMResult::Error(e) => Some(Err(StrykeError::runtime(e, 0))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile `code` and run it on the fusevm-only path; `None` means the
    /// program is outside the covered subset.
    fn native(code: &str) -> Option<StrykeResult<StrykeValue>> {
        let program = crate::parse(code).expect("parse");
        let chunk = crate::compiler::Compiler::new()
            .compile_program(&program)
            .expect("compile");
        try_run_native(&chunk, &mut VMHelper::new())
    }

    /// Native path must agree with the normal vm.rs path on the returned value
    /// for arithmetic programs.
    fn assert_parity_int(code: &str, expect: i64) {
        let vm = crate::run(code).expect("vm run");
        assert_eq!(vm.as_integer(), Some(expect), "vm.rs value for `{code}`");
        let nat = native(code)
            .unwrap_or_else(|| panic!("`{code}` not covered by native path"))
            .expect("native run");
        assert_eq!(nat.as_integer(), Some(expect), "native value for `{code}`");
    }

    /// Native path must agree with vm.rs on string-valued results.
    fn assert_parity_str(code: &str, expect: &str) {
        let vm = crate::run(code).expect("vm run");
        assert_eq!(vm.as_str().as_deref(), Some(expect), "vm.rs value for `{code}`");
        let nat = native(code)
            .unwrap_or_else(|| panic!("`{code}` not covered by native path"))
            .expect("native run");
        assert_eq!(nat.as_str().as_deref(), Some(expect), "native value for `{code}`");
    }

    #[test]
    fn native_integer_arithmetic_matches_vm() {
        assert_parity_int("1 + 2 * 3", 7);
        assert_parity_int("10 - 4 - 3", 3);
        assert_parity_int("2 * 3 * 4", 24);
        assert_parity_int("-(2 + 3)", -5);
    }

    #[test]
    fn native_scalar_concat_matches_vm() {
        assert_parity_str("\"a\" . \"b\"", "ab");
        assert_parity_str("\"x\" . 3", "x3");
        assert_parity_str("1 . 2", "12");
        assert_parity_str("\"sum=\" . (2 + 3)", "sum=5");
    }

    #[test]
    fn native_string_compares_match_vm() {
        assert_parity_int("\"a\" eq \"a\"", 1);
        assert_parity_int("\"a\" eq \"b\"", 0);
        assert_parity_int("\"a\" ne \"b\"", 1);
        assert_parity_int("\"a\" lt \"b\"", 1);
        assert_parity_int("\"b\" gt \"a\"", 1);
        assert_parity_int("\"a\" le \"a\"", 1);
        assert_parity_int("\"b\" ge \"a\"", 1);
    }

    #[test]
    fn native_numeric_compares_match_vm() {
        assert_parity_int("1 < 2", 1);
        assert_parity_int("2 < 1", 0);
        assert_parity_int("3 == 3", 1);
        assert_parity_int("3 != 4", 1);
        assert_parity_int("5 >= 5", 1);
        assert_parity_int("2 <= 1", 0);
        assert_parity_int("1.5 < 2.5", 1);
    }

    #[test]
    fn native_spaceship_and_lognot_match_vm() {
        assert_parity_int("1 <=> 2", -1);
        assert_parity_int("2 <=> 2", 0);
        assert_parity_int("3 <=> 1", 1);
        assert_parity_int("!0", 1);
        assert_parity_int("!5", 0);
    }

    #[test]
    fn native_if_else_control_flow_matches_vm() {
        assert_parity_int("if (1 < 2) { 10 } else { 20 }", 10);
        assert_parity_int("if (2 < 1) { 10 } else { 20 }", 20);
        // Perl truthiness: the string "0" is false (fusevm's native truthiness
        // would treat it true) — exercises the TRUTHY normalization.
        assert_parity_int("if (\"0\") { 1 } else { 2 }", 2);
        assert_parity_int("if (\"x\") { 1 } else { 2 }", 1);
    }

    #[test]
    fn native_scalar_variables_match_vm() {
        assert_parity_int("my $x = 5; $x + 1", 6);
        assert_parity_int("my $x = 5; my $y = 10; $x + $y", 15);
        assert_parity_int("my $x = 5; $x = $x * 2; $x", 10);
        assert_parity_str("my $s = \"hi\"; $s . \"!\"", "hi!");
    }

    /// Native path must agree with vm.rs on the value's display form (covers
    /// any scalar type — int, float, string).
    fn assert_parity_display(code: &str, expect: &str) {
        let vm = crate::run(code).expect("vm run");
        assert_eq!(vm.to_string(), expect, "vm.rs value for `{code}`");
        let nat = native(code)
            .unwrap_or_else(|| panic!("`{code}` not covered by native path"))
            .expect("native run");
        assert_eq!(nat.to_string(), expect, "native value for `{code}`");
    }

    #[test]
    fn native_div_mod_pow_match_vm() {
        // Display-based (native must equal vm.rs) — avoids int-vs-float
        // assumptions (Perl `**` yields a float; `/` is int only when divisible).
        for code in ["10 / 2", "7 / 2", "17 % 5", "(-7) % 3", "2 ** 10", "9 ** 0.5"] {
            let expect = crate::run(code).expect("vm run").to_string();
            assert_parity_display(code, &expect);
        }
    }

    #[test]
    fn native_while_loops_match_vm() {
        assert_parity_int(
            "my $s = 0; my $i = 1; while ($i <= 10) { $s = $s + $i; $i = $i + 1 } $s",
            55,
        );
        // if-with-comparison compiles to the fused SlotLtIntJumpIfFalse.
        assert_parity_int("my $x = 1; if ($x < 5) { $x = 9 } $x", 9);
        assert_parity_int("my $x = 7; if ($x < 5) { $x = 9 } $x", 7);
    }
}
