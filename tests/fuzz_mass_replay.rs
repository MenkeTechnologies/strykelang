//! Mass-replay the parse + compile fuzz logic on every committed `.stk` file.
//!
//! `tests/fuzz_smoke.rs` covers the small seed corpus. This harness widens the
//! input set to the entire `examples/`, `parity/cases/`, and `tests/data/` trees —
//! ~1800 files — to surface panics in parse and compile that the seeds miss.
//! Slow tests (`#[ignore]` by default) — run with `cargo test --test
//! fuzz_mass_replay -- --ignored` when bug-hunting.
//!
//! Eval is NOT mass-replayed: many `.stk` files do real work (HTTP fetches,
//! file IO, deep recursion) so executing all 1800 takes minutes and surfaces
//! user-program issues rather than stryke panics. Parse + compile run in
//! ~0.4s combined and target the actual bug class libfuzzer would.

use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use stryke::compiler::Compiler;

fn collect_stk_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip target/ and .git/ — they're huge and never contain source files.
            if matches!(
                path.file_name().and_then(|s| s.to_str()),
                Some("target") | Some(".git")
            ) {
                continue;
            }
            collect_stk_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("stk") {
            out.push(path);
        }
    }
}

fn all_stk_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for sub in ["examples", "parity/cases", "tests/data"] {
        let p = root.join(sub);
        if p.exists() {
            collect_stk_files(&p, &mut files);
        }
    }
    files.sort();
    files
}

/// Run `f` and report — never panic. Returns `Some((path, msg))` for any input
/// that panicked under `f`, so the caller can build a single failure report
/// listing every offending file (rather than dying on the first one).
fn try_each<F: Fn(&str) + Sync>(label: &str, files: &[PathBuf], f: F) -> Vec<(PathBuf, String)> {
    let mut panics = Vec::new();
    for path in files {
        let Ok(bytes) = fs::read(path) else { continue };
        let Ok(s) = std::str::from_utf8(&bytes) else {
            continue;
        };
        // Cap individual files at 256 KB — a few examples are megabytes (test
        // fixtures, generated data). Very large inputs add minutes without new
        // signal; the regression hunt is for unexpected panics, not big-input
        // perf issues.
        if s.len() > 262_144 {
            continue;
        }
        let result = catch_unwind(AssertUnwindSafe(|| f(s)));
        if let Err(e) = result {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&'static str>().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("<non-string panic in {label}>"));
            panics.push((path.clone(), msg));
        }
    }
    panics
}

fn report(label: &str, panics: Vec<(PathBuf, String)>) {
    if panics.is_empty() {
        return;
    }
    let mut msg = format!("{} panics ({} files):\n", label, panics.len());
    for (p, e) in panics.iter().take(20) {
        msg.push_str(&format!("  {}\n    → {}\n", p.display(), e));
    }
    if panics.len() > 20 {
        msg.push_str(&format!("  … {} more\n", panics.len() - 20));
    }
    panic!("{}", msg);
}

#[test]
#[ignore]
fn parse_mass_replay() {
    let files = all_stk_files();
    let panics = try_each("parse", &files, |s| {
        let _ = stryke::parse(s);
    });
    report("parse", panics);
}

#[test]
#[ignore]
fn compile_mass_replay() {
    let files = all_stk_files();
    let panics = try_each("compile", &files, |s| {
        let Ok(program) = stryke::parse(s) else {
            return;
        };
        let _ = Compiler::new().compile_program(&program);
    });
    report("compile", panics);
}
