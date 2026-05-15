//! Core list-reduction builtin pins: min/max/sum/product/avg/all/any/none.

use crate::common::*;

// ── sum ──────────────────────────────────────────────────────────────

#[test]
fn sum_of_simple_list() {
    let code = r#"
        sum(1, 2, 3, 4, 5) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sum_of_empty_returns_zero() {
    let code = r#"
        my @empty;
        sum(@empty) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sum_of_negative_numbers() {
    let code = r#"
        sum(-3, -2, -1, 0, 1, 2, 3) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sum_of_floats_with_tolerance() {
    let code = r#"
        my $r = sum(0.1, 0.1, 0.1);
        abs($r - 0.3) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── min / max ────────────────────────────────────────────────────────

#[test]
fn min_of_numbers() {
    let code = r#"
        min(3, 1, 4, 1, 5, 9, 2, 6) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn max_of_numbers() {
    let code = r#"
        max(3, 1, 4, 1, 5, 9, 2, 6) == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn min_of_single_element() {
    let code = r#"
        min(42) == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn max_of_single_element() {
    let code = r#"
        max(99) == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn min_with_negatives_works() {
    let code = r#"
        min(-1, -5, 0, 3) == -5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn max_with_negatives_works() {
    let code = r#"
        max(-1, -5, 0, 3) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── product ──────────────────────────────────────────────────────────

#[test]
fn product_of_list() {
    let code = r#"
        product(1, 2, 3, 4, 5) == 120 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn product_with_zero_is_zero() {
    let code = r#"
        product(1, 2, 0, 3, 4) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── avg / mean ───────────────────────────────────────────────────────

#[test]
fn avg_of_list() {
    let code = r#"
        my $r = avg(2, 4, 6, 8, 10);
        $r == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn avg_of_negatives_and_positives() {
    let code = r#"
        my $r = avg(-5, 0, 5);
        $r == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── all / any / none ─────────────────────────────────────────────────

#[test]
fn all_true_when_every_element_truthy() {
    let code = r#"
        all { _ > 0 } (1, 2, 3, 4, 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn all_false_when_one_element_falsy() {
    // Note: parentheses required around `all { } LIST` because the
    // ternary `? :` binds tighter than the LIST is grouped to the
    // builtin (parser precedence quirk).
    let code = r#"
        (all { _ > 0 } (1, 2, -1, 4, 5)) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn any_true_when_at_least_one_truthy() {
    let code = r#"
        (any { _ > 100 } (1, 2, 999, 4, 5)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn any_false_when_none_truthy() {
    let code = r#"
        (any { _ > 100 } (1, 2, 3, 4, 5)) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn none_true_when_no_element_matches() {
    let code = r#"
        none { _ > 100 } (1, 2, 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── count ────────────────────────────────────────────────────────────

#[test]
fn count_via_scalar_grep_idiom() {
    // BUG-232: `count { BLOCK } LIST` does not return the predicate-
    // match count as expected — it returns the value of the first
    // matched element. The Perl idiom `scalar(grep { } LIST)` is
    // the working replacement.
    let code = r#"
        my $n = scalar(grep { _ % 2 == 0 } (1, 2, 3, 4, 5, 6, 7, 8, 9, 10));
        $n == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── first ────────────────────────────────────────────────────────────

#[test]
fn first_matching_predicate_returned() {
    let code = r#"
        my $r = first { _ > 5 } (1, 3, 5, 7, 9);
        $r == 7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn first_with_no_match_returns_undef() {
    let code = r#"
        my $r = first { _ > 100 } (1, 2, 3);
        defined($r) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── head / tail ─────────────────────────────────────────────────────

#[test]
fn head_returns_first_element_via_slice() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        $a[0] == 10 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn last_element_via_negative_index() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        $a[-1] == 50 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tail_returns_rest_via_slice() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        my @rest = @a[1:4];
        join(",", @rest) eq "20,30,40,50" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── len ──────────────────────────────────────────────────────────────

#[test]
fn len_of_array() {
    let code = r#"
        len(1, 2, 3, 4, 5) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn len_of_string() {
    let code = r#"
        len("hello") == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn len_of_hash_keys() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        len(keys %h) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Composition: map + reduce ──────────────────────────────────────

#[test]
fn sum_of_squares_via_map_reduce() {
    let code = r#"
        my $r = sum(map { _ * _ } (1, 2, 3, 4, 5));
        $r == 55 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn max_after_grep_filter() {
    let code = r#"
        my $r = max(grep { _ % 2 == 1 } (1:10));
        $r == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── all/any over hashref values ─────────────────────────────────────

#[test]
fn all_over_hash_values() {
    let code = r#"
        my %h = (a => 10, b => 20, c => 30);
        all { _ > 0 } values %h ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── min/max via reduce equivalence ─────────────────────────────────

#[test]
fn reduce_max_matches_max() {
    let code = r#"
        my @input = (5, 2, 8, 1, 9, 3, 7);
        my $r1 = reduce { _0 > _1 ? _0 : _1 } 0, @input;
        my $r2 = max(@input);
        $r1 == $r2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
