//! Pin defined-or `//` and defined-or-assign `//=` semantics.
//!
//! Unlike `||`, `//` only short-circuits on **undef** — `0`, `""`,
//! `"0"` are all "defined" and must pass through. Probed against the
//! running interpreter on 2026-05-23 before pinning.

use crate::common::*;

#[test]
fn defined_or_undef_falls_through_to_rhs() {
    let code = r#"
        my $x;
        my $y = $x // 42;
        $y == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_zero_is_kept_not_substituted() {
    // `0` is defined; `//` must NOT replace it.
    let code = r#"
        my $x = 0;
        my $y = $x // 99;
        $y == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_empty_string_is_kept() {
    let code = r#"
        my $x = "";
        my $y = $x // "FALLBACK";
        $y eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_string_zero_is_kept() {
    let code = r#"
        my $x = "0";
        my $y = $x // "FALLBACK";
        $y eq "0" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_chains_left_to_right() {
    let code = r#"
        my ($a, $b, $c);
        $b = "hit";
        my $r = $a // $b // $c // "nope";
        $r eq "hit" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_short_circuits_on_first_defined() {
    // RHS must not be evaluated when LHS is defined — pin via a
    // side-effect counter held behind a hashref so the function
    // (which closes by value) can still mutate the storage.
    let code = r#"
        my $counter = { hits => 0 };
        fn Probe::bump($c) { $c->{hits}++; 99 }
        my $x = 7;
        my $r = $x // Probe::bump($counter);
        ($r == 7 && $counter->{hits} == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_evaluates_rhs_when_lhs_undef() {
    let code = r#"
        my $counter = { hits => 0 };
        fn Probe::bump($c) { $c->{hits}++; 99 }
        my $x;
        my $r = $x // Probe::bump($counter);
        ($r == 99 && $counter->{hits} == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_assign_only_fires_when_undef() {
    let code = r#"
        my $x;
        $x //= 7;
        my $y = 0;
        $y //= 99;     # already defined, no change
        ($x == 7 && $y == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_assign_initializes_hash_slot() {
    // Classic memoization pattern: `$cache{$k} //= compute($k)`.
    let code = r#"
        my %cache;
        $cache{foo} //= "first";
        $cache{foo} //= "second";   # no-op
        $cache{foo} eq "first" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_vs_logical_or_diverge_on_zero() {
    // Pin the canonical difference: `||` returns the RHS for 0,
    // but `//` returns the original 0.
    let code = r#"
        my $x = 0;
        my $by_or  = $x || 99;
        my $by_dor = $x // 99;
        ($by_or == 99 && $by_dor == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
