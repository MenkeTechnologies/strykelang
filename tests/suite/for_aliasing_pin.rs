//! Pin `for`-loop aliasing semantics: `for (@a)` aliases `$_` to
//! each element so in-place mutation modifies the array, `for my
//! $x (@a)` aliases `$x` likewise, but iterating a synthetic list
//! expression (e.g. `(@a, @b)`) breaks the alias because the list
//! constructor copies values. Probed against the running
//! interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn for_topic_mutation_writes_through_to_array() {
    let code = r#"
        my @a = (1, 2, 3);
        for (@a) { $_ *= 10 }
        join(",", @a) eq "10,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_named_lexical_mutation_writes_through_to_array() {
    let code = r#"
        my @a = (1, 2, 3);
        for my $x (@a) { $x *= 10 }
        join(",", @a) eq "10,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_concatenated_list_does_not_mutate_originals() {
    // Iterating `(@a, @b)` synthesizes a new flat list; mutating
    // the loop variable doesn't propagate back to @a or @b.
    let code = r#"
        my @a = (1, 2, 3);
        my @b = (10, 20);
        for my $x (@a, @b) { $x++ }
        (join(",", @a) eq "1,2,3" && join(",", @b) eq "10,20") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_over_literal_range_iterates_each_value() {
    let code = r#"
        my @seen;
        for (1..3) { push @seen, $_ }
        join(",", @seen) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_topic_restored_after_loop() {
    // `for (@a) { ... }` locally aliases $_ inside the loop;
    // outside, $_ regains its prior value.
    let code = r#"
        $_ = "outer";
        my @a = (10, 20);
        for (@a) { }
        $_ eq "outer" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_named_lexical_is_block_scoped() {
    // `$x` from `for my $x (...)` does not leak.
    let code = r#"
        my @a = (10, 20);
        for my $x (@a) { }
        defined($x) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn for_empty_list_runs_zero_iterations() {
    let code = r#"
        my $count = 0;
        for my $x (()) { $count++ }
        $count
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn for_iterates_in_order() {
    let code = r#"
        my @seen;
        my @input = (5, 1, 4, 2, 3);
        for my $x (@input) { push @seen, $x }
        join(",", @seen) eq "5,1,4,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_over_array_visits_every_element_exactly_once() {
    let code = r#"
        my @a = (10, 20, 30, 40);
        my $count = 0;
        for my $x (@a) { $count++ }
        $count == len(@a) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_aliasing_to_topic_does_not_alias_to_copy() {
    // Inside the body, `$_` is the live alias; a captured copy is
    // independent and won't update when the next iteration writes
    // through `$_`.
    let code = r#"
        my @a = (1, 2, 3);
        my @copies;
        for (@a) {
            my $copy = $_;
            push @copies, $copy;
            $_ *= 100;
        }
        (join(",", @a) eq "100,200,300" && join(",", @copies) eq "1,2,3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn for_named_lexical_independent_of_topic() {
    // `for my $x (@a)` does NOT clobber `$_`.
    let code = r#"
        $_ = "outer";
        my @a = (1, 2, 3);
        for my $x (@a) { }
        $_ eq "outer" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
