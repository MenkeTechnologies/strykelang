//! Pin `reduce { ... } LIST` semantics: the canonical Perl-style
//! accumulator using `$a`/`$b`, behavior on empty/singleton input,
//! interplay with explicit seed value, and use with strings.
//! Probed against the running interpreter on 2026-05-23 before pinning.

use crate::common::*;

#[test]
fn reduce_sum_one_to_ten() {
    // Classic accumulator: 1+2+...+10 = 55.
    let code = r#"
        reduce { $a + $b } 1..10
    "#;
    assert_eq!(eval_int(code), 55);
}

#[test]
fn reduce_with_explicit_seed_matches_unseeded() {
    // Seeding with the identity element (0 for +) gives the same
    // result as no seed.
    let code = r#"
        my $seeded   = reduce { $a + $b } 0, 1..10;
        my $unseeded = reduce { $a + $b } 1..10;
        $seeded == $unseeded ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reduce_product_of_one_to_five_is_factorial() {
    let code = r#"
        reduce { $a * $b } 1..5
    "#;
    assert_eq!(eval_int(code), 120);
}

#[test]
fn reduce_empty_list_returns_undef() {
    // No elements ⇒ no defined accumulator.
    let code = r#"
        my $r = reduce { $a + $b } ();
        defined($r) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn reduce_single_element_returns_that_element() {
    // Block body never executes; result is the lone element.
    let code = r#"
        reduce { $a + $b } 42
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn reduce_can_pick_longest_string() {
    // A common idiom: reduce → max-by-key over strings.
    let code = r#"
        my $longest = reduce { length($a) >= length($b) ? $a : $b }
                      qw(short tinyword mediumword);
        $longest eq "mediumword" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reduce_min_via_ternary() {
    // Implementing `min` purely via reduce.
    let code = r#"
        my $m = reduce { $a < $b ? $a : $b } 9, 3, 7, 1, 5, 8;
        $m == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reduce_with_string_concat() {
    let code = r#"
        my $s = reduce { "$a-$b" } qw(a b c d);
        $s eq "a-b-c-d" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reduce_count_satisfying_predicate() {
    // reduce as a fold that produces a count.
    let code = r#"
        # number of even values in 1..10
        my $n = reduce { $a + ($b % 2 == 0 ? 1 : 0) } 0, 1..10;
        $n == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reduce_does_not_mutate_input_array() {
    let code = r#"
        my @xs = (1, 2, 3, 4);
        my $r = reduce { $a + $b } @xs;
        ($r == 10 && len(@xs) == 4 && $xs[0] == 1 && $xs[-1] == 4) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reduce_left_to_right_subtraction() {
    // Pins fold direction: ((((1-2)-3)-4)-5) = -13.
    let code = r#"
        reduce { $a - $b } 1, 2, 3, 4, 5
    "#;
    assert_eq!(eval_int(code), -13);
}
