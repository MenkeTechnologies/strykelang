//! `--compat` arithmetic promotes to BigInt on i64 overflow; native stryke wraps.

use std::process::Command;

fn run_compat(code: &str) -> String {
    let exe = env!("CARGO_BIN_EXE_st");
    let out = Command::new(exe)
        .args(["--compat", "-e", code])
        .output()
        .expect("spawn stryke");
    assert!(out.status.success(), "stryke --compat failed: {:?}", out);
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn run_native(code: &str) -> String {
    let exe = env!("CARGO_BIN_EXE_st");
    let out = Command::new(exe)
        .args(["-e", code])
        .output()
        .expect("spawn stryke");
    assert!(out.status.success(), "stryke native failed: {:?}", out);
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn compat_2_to_the_100_via_repeated_mul() {
    assert_eq!(
        run_compat("my $x = 1; for (1..100) { $x *= 2 } print $x"),
        "1267650600228229401496703205376"
    );
}

#[test]
fn compat_2_to_the_100_via_pow_op() {
    assert_eq!(
        run_compat("print 2 ** 100"),
        "1267650600228229401496703205376"
    );
}

#[test]
fn compat_factorial_30() {
    assert_eq!(
        run_compat("my $f = 1; $f *= $_ for 1..30; print $f"),
        "265252859812191058636308480000000"
    );
}

#[test]
fn compat_add_overflow_promotes() {
    // i64::MAX + 1 = 9223372036854775808
    assert_eq!(
        run_compat("print 9223372036854775807 + 1"),
        "9223372036854775808"
    );
}

#[test]
fn compat_sub_overflow_promotes() {
    // (-i64::MAX) - 2 = -9223372036854775809 — phrased to dodge lexer's rejection
    // of `-9223372036854775808` as a single literal (it would overflow i64 by 1
    // before the unary minus folds in).
    assert_eq!(
        run_compat("print -9223372036854775807 - 2"),
        "-9223372036854775809"
    );
}

#[test]
fn compat_bigint_round_trips_through_int_when_fits() {
    // Result fits back in i64 — should display as a regular integer, no leading zero etc.
    assert_eq!(run_compat("print 2 ** 30"), "1073741824");
}

#[test]
fn native_mode_still_overflows_to_zero() {
    // Preserves existing behavior — bigint promotion is gated to --compat.
    assert_eq!(
        run_native("my $x = 1; for (1..100) { $x *= 2 } print $x"),
        "0"
    );
}

#[test]
fn compat_bigint_equality_with_string() {
    // Cross-type compare works because Display and PartialEq are wired up.
    assert_eq!(
        run_compat(r#"my $x = 2 ** 64; print $x eq "18446744073709551616" ? "y" : "n""#),
        "y"
    );
}

#[test]
fn compat_bigint_truthiness() {
    assert_eq!(run_compat("my $x = 2 ** 100; print $x ? 1 : 0"), "1");
    assert_eq!(run_compat("my $x = 0; $x = 2 ** 100 - 2 ** 100; print $x ? 1 : 0"), "0");
}
