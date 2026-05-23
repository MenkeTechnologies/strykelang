//! Pin `do { BLOCK }` semantics: block-as-expression returning the
//! last evaluated value, `do { } while (COND)` post-test loop,
//! `do { } until (COND)` post-test loop, list-context return.
//! Probed against the running interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn do_block_returns_last_expression() {
    let code = r#"
        my $r = do { my $x = 5; $x * 2 };
        $r
    "#;
    assert_eq!(eval_int(code), 10);
}

#[test]
fn do_block_returns_last_of_sequence_of_statements() {
    let code = r#"
        my $r = do { 1; 2; 3 };
        $r
    "#;
    assert_eq!(eval_int(code), 3);
}

#[test]
fn do_block_containing_if_returns_branch_value() {
    let code = r#"
        my $cond = 1;
        my $r = do { if ($cond) { 10 } else { 20 } };
        $r
    "#;
    assert_eq!(eval_int(code), 10);
}

#[test]
fn do_block_else_branch_is_returned_when_taken() {
    let code = r#"
        my $cond = 0;
        my $r = do { if ($cond) { 10 } else { 20 } };
        $r
    "#;
    assert_eq!(eval_int(code), 20);
}

#[test]
fn do_block_returns_list_in_list_context() {
    let code = r#"
        my @r = do { (1, 2, 3) };
        (len(@r) == 3 && $r[0] == 1 && $r[2] == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn do_while_runs_body_at_least_once() {
    // Body executes before the condition is checked, even when the
    // condition is false from the start.
    let code = r#"
        my $i = 100;
        do { $i++ } while ($i < 50);
        $i
    "#;
    assert_eq!(eval_int(code), 101);
}

#[test]
fn do_while_loops_until_condition_false() {
    let code = r#"
        my $i = 0;
        do { $i++ } while ($i < 5);
        $i
    "#;
    assert_eq!(eval_int(code), 5);
}

#[test]
fn do_until_loops_until_condition_true() {
    let code = r#"
        my $i = 0;
        do { $i++ } until ($i == 7);
        $i
    "#;
    assert_eq!(eval_int(code), 7);
}

#[test]
fn do_until_runs_body_at_least_once_when_cond_true_immediately() {
    let code = r#"
        my $i = 5;
        do { $i++ } until ($i >= 0);   # condition true from start
        $i
    "#;
    assert_eq!(eval_int(code), 6);
}

#[test]
fn do_block_can_compute_sum_of_range() {
    let code = r#"
        my $sum = do {
            my $s = 0;
            for my $i (1..10) { $s += $i }
            $s
        };
        $sum
    "#;
    assert_eq!(eval_int(code), 55);
}

#[test]
fn do_block_nested_returns_inner_block_value() {
    let code = r#"
        my $r = do { do { do { 42 } } };
        $r
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn do_block_local_vars_dont_leak_out() {
    let code = r#"
        my $x = 1;
        do { my $x = 99; $x };   # inner $x is shadowed
        $x
    "#;
    assert_eq!(eval_int(code), 1);
}
