//! IDE-strict static-analysis sweep over every `examples/*.stk` file.
//!
//! The JetBrains plugin's LSP runs `analyze_program_with_strict(..., strict_vars=true)`
//! on every open buffer regardless of whether the file contains `use strict;` —
//! see `strykelang/lsp.rs:compute_diagnostics`. Editors are where typos surface,
//! so the diagnostic walker is always strict in that context.
//!
//! The CLI's `--lint` mode is lenient (strict only fires if the source opts in
//! via `use strict;`). That asymmetry means an example can pass `cargo test
//! --test integration -- demos_no_interop` while still lighting up red in the
//! IDE. This pin closes the gap: every `examples/*.stk` file is analyzed with
//! `strict_vars=true` exactly the way the LSP does, and any new file or edit
//! that trips strict-vars or strict-subs fails the CI run before it merges.
//!
//! What this catches:
//!   * `while (my $x = …) { … $x … }` and `if (my $row = …) { … $row … }` —
//!     `MyExpr` declarations that were silently dropped from scope tracking
//!     before this commit's static_analysis.rs fix.
//!   * Stryke reflection hashes (`%all`, `%limits`, `%pc`, `%stryke::*`) that
//!     the strict-vars allowlist didn't know about.
//!   * Order-dependent package-global references in fn bodies — `our` decls
//!     must precede the fn that references them at lint time.
//!   * Subs / package globals created dynamically via `source($lib)` — the
//!     analyzer can't see inside string literals, so any demo using that
//!     pattern must predeclare stubs.

use std::fs;
use std::path::PathBuf;

use stryke::parse_with_file;
use stryke::static_analysis::analyze_program_with_strict;

fn examples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("examples");
    p
}

/// Mirror the LSP's `compute_diagnostics` exactly: prepend `use strict;` to
/// force the strict-vars walker on regardless of what the file declares.
/// We can't simply call `analyze_program_with_strict(..., true)` and skip the
/// pragma because strict_vars is also gated on the parser-detected `strict`
/// state of the file — the cleanest reproduction of the editor diagnostic is
/// to make the file itself opt in.
fn lint_strict(path: &PathBuf) -> Result<(), String> {
    let original = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let with_strict = format!("use strict;\n{}", original);
    let display = path.display().to_string();
    let program =
        parse_with_file(&with_strict, &display).map_err(|e| format!("parse error: {e}"))?;
    analyze_program_with_strict(&program, &display, true)
        .map_err(|e| format!("strict-mode static-analysis error: {e}"))
}

#[test]
fn every_example_passes_ide_strict_lint() {
    let dir = examples_dir();
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
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
        if let Err(msg) = lint_strict(path) {
            failures.push((path.clone(), msg));
        }
    }

    if !failures.is_empty() {
        let report: String = failures
            .iter()
            .map(|(p, m)| format!("  {}\n    {}\n", p.display(), m))
            .collect();
        panic!(
            "{} of {} examples failed IDE-strict lint:\n{}",
            failures.len(),
            entries.len(),
            report
        );
    }
}
