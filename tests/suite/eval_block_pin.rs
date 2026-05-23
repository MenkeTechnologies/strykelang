//! Pin `eval { BLOCK }` semantics: trap of die-with-string,
//! trap of die-with-ref, $@ clearing on success, nested eval
//! independence, return-value propagation on success. Probed
//! against the running interpreter on 2026-05-23 before pinning.
//!
//! `tests/suite/eval_errors.rs` covers `eval STRING`. This file
//! covers `eval BLOCK`, which is the strictly compiled form that
//! also catches runtime exceptions raised by `die`.

use crate::common::*;

#[test]
fn eval_block_returns_block_value_on_success() {
    let code = r#"
        my $r = eval { 6 * 7 };
        ($r == 42 && $@ eq "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_block_traps_die_string_into_dollar_at() {
    let code = r#"
        eval { die "boom\n" };
        $@ eq "boom\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_block_clears_dollar_at_on_subsequent_success() {
    let code = r#"
        eval { die "first\n" };
        my $first = $@;
        eval { 1 + 1 };
        ($first eq "first\n" && $@ eq "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_block_traps_die_with_hashref_payload() {
    // die can take any ref, and $@ becomes that ref.
    let code = r#"
        eval { die { code => 42, msg => "fail" } };
        (ref($@) eq "HASH"
         && $@->{code} == 42
         && $@->{msg} eq "fail") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_block_traps_die_with_arrayref_payload() {
    let code = r#"
        eval { die [1, 2, 3] };
        (ref($@) eq "ARRAY"
         && len(@{$@}) == 3
         && $@->[2] == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_block_nested_does_not_clobber_outer_at() {
    // Inner eval catches its own die; outer eval's $@ stays as the
    // outer die's payload.
    let code = r#"
        eval {
            eval { die "inner\n" };
            die "outer\n" if $@
        };
        $@ eq "outer\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_block_with_division_by_zero_caught() {
    let code = r#"
        my $r = eval { 1 / 0 };
        ($@ ne "" && !defined($r)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_block_can_be_rethrown_via_die() {
    // Common idiom: catch, inspect, rethrow.
    let code = r#"
        my $final = "";
        eval {
            eval { die "x\n" };
            if ($@) {
                die "wrapped: $@"
            }
        };
        $@ =~ /^wrapped: x/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_block_does_not_swallow_normal_return_value() {
    let code = r#"
        my @r = eval { (1, 2, 3) };
        len(@r) == 3 && $r[0] == 1 && $r[2] == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn eval_block_local_dollar_at_does_not_leak_to_caller() {
    // After a successful eval BLOCK, $@ must be empty even if a
    // prior eval set it.
    let code = r#"
        $@ = "stale";
        eval { 1 };
        $@ eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
