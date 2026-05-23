//! Pin `flat_map { BLOCK } LIST` semantics per
//! `docs/STYLE_GUIDE.md` §8: applies the block to each element and
//! concatenates the (possibly multi-element) results into one flat
//! list. Empty-returning blocks act as filters; arrayref-deref'ing
//! blocks act as list-of-lists flatteners. Probed against the
//! running interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn flat_map_doubles_each_element_into_two() {
    let code = r#"
        my @r = flat_map { ($_, $_ * 100) } (1, 2, 3);
        join(",", @r) eq "1,100,2,200,3,300" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn flat_map_empty_input_yields_empty() {
    let code = r#"
        my @r = flat_map { ($_) } ();
        len(@r)
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn flat_map_empty_returns_act_as_filter() {
    // Returning `()` drops the element entirely.
    let code = r#"
        my @r = flat_map { () } (1, 2, 3);
        len(@r)
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn flat_map_filter_via_conditional_empty() {
    // Common idiom: predicate-and-yield in one pass.
    let code = r#"
        my @r = flat_map { _ % 2 == 0 ? (_) : () } (1, 2, 3, 4, 5, 6);
        join(",", @r) eq "2,4,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn flat_map_deref_arrayref_concatenates() {
    let code = r#"
        my @nested = ([1, 2], [3, 4], [5, 6]);
        my @flat = flat_map { @$_ } @nested;
        join(",", @flat) eq "1,2,3,4,5,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn flat_map_mixed_scalar_and_arrayref_via_branch() {
    let code = r#"
        my @input = ([1, 2], 3, [4, 5, 6], 7);
        my @flat = flat_map { ref(_) eq "ARRAY" ? @$_ : (_) } @input;
        join(",", @flat) eq "1,2,3,4,5,6,7" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn flat_map_via_pipe_forward_chain() {
    // Per style guide §6b: read pipeline left-to-right.
    let code = r#"
        my @r = (1, 2, 3) |> flat_map { (_, _ * _) };
        join(",", @r) eq "1,1,2,4,3,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn flat_map_preserves_in_order_concatenation() {
    let code = r#"
        my @r = flat_map { ("(", _, ")") } (1, 2, 3);
        join("", @r) eq "(1)(2)(3)" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn flat_map_with_three_element_expansion() {
    let code = r#"
        my @r = flat_map { ($_, $_, $_) } (1, 2);
        join(",", @r) eq "1,1,1,2,2,2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn flat_map_chains_with_grep_after() {
    let code = r#"
        my @r = flat_map { (_, $_ * 2) } (1, 2, 3, 4);
        my @big = grep { _ > 4 } @r;
        join(",", @big) eq "6,8" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn flat_map_with_singleton_returns_passes_through() {
    // Block returns a single value per element — equivalent to map.
    let code = r#"
        my @a = (10, 20, 30);
        my @r = flat_map { _ + 1 } @a;
        join(",", @r) eq "11,21,31" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
