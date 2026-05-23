//! Pin statement-modifier forms: `EXPR if COND`, `EXPR unless COND`,
//! `EXPR while COND`, `EXPR until COND`, `EXPR for LIST`. Each is
//! a trailing modifier that suffixes a single expression statement.
//! Probed against the running interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn if_modifier_runs_when_true() {
    let code = r#"
        my $x = 0;
        $x = 7 if 1;
        $x
    "#;
    assert_eq!(eval_int(code), 7);
}

#[test]
fn if_modifier_skips_when_false() {
    let code = r#"
        my $x = 99;
        $x = 7 if 0;
        $x
    "#;
    assert_eq!(eval_int(code), 99);
}

#[test]
fn unless_modifier_runs_when_false() {
    let code = r#"
        my $x = 0;
        $x = 7 unless 0;
        $x
    "#;
    assert_eq!(eval_int(code), 7);
}

#[test]
fn unless_modifier_skips_when_true() {
    let code = r#"
        my $x = 99;
        $x = 7 unless 1;
        $x
    "#;
    assert_eq!(eval_int(code), 99);
}

#[test]
fn while_modifier_loops_until_condition_false() {
    let code = r#"
        my $i = 0;
        $i++ while $i < 10;
        $i
    "#;
    assert_eq!(eval_int(code), 10);
}

#[test]
fn until_modifier_loops_until_condition_true() {
    let code = r#"
        my $i = 0;
        $i++ until $i == 7;
        $i
    "#;
    assert_eq!(eval_int(code), 7);
}

#[test]
fn for_modifier_pushes_each_element() {
    let code = r#"
        my @collect;
        push @collect, $_ for (1, 2, 3, 4, 5);
        join(",", @collect) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_modifier_iterates_a_range() {
    let code = r#"
        my $sum = 0;
        $sum += $_ for 1..100;
        $sum
    "#;
    assert_eq!(eval_int(code), 5050);
}

#[test]
fn for_modifier_iterates_an_array() {
    let code = r#"
        my @a = (10, 20, 30);
        my $sum = 0;
        $sum += $_ for @a;
        $sum
    "#;
    assert_eq!(eval_int(code), 60);
}

#[test]
fn if_modifier_with_truthy_string() {
    // Any non-empty, non-"0" string is truthy.
    let code = r#"
        my $x = 0;
        $x = 1 if "hello";
        $x
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn unless_modifier_with_zero_string_runs() {
    // "0" is the canonical false string.
    let code = r#"
        my $x = 0;
        $x = 42 unless "0";
        $x
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn while_modifier_zero_iterations_when_condition_false() {
    let code = r#"
        my $i = 100;
        $i-- while $i > 200;
        $i
    "#;
    assert_eq!(eval_int(code), 100);
}

#[test]
fn for_modifier_inside_compound_statement() {
    // Common idiom: printf inside a for-modifier prints once per
    // element.
    let code = r#"
        my @out;
        push @out, "n=$_" for 1..3;
        join("|", @out) eq "n=1|n=2|n=3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
