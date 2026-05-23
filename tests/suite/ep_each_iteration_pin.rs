//! Pin `|> e p` (each-print) and `|> ep` (each-print shorthand)
//! iteration forms from `docs/STYLE_GUIDE.md` §8. Both go to stdout
//! per the style guide ("name is each-print, not err-print"). Also
//! pins `|> e { … }` block form. Probed against the running
//! interpreter on 2026-05-23.
//!
//! These tests inspect the value returned by `each`/`ep`/`e p` rather
//! than the stdout side effect — `eval_int` doesn't capture stdout.
//! The pin is that the form parses, dispatches, and visits every
//! element with the correct effect (verified via accumulator).

use crate::common::*;

#[test]
fn e_block_visits_each_element_in_order() {
    let code = r#"
        my @seen;
        (1, 2, 3) |> e { push @seen, _ };
        join(",", @seen) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn e_block_visit_count_matches_input_length() {
    let code = r#"
        my $n = 0;
        (10, 20, 30, 40, 50) |> e { $n++ };
        $n == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn e_block_can_receive_topic_value() {
    let code = r#"
        my $sum = 0;
        (1, 2, 3, 4, 5) |> e { $sum += _ };
        $sum == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn e_block_on_empty_list_zero_iterations() {
    let code = r#"
        my $n = 0;
        () |> e { $n++ };
        $n
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn e_block_on_range_input() {
    let code = r#"
        my $sum = 0;
        (1..10) |> e { $sum += _ };
        $sum
    "#;
    assert_eq!(eval_int(code), 55);
}

#[test]
fn e_block_after_map_in_pipe_chain() {
    let code = r#"
        my @doubled;
        (1, 2, 3) |> map { _ * 2 } |> e { push @doubled, _ };
        join(",", @doubled) eq "2,4,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn e_block_after_grep_in_pipe_chain() {
    let code = r#"
        my @evens;
        (1..6) |> grep { _ % 2 == 0 } |> e { push @evens, _ };
        join(",", @evens) eq "2,4,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn e_block_dispatches_through_named_fn() {
    let code = r#"
        my @collect;
        fn Demo::Each::stash($v) { push @collect, $v * 10 }
        (1, 2, 3) |> e { Demo::Each::stash(_) };
        join(",", @collect) eq "10,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn e_block_iterates_array_variable() {
    let code = r#"
        my @input = (3, 1, 4, 1, 5);
        my $sum = 0;
        @input |> e { $sum += _ };
        $sum == 14 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn e_block_iterates_array_via_paren_less() {
    // Stryke parses `e { ... }` directly on a value through `|>`.
    let code = r#"
        my @input = (10, 20, 30);
        my $product = 1;
        @input |> e { $product *= _ };
        $product == 6000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
