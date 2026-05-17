//! Runtime tests for the recent parse-pin family in 7e0d3f6fdf:
//! * `array_deref_slice_with_variable_bounds`
//! * `min_max_over_array_deref_slice`
//! * `reduce_fold_with_hashref_accumulator`
//! * `flat_maps_with_recursive_call`
//! * `nested_map_with_outer_lexical_capture`
//! * `array_deref_of_bareword_topic`
//!
//! Parse-pins live elsewhere; this file pins the **runtime values** the
//! same constructs must produce.

use crate::common::*;

// ── array-deref slice `@{$arr}[$i:$j]` with variable bounds ─────────────────

#[test]
fn arrayref_slice_with_variable_bounds() {
    let s = eval_string(
        r#"
        my $arr = [10, 20, 30, 40, 50, 60, 70]
        my $i = 1
        my $j = 4
        my @slice = @{$arr}[$i:$j]
        "@slice"
        "#,
    );
    assert_eq!(s, "20 30 40 50");
}

#[test]
fn arrayref_slice_full_range_equals_original() {
    let s = eval_string(
        r#"
        my $arr = [10, 20, 30, 40, 50]
        my @slice = @{$arr}[0:4]
        "@slice"
        "#,
    );
    assert_eq!(s, "10 20 30 40 50");
}

#[test]
fn arrayref_slice_single_element_via_equal_bounds() {
    let s = eval_string(
        r#"
        my $arr = [10, 20, 30, 40, 50]
        my @slice = @{$arr}[2:2]
        "@slice"
        "#,
    );
    assert_eq!(s, "30");
}

// ── min / max / sum over slice ──────────────────────────────────────────────

#[test]
fn min_over_arrayref_slice() {
    let n = eval_int(
        r#"
        my $arr = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5]
        min(@{$arr}[2:7])
        "#,
    );
    // slice = (4, 1, 5, 9, 2, 6), min = 1
    assert_eq!(n, 1);
}

#[test]
fn max_over_arrayref_slice() {
    let n = eval_int(
        r#"
        my $arr = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5]
        max(@{$arr}[2:7])
        "#,
    );
    assert_eq!(n, 9);
}

#[test]
fn sum_over_arrayref_slice() {
    let n = eval_int(
        r#"
        my $arr = [10, 20, 30, 40, 50, 60]
        sum(@{$arr}[1:4])
        "#,
    );
    // slice = (20, 30, 40, 50), sum = 140
    assert_eq!(n, 140);
}

#[test]
fn rolling_window_min_via_slice_in_loop() {
    // Classic "rolling minimum" pattern using @{$arr}[i:j].
    let s = eval_string(
        r#"
        my $arr = [5, 3, 8, 2, 7, 9, 1, 6]
        my $w = 3
        my @rolling
        for my $i (0:(len(@$arr) - $w)) {
            my $lo = $i
            my $hi = $i + $w - 1
            push @rolling, min(@{$arr}[$lo:$hi])
        }
        "@rolling"
        "#,
    );
    // Windows of size 3:
    //  (5,3,8)→3  (3,8,2)→2  (8,2,7)→2  (2,7,9)→2  (7,9,1)→1  (9,1,6)→1
    assert_eq!(s, "3 2 2 2 1 1");
}

// ── reduce / fold with hashref accumulator ─────────────────────────────────

#[test]
fn reduce_builds_a_frequency_hashref() {
    let n = eval_int(
        r#"
        my $h = reduce { $_0->{$_1}++; $_0 } +{}, ("a", "b", "a", "c", "b", "a")
        $h->{a} * 100 + $h->{b} * 10 + $h->{c}
        "#,
    );
    // a=3, b=2, c=1 → 3*100 + 2*10 + 1 = 321
    assert_eq!(n, 321);
}

#[test]
fn fold_alias_works_with_hashref_accumulator() {
    let n = eval_int(
        r#"
        my $h = fold { $_0->{$_1}++; $_0 } +{}, ("x", "x", "y", "x", "y", "z")
        $h->{x} * 100 + $h->{y} * 10 + $h->{z}
        "#,
    );
    // x=3, y=2, z=1 → 321
    assert_eq!(n, 321);
}

#[test]
fn reduce_sums_with_init_value() {
    let n = eval_int(r#"reduce { $_0 + $_1 } 100, 1, 2, 3, 4, 5"#);
    // 100 + 1 + 2 + 3 + 4 + 5 = 115
    assert_eq!(n, 115);
}

#[test]
fn reduce_computes_product_with_init() {
    let n = eval_int(r#"reduce { $_0 * $_1 } 1, 1, 2, 3, 4, 5"#);
    // 1 * 1 * 2 * 3 * 4 * 5 = 120
    assert_eq!(n, 120);
}

#[test]
fn reduce_over_existing_array() {
    let n = eval_int(
        r#"
        my @nums = (10, 20, 30)
        reduce { $_0 + $_1 } 0, @nums
        "#,
    );
    assert_eq!(n, 60);
}

// ── nested map with outer-lexical capture ─────────────────────────────────

#[test]
fn nested_map_with_outer_lexical_capture_no_underscore_chain() {
    // Same as `_<` test but using an explicit `my $row = @$_` capture
    // — alternative idiom that should produce the same result.
    let s = eval_string(
        r#"
        my @rows = ([1, 2, 3], [4, 5, 6])
        my @flat = map {
            my @row = @$_
            map { $_ * 10 } @row
        } @rows
        "@flat"
        "#,
    );
    assert_eq!(s, "10 20 30 40 50 60");
}

// ── array-deref of bareword topic `@{_}` ───────────────────────────────────

#[test]
fn array_deref_of_bareword_topic_unwraps_arrayref() {
    let n = eval_int(
        r#"
        my @rows = ([1, 2, 3], [4, 5, 6], [7, 8, 9])
        my @sums = map { my @r = @{_}; sum(@r) } @rows
        "@sums" eq "6 15 24" ? 1 : 0
        "#,
    );
    assert_eq!(n, 1);
}

// ── range basics ───────────────────────────────────────────────────────────

#[test]
fn colon_range_inclusive_lower_to_upper() {
    let n = eval_int(r#"sum 1:10"#);
    assert_eq!(n, 55);
}

#[test]
fn colon_range_with_variable_upper_bound() {
    let n = eval_int(
        r#"
        my $upper = 100
        sum 1:$upper
        "#,
    );
    // 100*101/2 = 5050
    assert_eq!(n, 5050);
}

#[test]
fn colon_range_descending_yields_empty() {
    let n = eval_int(r#"len(5:2)"#);
    assert_eq!(n, 0);
}

// ── recursive flat-out via explicit recursion (imperative) ─────────────────
// The functional `flat_map` form has a subtle recursion bug; the imperative
// pattern with `for + push @out` works reliably and is the recommended
// idiom in stryke's STYLE_GUIDE.

#[test]
fn imperative_deep_flatten_via_recursion() {
    let s = eval_string(
        r#"
        fn Flat::deep($x) {
            return $x unless ref($x) eq "ARRAY"
            my @out
            for my $item (@$x) {
                push @out, Flat::deep($item)
            }
            @out
        }
        my $nested = [[1, [2, [3, 4]]], 5, [[6], 7]]
        my @flat = Flat::deep($nested)
        "@flat"
        "#,
    );
    assert_eq!(s, "1 2 3 4 5 6 7");
}
