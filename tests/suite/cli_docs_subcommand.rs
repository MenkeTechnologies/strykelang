//! Coverage for the `stryke docs` subcommand's non-interactive paths.
//!
//! Three behaviors that AI agents and CI scripts depend on:
//!   * `stryke docs <TOPIC>` dumps the page and exits 0 (one-shot,
//!     `man pmap`-style — no interactive TUI, no `q` to quit).
//!   * `stryke docs --toc` / `--list` / `--search PAT` exit cleanly
//!     with structured output, regardless of TTY.
//!   * `stryke docs` (no args) on a piped stdin or with
//!     `STRYKE_NO_TTY=1` dumps the intro page and exits 0 instead of
//!     blocking on a TUI loop.
//!
//! `run_docs` returns `None` only if spawning `stryke docs` fails (rare —
//! the exe path comes from [`env!("CARGO_BIN_EXE_stryke")]`, same as other
//! integration CLI suites).

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Path to the `stryke` executable built for this test run (`cargo test`
/// sets `CARGO_BIN_EXE_*`). Relative `target/debug/stryke` probes break when
/// the integration harness cwd is not the crate root — and can disagree with
/// `%b` / `--list` parity if a different `stryke` appears earlier on `$PATH`.
fn stryke_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stryke"))
}

/// Run `stryke docs ARGS...` with stdin piped (so `is_terminal()` on
/// stdin returns false — kicks the subcommand into non-interactive
/// mode).
fn run_docs(args: &[&str]) -> Option<(i32, String, String)> {
    let bin = stryke_binary();
    let mut cmd = Command::new(&bin);
    cmd.arg("docs").args(args).stdin(Stdio::null());
    let out = cmd.output().ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    ))
}

// ── Topic-name resolution (one-shot dump) ───────────────────────────────────

/// `stryke docs pmap` should print the pmap page and exit 0. Must NOT
/// enter the interactive TUI even when stdout is a tty (because a topic
/// argument is a one-shot lookup signal, like `man pmap`).
#[test]
fn docs_with_topic_dumps_and_exits_zero() {
    let Some((rc, stdout, _stderr)) = run_docs(&["pmap"]) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(rc, 0, "stryke docs pmap should exit 0, got {rc}");
    // pmap doc page mentions parallelism / rayon — pin a stable substring.
    let lower = stdout.to_lowercase();
    assert!(
        lower.contains("pmap") && (lower.contains("parallel") || lower.contains("rayon")),
        "expected pmap doc content, got first 200 chars: {:?}",
        stdout.chars().take(200).collect::<String>(),
    );
}

/// Page-number argument also dumps and exits.
#[test]
fn docs_with_page_number_dumps_and_exits_zero() {
    let Some((rc, stdout, _)) = run_docs(&["1"]) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(rc, 0);
    assert!(!stdout.is_empty(), "page 1 should produce output");
}

/// Unknown topic → exit 1 with a friendly stderr hint.
#[test]
fn docs_with_unknown_topic_exits_one() {
    let Some((rc, _stdout, stderr)) = run_docs(&["definitely_not_a_real_builtin_xyz"]) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(rc, 1);
    assert!(
        stderr.contains("no documentation for")
            || stderr.contains("definitely_not_a_real_builtin_xyz"),
        "expected unknown-topic hint in stderr, got: {stderr:?}",
    );
}

// ── Flags ────────────────────────────────────────────────────────────────────

