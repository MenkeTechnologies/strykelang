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

/// Convert a stryke constant to a native fusevm Value, if it is in the Phase-1
/// subset (integer/float). Heap values (strings/arrays/hashes/closures/…) return
/// `None` and are handled in later phases.
fn const_to_fusevm(v: &StrykeValue) -> Option<fusevm::Value> {
    if let Some(i) = v.as_integer() {
        Some(fusevm::Value::Int(i))
    } else {
        v.as_float().map(fusevm::Value::Float)
    }
}

/// Convert a fusevm result Value back to a StrykeValue (Phase-1 subset).
fn fusevm_to_stryke(v: &fusevm::Value) -> Option<StrykeValue> {
    match v {
        fusevm::Value::Int(n) => Some(StrykeValue::integer(*n)),
        fusevm::Value::Float(f) => Some(StrykeValue::float(*f)),
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
            // End of the top-level program: stop lowering and let fusevm return
            // the value left on the stack.
            Op::Return | Op::Halt => break,
            // Anything else is outside the Phase-1 subset → fall back to vm.rs.
            _ => return None,
        }
    }

    let fchunk = b.build();
    let mut vm = fusevm::VM::new(fchunk);
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

    #[test]
    fn native_integer_arithmetic_matches_vm() {
        assert_parity_int("1 + 2 * 3", 7);
        assert_parity_int("10 - 4 - 3", 3);
        assert_parity_int("2 * 3 * 4", 24);
        assert_parity_int("-(2 + 3)", -5);
    }
}
