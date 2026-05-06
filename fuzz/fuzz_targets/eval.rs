//! Fuzz the full pipeline: parse + compile + execute.
//!
//! Runs the program in a fresh `VMHelper` so global state (subs, packages, special
//! vars) doesn't leak across iterations. The harness asserts only that execution
//! doesn't panic — it doesn't constrain output, exit code, or runtime errors.
//!
//! Run under cargo-fuzz:
//!   cargo +nightly fuzz run eval
//!
//! Note: a malicious input can write `for (1..1e18) { ... }` and burn libfuzzer
//! time. We cap input size and rely on libfuzzer's `-timeout=N` flag for
//! per-input wall-clock limits (default 1200 s — pass `-timeout=10` for tighter
//! cycles).

#![no_main]

use libfuzzer_sys::fuzz_target;
use stryke::vm_helper::VMHelper;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if s.len() > 32_768 {
            return;
        }
        let Ok(program) = stryke::parse(s) else {
            return;
        };
        let mut interp = VMHelper::new();
        // Suppress stdout so fuzzer logs aren't drowned by `print` output.
        interp.suppress_stdout = true;
        let _ = stryke::try_vm_execute(&program, &mut interp);
    }
});
