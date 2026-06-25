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
    for op in &chunk.ops {
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
            // End of the top-level program: stop lowering and let fusevm return
            // the value left on the stack.
            Op::Return | Op::Halt => break,
            // Anything else is outside the Phase-1 subset → fall back to vm.rs.
            _ => return None,
        }
    }

    let fchunk = b.build();
    let mut vm = fusevm::VM::new(fchunk);
    vm.set_extension_handler(Box::new(native_ext_handler));
    match vm.run() {
        fusevm::VMResult::Ok(v) => match fusevm_to_stryke(&v) {
            Some(sv) => Some(Ok(sv)),
            // Result type outside the subset; let vm.rs handle it (the arithmetic
            // subset has no side effects, so re-running is safe).
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
}
