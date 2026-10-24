//! `--compat` regressions for bare `&name` calls, empty-argument calls, and
//! `push`/`pop`/`shift`/`unshift` autovivification through arrow subscripts
//! (including blessed objects). Every expected string is stock `perl` output
//! for the same program.
//!
//! `--compat` is process-global, so each case runs the real `st` binary in a
//! subprocess.

use std::process::Command;

fn run(args: &[&str], code: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_st"))
        .args(args)
        .args(["-e", code])
        .env("STRYKE_CACHE", "0")
        .output()
        .expect("spawn st");
    assert!(
        out.status.success(),
        "st {args:?} -e {code:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn compat(code: &str) -> String {
    run(&["--compat"], code)
}

// ── bare `&name` passes the caller's `@_` ────────────────────────────────────

#[test]
fn bare_amp_call_sees_the_callers_args() {
    let f2 = r#"sub f2 { print scalar(@_), ":@_" } "#;
    assert_eq!(compat(&format!("{f2} sub f {{ &f2 }} f(1, 2, 3)")), "3:1 2 3");
    assert_eq!(compat(&format!("{f2} sub f {{ &f2; }} f(1, 2, 3)")), "3:1 2 3");
    // `&f2()` with explicit empty parens passes nothing.
    assert_eq!(compat(&format!("{f2} sub f {{ &f2() }} f(1, 2, 3)")), "0:");
}

#[test]
fn bare_amp_call_keeps_context_and_binds_in_expressions() {
    assert_eq!(
        compat(
            r#"sub f2 { wantarray ? "L" : "S" } sub f { my @a = &f2; my $s = &f2; print "@a $s" } f()"#
        ),
        "L S"
    );
    assert_eq!(compat(r#"sub f2 { $_[0] * 10 } sub f { print &f2 + 1 } f(4)"#), "41");
}

#[test]
fn bare_amp_call_to_package_qualified_sub() {
    assert_eq!(
        compat(r#"package P; sub g { print "g:@_" } package main; sub f { &P::g } f(7, 8)"#),
        "g:7 8"
    );
}

#[test]
fn amp_forms_that_do_not_call_are_unchanged() {
    assert_eq!(
        compat(
            r#"sub x {1} print defined &x ? "d" : "u", exists &x ? "e" : "n", defined &nope ? "d" : "u""#
        ),
        "deu"
    );
    assert_eq!(compat(r#"sub f2 { print "f2:@_" } sub f { goto &f2 } f(5, 6)"#), "f2:5 6");
}

// ── empty-argument calls ─────────────────────────────────────────────────────

#[test]
fn empty_call_gets_empty_args_under_compat() {
    assert_eq!(compat("sub f { print scalar(@_) } f()"), "0");
    assert_eq!(compat("for (1, 2) { sub g { print scalar(@_) } g() }"), "00");
}

#[test]
fn empty_call_still_receives_topic_in_default_mode() {
    assert_eq!(
        run(&[], r#"fn dbl { $_[0] * 2 } print join ",", map { dbl() } 1:3"#),
        "2,4,6"
    );
    assert_eq!(
        run(&[], r#"fn dbl { $_[0] * 2 } print join ",", map { dbl } 1:3"#),
        "2,4,6"
    );
}

// ── autoviv through arrow subscripts ─────────────────────────────────────────

#[test]
fn push_and_unshift_vivify_arrow_hash_and_array_slots() {
    assert_eq!(
        compat(
            r#"my $h = {}; push @{ $h->{k} }, 1, 2; unshift @{ $h->{u} }, 9; print "@{$h->{k}} @{$h->{u}}""#
        ),
        "1 2 9"
    );
    assert_eq!(
        compat(r#"my $r = []; push @{ $r->[0] }, 1; print scalar(@{ $r->[0] })"#),
        "1"
    );
}

#[test]
fn pop_and_shift_vivify_an_empty_array_in_the_slot() {
    assert_eq!(
        compat(
            r#"my $h = {}; my $p = pop @{ $h->{p} }; my $s = shift @{ $h->{s} }; print defined $p ? "d" : "u", defined $s ? "d" : "u", ref $h->{p}, ref $h->{s}"#
        ),
        "uuARRAYARRAY"
    );
}

#[test]
fn push_vivifies_every_level_of_a_deep_chain() {
    assert_eq!(
        compat(
            r#"my $h = {}; push @{ $h->{a}{b} }, 3; push @{ $h->{x}[2] }, 4; print "$h->{a}{b}[0] $h->{x}[2][0] ", scalar(@{ $h->{x} })"#
        ),
        "3 4 3"
    );
}

#[test]
fn blessed_objects_vivify_like_plain_refs() {
    assert_eq!(
        compat(
            r#"my $o = bless {}, "C"; push @{ $o->{list} }, 1, 2; $o->{a}{b} = 1; print "@{$o->{list}} $o->{a}{b} ", ref $o"#
        ),
        "1 2 1 C"
    );
    assert_eq!(
        compat(r#"my $o = bless [], "C"; push @{ $o->[1] }, 5; print $o->[1][0], ref $o"#),
        "5C"
    );
}
