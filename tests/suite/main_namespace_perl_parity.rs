//! Perl-5 parity for `main::` reserved-variable semantics.
//!
//! Per the Perl Documentation: certain built-in identifiers are forced
//! into the main package — special filehandles (STDIN/STDOUT/STDERR/
//! ARGV/ARGVOUT), special arrays/hashes (@ARGV, @INC, %ENV, %SIG), and
//! every punctuation variable. Each lookup form below is run through
//! both `perl(1)` and `stryke --compat`; STDOUT must match
//! byte-for-byte. Without parity, `$main::!` / `$main::1` / `<main::STDIN>`
//! would have stryke-only behavior that diverges from user expectations.
//!
//! Skips silently when `perl(1)` isn't on `$PATH` — the suite is
//! conformance, not a hard requirement. The companion
//! `main_pkg_aliasing.rs` tests stryke-internal equality of the
//! bare-vs-qualified forms; this file proves stryke matches perl.
//!
//! Stdin-fed tests (`<STDIN>`, `<main::STDIN>`) provide identical
//! input bytes to both interpreters and assert the same stdout.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn stryke_binary() -> Option<PathBuf> {
    for candidate in ["target/release/stryke", "target/debug/stryke"] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn perl_available() -> bool {
    Command::new("perl").arg("-e").arg("1").output().is_ok()
}

/// Run `code` through `perl -e` and `stryke --compat -e`, return both
/// stdouts. Returns `None` when either interpreter is missing so the
/// caller skips cleanly.
fn parity_stdout(code: &str) -> Option<(String, String)> {
    if !perl_available() {
        return None;
    }
    let stryke = stryke_binary()?;
    let perl = Command::new("perl").arg("-e").arg(code).output().ok()?;
    let stk = Command::new(&stryke)
        .args(["--compat", "-e", code])
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&perl.stdout).to_string(),
        String::from_utf8_lossy(&stk.stdout).to_string(),
    ))
}

/// Same as [`parity_stdout`] but with a stdin payload — used for
/// `<main::STDIN>` / `<STDIN>` diamond reads. Both interpreters see
/// the same bytes.
fn parity_stdout_with_stdin(code: &str, stdin_data: &str) -> Option<(String, String)> {
    if !perl_available() {
        return None;
    }
    let stryke = stryke_binary()?;

    let mut perl = Command::new("perl")
        .arg("-e")
        .arg(code)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    perl.stdin.as_mut()?.write_all(stdin_data.as_bytes()).ok()?;
    let perl_out = perl.wait_with_output().ok()?;

    let mut stk = Command::new(&stryke)
        .args(["--compat", "-e", code])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    stk.stdin.as_mut()?.write_all(stdin_data.as_bytes()).ok()?;
    let stk_out = stk.wait_with_output().ok()?;

    Some((
        String::from_utf8_lossy(&perl_out.stdout).to_string(),
        String::from_utf8_lossy(&stk_out.stdout).to_string(),
    ))
}

macro_rules! parity {
    ($name:ident, $code:expr) => {
        #[test]
        fn $name() {
            let Some((perl, stk)) = parity_stdout($code) else {
                eprintln!("skip: perl(1) or stryke binary not available");
                return;
            };
            assert_eq!(
                perl, stk,
                "main:: parity regressed for:\n    {}\n\nperl stdout:\n{:?}\n\nstryke stdout:\n{:?}\n",
                $code, perl, stk,
            );
        }
    };
}

macro_rules! parity_stdin {
    ($name:ident, $code:expr, $stdin:expr) => {
        #[test]
        fn $name() {
            let Some((perl, stk)) = parity_stdout_with_stdin($code, $stdin) else {
                eprintln!("skip: perl(1) or stryke binary not available");
                return;
            };
            assert_eq!(
                perl, stk,
                "main:: parity (stdin) regressed for:\n    {}\n\nperl stdout:\n{:?}\n\nstryke stdout:\n{:?}\n",
                $code, perl, stk,
            );
        }
    };
}

// ── Punctuation variables — full parity in --compat ───────────────
//
// `stryke --compat` is byte-identical to perl(1). Perl 5.42's parser
// doesn't actually treat `$main::!` etc as the bare `$!` (despite the
// official docs claiming punctuation vars reside in main): it parses
// `$main::` as the empty-name var and leaves the punct as literal
// text. Stryke matches this exactly in compat mode by gating the
// lexer/interpolator punct-leaf pickup behind `!compat_mode()`.
// Default (non-compat) stryke implements the docs faithfully and is
// tested below as the strict-extensions block.

parity!(
    topic_var_qualified_equals_bare,
    r#"$_ = "topic"; print "[$_]|[$main::_]\n""#
);

parity!(
    errno_qualified_matches_perl_under_compat,
    r#"open(my $f, "<", "/nonexistent_path_xyzzy") or 1; print "[$!]|[$main::!]\n""#
);

parity!(
    eval_error_qualified_matches_perl_under_compat,
    r#"eval { die "boom\n" }; print "[$@]|[$main::@]\n""#
);

parity!(
    regex_capture_1_qualified_equals_bare,
    r#""abc" =~ /(b)/; print "[$1]|[$main::1]\n""#
);

parity!(
    regex_capture_2_qualified_equals_bare,
    r#""abc" =~ /(a)(b)/; print "[$2]|[$main::2]\n""#
);

