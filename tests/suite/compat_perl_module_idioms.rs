//! `--compat` regressions for idioms that core Perl modules and ordinary Perl
//! code lean on. Every expected string is stock `perl` output for the same
//! program.
//!
//! `--compat` is process-global, so each case runs the real `st` binary in a
//! subprocess — the same way `parity/run_parity.sh` does.

use std::process::Command;

/// Run `st --compat -e CODE` and return stdout, asserting the run succeeded.
fn compat(code: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_st"))
        .args(["--compat", "-e", code])
        .env("STRYKE_CACHE", "0")
        .output()
        .expect("spawn st");
    assert!(
        out.status.success(),
        "st --compat -e {code:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ── `shift->...` / `pop->...` ────────────────────────────────────────────────

#[test]
fn shift_arrow_dereferences_the_shifted_value() {
    assert_eq!(
        compat("sub x { shift->{x} } sub h { shift->[1] } print x({x => 7}), h([1, 9])"),
        "79"
    );
    assert_eq!(compat("sub l { pop->[-1] } print l([1], [5, 6])"), "6");
}

#[test]
fn shift_arrow_method_call_is_the_oo_accessor_idiom() {
    assert_eq!(
        compat(
            "package P; sub new { bless {v => $_[1]}, $_[0] } sub v { shift->{v} } \
             sub twice { shift->v * 2 } package main; print P->new(4)->twice"
        ),
        "8"
    );
}

// ── `map(...)` / `grep(...)` parenthesized call form ────────────────────────

#[test]
fn map_and_grep_accept_the_parenthesized_call_form() {
    assert_eq!(compat(r#"print join ",", map("<$_>", 1, 2)"#), "<1>,<2>");
    assert_eq!(compat(r#"print join ",", map({ $_ * 10 } 1, 2)"#), "10,20");
    assert_eq!(compat(r#"print join ",", grep(/a/, qw(ab cd ba))"#), "ab,ba");
    assert_eq!(compat(r#"print join ",", grep({ $_ > 1 } 1, 2, 3)"#), "2,3");
    // The closing paren ends the argument list, so a slice can follow it.
    assert_eq!(compat(r#"print join ",", (map($_ + 1, 1, 2, 3))[0, 2]"#), "2,4");
    // File::Basename's shape: `map("\Q$_\E", @_)`.
    assert_eq!(compat(r#"print join " ", map("\Q$_\E", "a.b")"#), r"a\.b");
}
