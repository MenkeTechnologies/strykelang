//! CLI tests for the `s` and `st` binaries.
//!
//! Both are 1-line `include!("../main.rs")` wrappers over the main
//! `stryke` binary (see `Cargo.toml:21-27`); the only thing they
//! contribute is their argv[0] basename, which the REPL gate at
//! `strykelang/main.rs:1737-1743` reads to decide whether `no args`
//! means "drop into REPL" vs "syntax error". Without these tests the
//! `[[bin]] s` and `[[bin]] st` targets only get built (`cargo
//! build --bins`) but never *exercised* — a regression to the
//! `include!` line or to the argv[0] dispatch lands silently.
//!
//! Tests run the binaries directly via `CARGO_BIN_EXE_<name>` (set
//! by cargo for any bin target in the same package); each scenario
//! is one-shot and independent.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn bin(name: &str) -> PathBuf {
    // `env!("CARGO_BIN_EXE_<name>")` is populated by cargo at
    // compile-time for every `[[bin]]` target in the current package.
    // Use the runtime form so test binaries don't fail to link when
    // a bin target is renamed.
    let var = format!("CARGO_BIN_EXE_{}", name);
    PathBuf::from(std::env::var(var).expect("CARGO_BIN_EXE_* set by cargo"))
}

/// Run `bin_name` with `args`, optional stdin, capture stdout/stderr/exit.
/// Never panics on failure — returns the components so the test can
/// assert on whatever shape it wants. Timeout caps wall-clock at 30s
/// to keep a stuck binary from hanging CI.
fn run(bin_name: &str, args: &[&str], stdin_input: Option<&str>) -> (String, String, i32) {
    let mut cmd = Command::new(bin(bin_name));
    cmd.args(args);
    if stdin_input.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn bin");
    if let Some(input) = stdin_input {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin pipe")
            .write_all(input.as_bytes())
            .expect("write stdin");
    }
    // Polling-loop timeout — std doesn't expose wait_timeout on Child.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().expect("wait_with_output");
                return (
                    String::from_utf8_lossy(&output.stdout).into_owned(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                    status.code().unwrap_or(-1),
                );
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    panic!("{bin_name} {:?} timed out", args);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("try_wait failed: {e}"),
        }
    }
}

// ─── `s` binary ─────────────────────────────────────────────────────

#[test]
fn s_help_flag_prints_usage_and_exits_zero() {
    let (out, _err, code) = run("s", &["--help"], None);
    assert_eq!(code, 0, "--help must exit 0");
    // clap-generated usage line — present regardless of subcommand layout.
    assert!(
        out.to_lowercase().contains("usage") || out.to_lowercase().contains("options"),
        "expected clap help output, got stdout: {out:?}"
    );
}

#[test]
fn s_eval_expression_prints_result() {
    // `-e 'print 1+2'` — happy path; should print `3` to stdout.
    let (out, _err, code) = run("s", &["-e", "print 1+2"], None);
    assert_eq!(code, 0, "happy-path -e must exit 0");
    assert!(out.contains("3"), "expected `3` in stdout, got {out:?}");
}

#[test]
fn s_missing_script_file_errors_nonzero() {
    let (_out, err, code) = run("s", &["/__zshrs_definitely_not_a_file__.pl"], None);
    assert_ne!(code, 0, "missing file must exit nonzero");
    // Don't pin the exact error message — `Can't open` / `No such file` /
    // platform-specific text varies. Just require *something* on stderr.
    assert!(!err.is_empty(), "expected stderr diagnostic, got empty");
}

#[test]
fn s_unknown_flag_errors_nonzero() {
    let (_out, err, code) = run("s", &["--definitely-not-a-real-flag"], None);
    assert_ne!(code, 0, "unknown flag must exit nonzero");
    // clap puts unknown-arg errors on stderr.
    assert!(
        err.contains("--definitely-not-a-real-flag")
            || err.to_lowercase().contains("unexpected")
            || err.to_lowercase().contains("unrecognized")
            || err.to_lowercase().contains("error"),
        "expected clap diagnostic mentioning the flag, got stderr: {err:?}"
    );
}

#[test]
fn s_stdin_script_via_dash_e_with_arith() {
    // Combine `-e` with non-trivial expression — exercises the lexer
    // + parser + executor end-to-end through the `s` binary entry.
    let (out, _err, code) = run("s", &["-e", "my $x = 2 ** 8; print $x"], None);
    assert_eq!(code, 0);
    assert!(out.contains("256"), "got stdout: {out:?}");
}

// ─── `st` binary ────────────────────────────────────────────────────

#[test]
fn st_help_flag_prints_usage_and_exits_zero() {
    let (out, _err, code) = run("st", &["--help"], None);
    assert_eq!(code, 0);
    assert!(
        out.to_lowercase().contains("usage") || out.to_lowercase().contains("options"),
        "expected clap help output, got: {out:?}"
    );
}

#[test]
fn st_eval_expression_prints_result() {
    let (out, _err, code) = run("st", &["-e", "print uc('hi')"], None);
    assert_eq!(code, 0);
    assert!(out.contains("HI"), "expected `HI` in stdout, got {out:?}");
}

#[test]
fn st_missing_script_file_errors_nonzero() {
    let (_out, err, code) = run("st", &["/__zshrs_definitely_not_a_file__.pl"], None);
    assert_ne!(code, 0);
    assert!(!err.is_empty());
}

#[test]
fn st_unknown_flag_errors_nonzero() {
    let (_out, err, code) = run("st", &["--definitely-not-a-real-flag"], None);
    assert_ne!(code, 0);
    assert!(
        err.contains("--definitely-not-a-real-flag")
            || err.to_lowercase().contains("unexpected")
            || err.to_lowercase().contains("unrecognized")
            || err.to_lowercase().contains("error"),
        "got stderr: {err:?}"
    );
}

#[test]
fn st_string_concat_via_dash_e() {
    let (out, _err, code) = run("st", &["-e", "print 'foo' . 'bar'"], None);
    assert_eq!(code, 0);
    assert!(out.contains("foobar"), "got stdout: {out:?}");
}

// ─── Cross-binary parity ────────────────────────────────────────────

#[test]
fn s_and_st_produce_identical_output_for_same_script() {
    // `s` and `st` are `include!("../main.rs")` aliases — given the
    // same `-e`, they MUST produce byte-identical stdout.
    let script = "my @a = (1,2,3); print join(',', @a)";
    let (s_out, _, s_code) = run("s", &["-e", script], None);
    let (st_out, _, st_code) = run("st", &["-e", script], None);
    assert_eq!(s_code, st_code, "exit-code divergence: s={s_code}, st={st_code}");
    assert_eq!(
        s_out, st_out,
        "stdout divergence between s and st aliases — `include!` form drifted?\n\
         s:  {s_out:?}\n\
         st: {st_out:?}"
    );
}