parity!(
    irs_qualified_matches_perl_under_compat,
    r#"$/ = "X"; print "[$/]|[$main::/]\n""#
);

parity!(
    program_name_qualified_equals_bare,
    r#"print "[$0]|[$main::0]\n""#
);

// ── Special arrays / hashes: @ARGV, @INC, %ENV, %SIG ───────────────

parity!(
    argv_array_qualified_equals_bare,
    r#"@ARGV = ("a", "b", "c"); print join("|", @ARGV), "\n"; print join("|", @main::ARGV), "\n""#
);

parity!(
    inc_array_qualified_length_matches_bare,
    r#"print scalar(@INC) == scalar(@main::INC) ? "yes\n" : "no\n""#
);

parity!(
    env_hash_qualified_keys_match_bare,
    r#"my $a = join(",", sort keys %ENV); my $b = join(",", sort keys %main::ENV); print $a eq $b ? "yes\n" : "no\n""#
);

parity!(
    sig_hash_qualified_keys_match_bare,
    r#"my $a = scalar(keys %SIG); my $b = scalar(keys %main::SIG); print $a == $b ? "yes\n" : "no\n""#
);

// ── Special filehandles: STDIN, STDOUT, STDERR ─────────────────────
//
// Filehandles take the QUALIFIED form `main::STDOUT` directly (no
// sigil). Perl accepts both. stryke must too.

parity!(
    print_to_main_stdout_qualified_form,
    r#"print main::STDOUT "stdout-via-main\n""#
);

parity!(print_to_bare_stdout, r#"print STDOUT "stdout-bare\n""#);

parity!(
    printf_via_main_stdout,
    r#"printf main::STDOUT "%d=%s\n", 42, "answer""#
);

// ── Diamond operator: <STDIN> vs <main::STDIN> ─────────────────────

parity_stdin!(
    diamond_bare_stdin_reads_line,
    r#"my $line = <STDIN>; print "got=[$line]""#,
    "hello-world\n"
);

parity_stdin!(
    diamond_main_qualified_stdin_reads_line,
    r#"my $line = <main::STDIN>; print "got=[$line]""#,
    "hello-world\n"
);

parity_stdin!(
    diamond_main_qualified_stdin_matches_bare_form,
    // Two separate runs would lose state; instead read once via
    // qualified form and verify the value matches what bare would
    // see for the same input.
    r#"my $line = <main::STDIN>; chomp $line; print "[$line]\n""#,
    "parity-check-line\n"
);

// ── Open / close on `main::FH` ─────────────────────────────────────

parity!(
    open_close_print_via_main_qualified_fh,
    r#"my $p = "/tmp/stryke_perl_parity_main_fh.$$"; open(main::OUT, ">", $p) or die; print main::OUT "wrote-via-main\n"; close(main::OUT); open(my $r, "<", $p) or die; my $line = <$r>; close $r; unlink $p; print $line"#
);

// ── Scalar interpolation inside double-quoted strings ──────────────

parity!(
    interp_main_topic_in_dq_string,
    r#"$_ = "T"; print "main=$main::_\n""#
);

parity!(
    interp_main_capture1_in_dq_string,
    r#""abc" =~ /(b)/; print "cap=$main::1\n""#
);

// ── stryke extensions beyond Perl 5.42's parser ───────────────────
//
// Perl's official docs say "All punctuation variables like $_ reside
// in main." stryke implements this faithfully — `$main::!`,
// `$main::@`, `$main::/`, `$main::$` all canonicalize to the bare
// punctuation var via `strip_main_prefix`. Perl 5.42's parser
// disagrees: it treats `$main::!` as `$main::` (empty) followed by
// literal `!`. These tests pin the stryke-strict behavior under
// `--compat` and assert stryke gives the docs-faithful answer while
// noting that perl(1) does not.

/// Run stryke in DEFAULT mode (no `--compat`). Used by the
/// strict-extension tests below where stryke implements the Perl
/// docs more faithfully than Perl 5.42's parser actually does.
fn stryke_default_run(code: &str) -> Option<String> {
    let bin = stryke_binary()?;
    let out = std::process::Command::new(&bin)
        .args(["-e", code])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

#[test]
fn stryke_strict_main_qualified_errno_matches_bare() {
    let Some(stdout) = stryke_default_run(
        r#"open(my $f, "<", "/nope_path_xyzzy") or 1; print "[$!]|[$main::!]\n""#,
    ) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert!(
        stdout.contains("|[No such file or directory") || stdout.contains("|[No such file"),
        "$main::! must canonicalize to $! in non-compat (stryke docs-faithful extension): {stdout}",
    );
}

#[test]
fn stryke_strict_main_qualified_eval_error_matches_bare() {
    let Some(stdout) = stryke_default_run(
        r#"eval { die "boom\n" }; chomp(my $a = $@); chomp(my $b = $main::@); print "[$a]|[$b]\n""#,
    ) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(
        stdout.trim(),
        "[boom]|[boom]",
        "$main::@ must canonicalize to $@ in non-compat",
    );
}

#[test]
fn stryke_strict_main_qualified_irs_matches_bare() {
    let Some(stdout) = stryke_default_run(r#"$/ = "Z"; print "[$/]|[$main::/]\n""#) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(
        stdout.trim(),
        "[Z]|[Z]",
        "$main::/ must canonicalize to $/ in non-compat",
    );
}
