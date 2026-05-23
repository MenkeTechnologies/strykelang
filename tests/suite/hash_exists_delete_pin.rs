//! Pin the `exists` vs `defined` distinction and `delete`'s return
//! semantics on hashes. These are commonly confused — explicit
//! `undef` and missing keys must remain distinguishable. Probed
//! against the running interpreter on 2026-05-23 before pinning.

use crate::common::*;

#[test]
fn exists_on_populated_key_is_true() {
    let code = r#"
        my %h = (a => 1, b => 2);
        exists($h{a}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn exists_on_missing_key_is_false() {
    let code = r#"
        my %h = (a => 1);
        exists($h{nope}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn exists_distinguishes_explicit_undef_from_missing() {
    // An explicit `undef` value still makes the key exist.
    let code = r#"
        my %h;
        $h{a} = undef;
        (exists($h{a}) && !defined($h{a}) && !exists($h{b})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn exists_on_zero_valued_key_is_true() {
    // `0` is defined; exists must be true.
    let code = r#"
        my %h = (a => 0);
        (exists($h{a}) && defined($h{a}) && $h{a} == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn delete_returns_the_removed_value() {
    let code = r#"
        my %h = (a => 42, b => 7);
        my $v = delete $h{a};
        ($v == 42 && !exists($h{a}) && exists($h{b})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn delete_on_missing_key_returns_undef() {
    let code = r#"
        my %h = (a => 1);
        my $v = delete $h{nope};
        defined($v) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn delete_decreases_keys_count() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        delete $h{b};
        len(keys(%h)) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn delete_then_reinsert_makes_key_exist_again() {
    let code = r#"
        my %h = (a => 1);
        delete $h{a};
        my $gone = exists($h{a}) ? 1 : 0;
        $h{a} = 99;
        my $back = exists($h{a}) ? 1 : 0;
        ($gone == 0 && $back == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn exists_works_through_hashref_arrow_deref() {
    let code = r#"
        my $h = { x => 1 };
        (exists($h->{x}) && !exists($h->{y})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn delete_through_hashref_arrow_deref() {
    let code = r#"
        my $h = { x => 10, y => 20 };
        my $v = delete $h->{x};
        ($v == 10 && len(keys(%$h)) == 1 && exists($h->{y})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn empty_hash_has_no_keys() {
    let code = r#"
        my %h;
        (len(keys(%h)) == 0 && !exists($h{anything})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
