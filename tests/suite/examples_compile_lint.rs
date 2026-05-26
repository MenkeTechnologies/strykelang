//! Compile-bytecode sweep across every `examples/*.stk` file. Companion
//! to `examples_strict_lint.rs`:
//!
//!   * `examples_strict_lint` does parse + IDE-strict static analysis (the
//!     LSP diagnostic path). Catches scoping / undefined-var / sigil-class
//!     errors before runtime.
//!   * `examples_compile_lint` (this file) does parse + bytecode compile
//!     (the runtime gate). Catches `Unsupported` constructs the compiler
//!     bails on AT compile time — e.g. the `my @rows = …` in expression
//!     position restriction at `compiler.rs:8367` that no amount of static
//!     analysis can foresee.
//!
//! Without this pin a demo can pass IDE-strict lint, look correct in the
//! editor, and still die with `compile: Unsupported …` on `cargo run`. The
//! IDE doesn't run a compile-bytecode pass for performance reasons, so this
//! sweep is the only catch for that class of regression.
//!
//! Implementation: subprocess `./target/debug/st --lint $file` per example.
//! That binary is freshly built whenever any source file changes, and the
//! `--lint` flag means parse + compile bytecode without running — exactly
//! the right gate. Skipped if no built binary exists (mirrors
//! `demos_no_interop.rs` policy — local-dev workflows that haven't run
//! `cargo build` yet aren't penalised).

use std::path::PathBuf;
use std::process::Command;

fn examples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("examples");
    p
}

fn stryke_binary() -> Option<PathBuf> {
    let cands = [
        "target/debug/st",
        "target/release/st",
        "target/debug/stryke",
        "target/release/stryke",
    ];
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for cand in cands {
        let p = PathBuf::from(cand);
        if let Ok(meta) = std::fs::metadata(&p) {
            if let Ok(m) = meta.modified() {
                if best.as_ref().is_none_or(|(_, t)| m > *t) {
                    best = Some((p, m));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

#[test]
fn every_example_compiles_to_bytecode_cleanly() {
    let bin = match stryke_binary() {
        Some(b) => b,
        None => {
            eprintln!(
                "warning: no built stryke binary under target/{{debug,release}} — \
                 examples_compile_lint sweep skipped. Run `cargo build` first."
            );
            return;
        }
    };
    let dir = examples_dir();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read examples dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("stk"))
        .collect();
    entries.sort();
    assert!(
        !entries.is_empty(),
        "no .stk files found under {} — paths wrong?",
        dir.display()
    );

    let mut failures: Vec<(PathBuf, String)> = Vec::new();
    for path in &entries {
        let output = Command::new(&bin)
            .arg("--lint")
            .arg(path)
            .output()
            .unwrap_or_else(|e| panic!("invoke {}: {e}", bin.display()));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // `--lint` prints "$file compile OK" on success — to STDERR, not
        // stdout (verified empirically: success prints 0 bytes to stdout
        // and ~42 bytes to stderr). On failure stryke prints the diagnostic
        // (also to stderr) plus `Execution of $file aborted due to compilation
        // errors.`. We treat exit-status as the source of truth and use the
        // streams only to surface the first error line in the failure report.
        if !output.status.success() {
            let msg = stderr
                .lines()
                .find(|l| !l.is_empty() && !l.contains("Execution"))
                .or_else(|| stdout.lines().next())
                .unwrap_or("(no output)")
                .to_string();
            failures.push((path.clone(), msg));
        }
    }

    if !failures.is_empty() {
        let report: String = failures
            .iter()
            .map(|(p, m)| format!("  {}\n    {}\n", p.display(), m))
            .collect();
        panic!(
            "{} of {} examples failed --lint (compile bytecode):\n{}",
            failures.len(),
            entries.len(),
            report
        );
    }
}
