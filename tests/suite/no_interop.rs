//! `--no-interop` mode regression tests. Spawns the binary so the global
//! `NO_INTEROP_MODE` flag is process-isolated (parallel `cargo test` workers
//! can't race on it).
//!
//! Stryke's runtime binds positional reduce/sort/pair* args to BOTH `$a`/`$b`
//! (Perl-compatible) AND `$_0`/`$_1` (idiomatic stryke). In `--no-interop`
//! mode the Perl-compatible names are rejected at parse time so users learn
//! the idiomatic form. The check fires from two sites:
//!   1. Lexer (`strykelang/lexer.rs`) — `$a` / `$b` outside string interpolation.
//!   2. Parser interpolation (`strykelang/parser.rs::parse_interpolated_string`)
//!      — `"$a"`, `"${a}"`, `"$a[0]"`, etc.
//!
//! Both sites must reject so a user can't sneak `$a` past via interpolation.

use std::process::Command;

fn stryke() -> &'static str {
    env!("CARGO_BIN_EXE_st")
}

fn no_interop_run(code: &str) -> std::process::Output {
    Command::new(stryke())
        .args(["--no-interop", "-e", code])
        .output()
        .expect("spawn")
}

fn run_default(code: &str) -> std::process::Output {
    Command::new(stryke())
        .args(["-e", code])
        .output()
        .expect("spawn")
}

fn assert_rejects_dollar_a_or_b(out: &std::process::Output, name: &str) {
    assert!(
        !out.status.success(),
        "expected --no-interop to reject `${name}`; status={:?} stdout={} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("$_0") && stderr.contains("$_1") && stderr.contains(name),
        "diagnostic must mention both $_0/$_1 and the rejected name `{name}`: stderr={stderr}"
    );
}

// ── Lexer-level rejection (bare $a / $b in code) ──

#[test]
fn no_interop_rejects_bare_dollar_a() {
    assert_rejects_dollar_a_or_b(&no_interop_run("print $a"), "$a");
}

#[test]
fn no_interop_rejects_bare_dollar_b() {
    assert_rejects_dollar_a_or_b(&no_interop_run("print $b"), "$b");
}

#[test]
fn no_interop_rejects_my_dollar_a_declaration() {
    // Declaration is rejected — the name itself is the Perl-ism, not just
    // its use as a magic var.
    assert_rejects_dollar_a_or_b(&no_interop_run("my $a = 1"), "$a");
}

#[test]
fn no_interop_rejects_dollar_a_in_reduce_block() {
    // The original failure mode: users writing Perl-style reduce.
    assert_rejects_dollar_a_or_b(
        &no_interop_run("p reduce { $a + $b } 1..10"),
        "$a",
    );
}

// ── Parser interpolation-path rejection (inside `"…"`) ──

#[test]
fn no_interop_rejects_dollar_a_in_double_quoted_string() {
    assert_rejects_dollar_a_or_b(&no_interop_run(r#"print "v=$a""#), "$a");
}

#[test]
fn no_interop_rejects_dollar_b_in_double_quoted_string() {
    assert_rejects_dollar_a_or_b(&no_interop_run(r#"print "v=$b""#), "$b");
}

#[test]
fn no_interop_rejects_braced_dollar_a_in_string() {
    // `${a}` braced form goes through a separate parser branch.
    assert_rejects_dollar_a_or_b(&no_interop_run(r#"print "v=${a}""#), "$a");
}

#[test]
fn no_interop_rejects_dollar_a_with_subscript_in_string() {
    // `"$a[0]"` extends the bare-name path with `[idx]` — the check upstream
    // catches it before the subscript chain forms.
    assert_rejects_dollar_a_or_b(&no_interop_run(r#"print "v=$a[0]""#), "$a");
}

// ── Negative cases: must NOT over-reject ──

#[test]
fn no_interop_allows_longer_names_starting_with_a() {
    // Only exact `$a` / `$b` are blocked; `$apple`, `$abc`, `$ab`, `$a1` are fine.
    let out = no_interop_run(r#"my $apple = 5; my $abc = 7; my $a1 = 9; print $apple + $abc + $a1, "\n""#);
    assert!(
        out.status.success(),
        "longer names must not be over-rejected: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "21\n");
}

#[test]
fn no_interop_allows_longer_names_starting_with_b() {
    let out = no_interop_run(r#"my $bell = 1; my $bus = 2; print $bell + $bus, "\n""#);
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}

#[test]
fn no_interop_allows_dollar_underscore_zero_and_one_in_reduce() {
    // Stryke's runtime binds these positionally — they're the canonical
    // replacement for `$a` / `$b` in --no-interop.
    let out = no_interop_run(r#"p reduce { $_0 + $_1 } 1..10"#);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "55\n");
}

#[test]
fn no_interop_allows_dollar_underscore_zero_one_in_sort() {
    // `sort { … }` with `$_0` / `$_1` as positional comparator args.
    let out = no_interop_run(r#"print join(",", sort { $_0 <=> $_1 } 3, 1, 2), "\n""#);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1,2,3\n");
}

#[test]
fn no_interop_allows_escaped_dollar_a_literal() {
    // `\$a` in a double-quoted string is a literal `$a`, not interpolation.
    // The check must not fire on the literal text.
    let out = no_interop_run(r#"print "v=\$a""#);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "v=$a");
}

#[test]
fn no_interop_allows_dollar_a_in_single_quoted_string() {
    // Single-quoted strings don't interpolate at all, so `$a` is just text.
    let out = no_interop_run(r#"print 'v=$a'"#);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "v=$a");
}

// ── Default mode (no `--no-interop`) must keep working ──

#[test]
fn default_mode_allows_dollar_a_in_reduce() {
    let out = run_default(r#"p reduce { $a + $b } 1..10"#);
    assert!(
        out.status.success(),
        "default mode must allow $a/$b: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "55\n");
}

#[test]
fn default_mode_allows_dollar_a_in_string_interp() {
    let out = run_default(r#"my $a = 7; print "v=$a\n""#);
    assert!(
        out.status.success(),
        "default mode must allow `$a` in interp: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "v=7\n");
}
