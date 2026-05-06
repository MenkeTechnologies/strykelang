//! Fuzz the bytecode compiler.
//!
//! Parse + compile, but do NOT execute — the compiler has its own panic surface
//! (slot allocation, scope-stack bookkeeping, name-pool overflow, deferred-block
//! resolution). Inputs that parse but don't compile must error cleanly, never
//! panic.
//!
//! Run under cargo-fuzz:
//!   cargo +nightly fuzz run compile

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if s.len() > 65_536 {
            return;
        }
        let Ok(program) = stryke::parse(s) else {
            return;
        };
        let comp = stryke::compiler::Compiler::new();
        let _ = comp.compile_program(&program);
    }
});
