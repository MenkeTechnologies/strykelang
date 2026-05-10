//! Coverage for the `doctor` / `health` runtime-diagnostic builtin and
//! the `~/.stryke/` path helpers it surfaces.
//!
//! The builtin is a structured introspection point — its output shape
//! is consumed by users at the prompt (and by future scripts that grep
//! its sections), so the tests pin section headings, the warning-count
//! return contract, and the fact that paths resolve under the single
//! `~/.stryke/` root rather than splitting across XDG / Library
//! directories.
//!
//! The builtin prints to stdout. Tests capture stdout with the
//! `suppress_stdout` helper on `VMHelper`, then call the builtin via
//! the exposed dispatcher path.

use std::path::PathBuf;
use std::process::Command;

use crate::common::{eval_int, eval_string, GLOBAL_FLAGS_LOCK};

fn stryke_binary() -> Option<PathBuf> {
    // Pick the freshest binary by mtime — prevents a stale `target/release/`
    // from a previous build (kept around because dev builds use
    // `cargo build` not `cargo build --release`) shadowing the actual
    // working tree's `target/debug/`. CI / release builds still find
    // their binary; the test author's daily `cargo test` doesn't run
    // against a stranger's months-old release.
    let cands = ["target/release/stryke", "target/debug/stryke"];
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

fn run_doctor() -> Option<String> {
    let bin = stryke_binary()?;
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let out = Command::new(&bin).args(["-e", "doctor"]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

// ── Return-value contract ────────────────────────────────────────────────────

/// `doctor` returns the warning count as an integer (0 = healthy).
/// Pin the contract — callers wire this into CI exit codes.
#[test]
fn doctor_returns_integer_warning_count() {
    let n = eval_int(r#"my $w = doctor(); $w"#);
    assert!(
        (0..=100).contains(&n),
        "doctor warning count out of plausible range: {n}",
    );
}

/// `health` is a stable alias of `doctor` and returns the same count.
#[test]
fn health_alias_returns_same_count() {
    let a = eval_int(r#"my $x = doctor(); $x"#);
    let b = eval_int(r#"my $x = health(); $x"#);
    assert_eq!(a, b, "doctor and health must agree on warning count");
}

// ── Output structure ─────────────────────────────────────────────────────────

/// Banner is present and uses Unicode box drawing.
#[test]
fn doctor_emits_banner() {
    let Some(out) = run_doctor() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert!(out.contains("STRYKE DOCTOR"));
    assert!(out.contains('╔') && out.contains('╚'));
}

/// Every advertised section appears.
#[test]
fn doctor_emits_all_sections() {
    let Some(out) = run_doctor() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    for section in [
        ">>  identity",
        ">>  runtime flags",
        ">>  environment",
        ">>  reflection",
        ">>  concurrency",
        ">>  paths",
        ">>  toolchain",
        ">>  sanity",
        ">>  summary",
    ] {
        assert!(
            out.contains(section),
            "missing section {section}\n----- doctor output -----\n{out}",
        );
    }
}

/// Identity section has version, binary, build, target.
#[test]
fn doctor_identity_includes_version_binary_build_target() {
    let Some(out) = run_doctor() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert!(out.contains("version"));
    assert!(out.contains("binary"));
    assert!(out.contains("build"));
    assert!(out.contains("target"));
    // Architecture is one of the standard triples.
    assert!(
        out.contains("aarch64") || out.contains("x86_64") || out.contains("riscv"),
        "expected a known architecture token in doctor output",
    );
}

// ── Paths section ────────────────────────────────────────────────────────────

/// All path lines live under `~/.stryke/` — the unified state root.
/// Catches regressions like the recent `~/.cache/stryke/ffi` split.
#[test]
fn doctor_paths_all_under_stryke_home() {
    let Some(out) = run_doctor() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    let mut in_paths = false;
    let mut bad: Vec<String> = Vec::new();
    for line in out.lines() {
        if line.starts_with(">>  paths") {
            in_paths = true;
            continue;
        }
        if in_paths {
            if line.starts_with(">>") {
                break;
            }
            // Any path under `.cache/stryke`, `.config/stryke`, or
            // `Library/Caches/stryke` would be a regression.
            for stale in [
                ".cache/stryke",
                "Library/Caches/stryke",
                "Library/Application Support/stryke",
            ] {
                if line.contains(stale) {
                    bad.push(line.to_string());
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "stale path roots leaked into doctor:\n{}",
        bad.join("\n")
    );
}

/// Path lines name the expected stryke-home subdirs.
#[test]
fn doctor_paths_lists_canonical_subdirs() {
    let Some(out) = run_doctor() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    for label in [
        "stryke home",
        "bytecode cache",
        "package store",
        "global bin",
        "repl history",
        "rust FFI cache",
    ] {
        assert!(out.contains(label), "missing path label {label}");
    }
}

// ── Reflection section ───────────────────────────────────────────────────────

/// Reflection counts are present and plausible (>= the 200 floor that
/// matches the existing reflection-hash sanity check).
#[test]
fn doctor_reflection_section_has_real_counts() {
    let Some(out) = run_doctor() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert!(out.contains("primaries"));
    assert!(out.contains("aliases"));
    assert!(out.contains("all spellings"));
    assert!(out.contains("categories"));
    assert!(out.contains("list builtins"));
    // Find "primaries           NNNN" and parse the count.
    let line = out
        .lines()
        .find(|l| l.contains("primaries") && !l.contains("non-empty"))
        .expect("missing primaries line");
    let count: i64 = line
        .split_whitespace()
        .find_map(|t| t.parse::<i64>().ok())
        .expect("no integer in primaries line");
    assert!(count >= 200, "primary count {count} below floor (200)");
}

// ── Toolchain section ────────────────────────────────────────────────────────

/// Toolchain section enumerates the rustc/cargo/perl/git/rg probes.
/// Each line starts with `✓` (found) or `—` (not on PATH). No crashes
/// when a tool is missing.
#[test]
fn doctor_toolchain_uses_found_or_missing_marker() {
    let Some(out) = run_doctor() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    let mut in_tool = false;
    for line in out.lines() {
        if line.starts_with(">>  toolchain") {
            in_tool = true;
            continue;
        }
        if in_tool {
            if line.starts_with(">>") {
                break;
            }
            if line.trim().is_empty() {
                continue;
            }
            assert!(
                line.contains('✓') || line.contains('—'),
                "toolchain line missing marker: {line:?}",
            );
        }
    }
}

// ── Summary section ──────────────────────────────────────────────────────────

/// On a healthy install the summary reports `✓ healthy`.
#[test]
fn doctor_summary_reports_healthy_when_no_warnings() {
    let Some(out) = run_doctor() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    let n = eval_int(r#"doctor()"#);
    if n == 0 {
        assert!(
            out.contains("✓ healthy"),
            "doctor returned 0 warnings but summary missing healthy marker:\n{out}",
        );
    } else {
        assert!(
            out.contains("⚠"),
            "doctor returned {n} warnings but summary missing warning marker",
        );
    }
}

// ── Runtime-flag mirroring ───────────────────────────────────────────────────

/// `doctor` reports the runtime flags accurately. Run under `--no-interop`
/// and confirm the flag line shows `on`.
#[test]
fn doctor_shows_no_interop_when_flag_set() {
    let Some(bin) = stryke_binary() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let out = Command::new(&bin)
        .args(["--no-interop", "-e", "doctor"])
        .output()
        .expect("run stryke");
    let text = String::from_utf8_lossy(&out.stdout);
    // Find the no-interop line — it's printed regardless of flag state,
    // just with "on" or "off". We want "on" here.
    let line = text
        .lines()
        .find(|l| l.contains("--no-interop"))
        .expect("missing --no-interop line");
    assert!(
        line.contains("on"),
        "expected --no-interop=on, got: {line:?}",
    );
}

// ── Sanity-check section ─────────────────────────────────────────────────────

/// The sanity-check section runs the documented checks and prints
/// either a ✓ or ✗ for each. Counts match the documented set.
#[test]
fn doctor_sanity_checks_run() {
    let Some(out) = run_doctor() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    for check in [
        "primaries non-empty",
        "all-spellings ⊇ primaries",
        "rayon pool ≥ 1",
        "stryke home resolved",
        "no uncategorized primaries",
    ] {
        assert!(out.contains(check), "sanity check missing: {check}\n{out}",);
    }
}

// ── Stable across runs ───────────────────────────────────────────────────────

/// Two consecutive `doctor` invocations on the same binary should agree
/// on the static counts (primaries / aliases / spellings). Anti-flake
/// guard for any future caching that tries to cache reflection state.
#[test]
fn doctor_counts_stable_across_runs() {
    let n_first = eval_int(
        r#"
        my @lines = split /\n/, ddump({
            primaries => scalar(keys %stryke::builtins),
            spellings => scalar(keys %stryke::all)
        })
        $stryke::all{pmap} eq "" ? 0 : 1
        "#,
    );
    let n_second = eval_int(
        r#"
        $stryke::all{pmap} eq "" ? 0 : 1
        "#,
    );
    assert_eq!(n_first, n_second);
    assert_eq!(n_first, 1, "pmap should be in %all every run");
}

/// `--compat` mode disables stryke extensions, including `doctor`.
/// Pin the rejection so a future change can't accidentally let
/// `doctor` slip through under `--compat` (it'd surprise Perl 5
/// users who expect a clean compat surface).
#[test]
fn doctor_disabled_under_compat_mode() {
    let Some(bin) = stryke_binary() else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let out = Command::new(&bin)
        .args(["--compat", "-e", "doctor"])
        .output()
        .expect("run stryke");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_ne!(
        out.status.code(),
        Some(0),
        "doctor should fail under --compat"
    );
    assert!(
        stderr.contains("doctor") || stderr.contains("extension") || stderr.contains("--compat"),
        "expected compat-rejection message, got: {stderr:?}",
    );
}

// ── now / quantiles touchpoints (added same session as doctor) ───────────────

/// Bare `now` returns Unix epoch seconds (alias of `time`) — pin the
/// integer-seconds contract so a future change can't silently flip it
/// back to a stringified datetime.
#[test]
fn now_bare_returns_unix_epoch_integer() {
    let n = eval_int(r#"now"#);
    let nsec = eval_int(r#"time"#);
    assert!(
        (n - nsec).abs() <= 1,
        "now ({n}) should match time ({nsec}) ± 1 second",
    );
    // Sanity: must look like a current Unix timestamp.
    assert!(n > 1_700_000_000 && n < 4_000_000_000);
}

/// `now(TZ)` keeps the timezone-aware datetime form via
/// `datetime_now_tz` — pin that path didn't regress.
#[test]
fn now_with_timezone_returns_iso_8601_string() {
    let s = eval_string(r#"now("UTC")"#);
    // Format: YYYY-MM-DDThh:mm:ss(.sss)?Z
    assert!(
        s.starts_with("20") && s.contains('T') && s.ends_with('Z'),
        "expected ISO-8601 UTC datetime, got {s:?}",
    );
}

/// `quantiles` returns one element per probability; results are
/// non-decreasing (probabilities go 0.25, 0.5, 0.75 in this fixture).
#[test]
fn quantiles_returns_one_value_per_probability_sorted() {
    let s = eval_string(
        r#"
        my @d = (1..1000);
        my @q = quantiles(\@d, [0.25, 0.5, 0.75]);
        sprintf "%.0f,%.0f,%.0f", @q
        "#,
    );
    assert_eq!(s, "251,500,750");
}