/// `--list` enumerates every dispatch primary in `%b` exactly once
/// — pre-fix, dedup-by-text-pointer dropped ~288 primaries when a
/// hand-written hover entry was shared (`"sum" | "sum0" => "..."`
/// returned the same `&'static str` for both, so `sum0` was skipped).
/// The parity guarantee `--list ⊇ %b` is what makes "browse every
/// builtin" a useful affordance.
#[test]
fn docs_list_covers_every_dispatch_primary() {
    let Some((rc, stdout, _)) = run_docs(&["--list"]) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(rc, 0);
    let bin = stryke_binary();
    // Strip the ` 12. name` prefix and collect listed topics.
    let listed: std::collections::HashSet<String> = stdout
        .lines()
        .filter_map(|l| {
            let t = l.trim_start();
            // Format is `NN. name` — drop digits-and-dot prefix.
            let after_num = t.find(". ")?;
            Some(t[after_num + 2..].trim().to_string())
        })
        .collect();
    // Pull every primary from `%b` via the same binary.
    let primaries_out = std::process::Command::new(&bin)
        .args(["-e", r#"for (sort keys %b) { print "$_\n" }"#])
        .output()
        .expect("run %b dump");
    let primaries: std::collections::HashSet<String> =
        String::from_utf8_lossy(&primaries_out.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();
    let missing: Vec<&String> = primaries.difference(&listed).collect();
    assert!(
        missing.is_empty(),
        "%b has {} primaries missing from `s docs --list`: {:?}",
        missing.len(),
        missing.iter().take(5).collect::<Vec<_>>(),
    );
}

/// `--list` / `-l` emit one topic per line.
#[test]
fn docs_list_flag_emits_numbered_topics() {
    let Some((rc, stdout, _)) = run_docs(&["--list"]) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(rc, 0);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() > 50,
        "expected many topics, got {} lines",
        lines.len()
    );
    // Each line starts with `  N. name` (3-space-then-number-dot pattern).
    let numbered = lines.iter().filter(|l| l.contains(". ")).count();
    assert!(
        numbered > 50,
        "expected numbered topics, got {numbered} of {}",
        lines.len()
    );
}

/// `--search PATTERN` returns matching topics with `(category)` suffix.
#[test]
fn docs_search_flag_returns_matches() {
    let Some((rc, stdout, _)) = run_docs(&["--search", "parallel"]) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(rc, 0);
    let lower = stdout.to_lowercase();
    assert!(lower.contains("pmap"), "search 'parallel' should hit pmap");
    assert!(
        lower.contains("pgrep"),
        "search 'parallel' should hit pgrep"
    );
}

/// `--help` prints usage and exits 0.
#[test]
fn docs_help_flag_prints_usage() {
    let Some((rc, stdout, _)) = run_docs(&["--help"]) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(rc, 0);
    assert!(stdout.contains("USAGE") || stdout.to_lowercase().contains("usage"));
}

/// `--toc` / `-t` prints the table of contents and exits 0.
#[test]
fn docs_toc_flag_exits_zero() {
    let Some((rc, stdout, _)) = run_docs(&["--toc"]) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(rc, 0);
    assert!(!stdout.is_empty(), "--toc should produce output");
}

// ── No-args + non-interactive guards ────────────────────────────────────────

/// `stryke docs` with no args and a piped stdin (i.e.
/// CI-style invocation) dumps the intro page and exits 0 — never
/// enters the TUI loop. Pinned because the regression that prompted
/// BUG-112 broke this in three different shapes
#[test]
fn docs_no_args_with_piped_stdin_exits_zero() {
    let Some((rc, stdout, _)) = run_docs(&[]) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(rc, 0, "no-args + piped stdin should not enter TUI");
    assert!(
        stdout.contains("STRYKE ENCYCLOPEDIA")
            || stdout.contains("INTERACTIVE REFERENCE")
            || stdout.contains("Introduction"),
        "expected intro-page banner, got first 200: {:?}",
        stdout.chars().take(200).collect::<String>(),
    );
}

/// `STRYKE_NO_TTY=1` forces non-interactive mode even on a real
/// terminal — opt-out for users who want scripted-only behavior.
#[test]
fn docs_with_stryke_no_tty_env_exits_zero() {
    let bin = stryke_binary();
    let out = Command::new(&bin)
        .arg("docs")
        .arg("pmap")
        .env("STRYKE_NO_TTY", "1")
        .stdin(Stdio::null())
        .output()
        .expect("run stryke");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.to_lowercase().contains("pmap"));
}

/// `NO_TTY=1` (the generic form, no `STRYKE_` prefix) is honored too.
#[test]
fn docs_with_generic_no_tty_env_exits_zero() {
    let bin = stryke_binary();
    let out = Command::new(&bin)
        .arg("docs")
        .arg("pmap")
        .env("NO_TTY", "1")
        .stdin(Stdio::null())
        .output()
        .expect("run stryke");
    assert_eq!(out.status.code(), Some(0));
}

/// `stryke docs pmap | head -3` emits the first three lines and exits
/// cleanly — verifying the behavior the user originally hit when
#[test]
fn docs_pmap_pipes_to_head_cleanly() {
    let bin = stryke_binary();
    // Use sh to chain the pipe portably.
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("{} docs pmap | head -3", bin.display()))
        .output()
        .expect("run pipe");
    assert_eq!(
        out.status.code(),
        Some(0),
        "pipe to head should exit 0 (was: {:?})",
        out.status.code()
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // head -3 keeps the first 3 lines; can be at least 1 non-empty.
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(!lines.is_empty(), "head -3 produced nothing");
}
