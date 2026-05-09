//! Behavior-pinning batch BQ (2026-05-08): ~22 tests — list reverse/sort/scan/split, tail ops,
//! midpoint & slope & hypot, logs & parse, matrix shape, merge_hash, sumsq, running extrema & diff,
//! index/elem search, case & round, joins, bit_not, aggregates, clamp(min,max,val), divmod, cons.

use crate::common::*;

#[test]
fn list_reverse_bq() {
    assert_eq!(
        eval_string(r#"join(",", reverse(1, 2, 3))"#),
        "3,2,1"
    );
}

#[test]
fn perl_numeric_sort_bq() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } (20, 4, 80))"#),
        "4,20,80"
    );
}

#[test]
fn scan_and_split_bq() {
    assert_eq!(
        eval_string(r#"join(",", scan_left({ $_[0] + $_[1] }, 0, 10, 100, 1000))"#),
        "0,0,10,110,1110"
    );
    assert_eq!(
        eval_string(r#"stringify(split_at(3, 1, 2, 3, 4, 5))"#),
        "([1, 2, 3], [4, 5])"
    );
}

#[test]
fn take_drop_last_bq() {
    assert_eq!(
        eval_string(r#"join(",", take_last(2, 1, 2, 3, 4, 5))"#),
        "4,5"
    );
    assert_eq!(
        eval_string(r#"join(",", drop_last(2, 1, 2, 3, 4, 5))"#),
        "1,2,3"
    );
}

#[test]
fn planar_midpoint_slope_hypot_bq() {
    assert_eq!(
        eval_string(r#"stringify(midpoint(0, 0, 40, 60))"#),
        "(20, 30)"
    );
    assert_eq!(eval_int("midpoint_of(10, 30)"), 20);
    assert_eq!(eval_int("slope(0, 0, 4, 8)"), 2);
    assert_eq!(eval_int("triangle_hypotenuse(3, 4)"), 5);
}

#[test]
fn logs_parse_abs_bq() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", log10(1000))"#),
        "3.0000000000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.6f", log1p_exp(2))"#),
        "2.126928"
    );
    assert_eq!(eval_int("abs_diff(40, 7)"), 33);
    assert_eq!(eval_int("parse_float(\"-3e2\")"), -300);
    assert_eq!(eval_int("int(\"099\")"), 99);
    assert_eq!(eval_int("hex(\"ff\")"), 255);
}

#[test]
fn matrix_shape_bq() {
    assert_eq!(
        eval_string(r#"stringify(matrix_shape([[1, 2], [3, 4]]))"#),
        "(2, 2)"
    );
}

#[test]
fn merge_hash_shallow_bq() {
    assert_eq!(
        eval_string(r#"stringify(merge_hash({ a => 1 }, { b => 3 }, { a => 9 }))"#),
        "+{a => 9, b => 3}"
    );
}

#[test]
fn sumsq_bq() {
    assert_eq!(eval_int("sumsq(3, 4)"), 25);
}

#[test]
fn running_min_max_diff_bq() {
    assert_eq!(
        eval_string(r#"join(",", running_max(1, 5, 3, 9, 2))"#),
        "1,5,5,9,9"
    );
    assert_eq!(
        eval_string(r#"join(",", running_min(10, 4, 6, 2))"#),
        "10,4,4,2"
    );
    assert_eq!(
        eval_string(r#"join(",", diff(1, 4, 7, 7))"#),
        "3,3,0"
    );
}

#[test]
fn index_rindex_elem_bq() {
    assert_eq!(eval_int(r#"index("banana", "na")"#), 2);
    assert_eq!(eval_int(r#"rindex("banana", "na")"#), 4);
    assert_eq!(eval_int(r#"elem_index("b", qw(a b c))"#), 1);
    assert_eq!(eval_int(r#"rindex("abcab", "ab")"#), 3);
}

#[test]
fn case_twiddle_bq() {
    assert_eq!(eval_string(r#"lcfirst("HELLO")"#), "hELLO");
    assert_eq!(eval_string(r#"ucfirst("hello")"#), "Hello");
}

#[test]
fn round_places_bq() {
    assert_eq!(
        eval_string(r#"sprintf("%.2f", round(1.2345, 2))"#),
        "1.23"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.3f", round(1.2345, 3))"#),
        "1.235"
    );
}

#[test]
fn join_strings_bq() {
    assert_eq!(
        eval_string(r#"join("", "hel", "lo")"#),
        "hello"
    );
    assert_eq!(
        eval_string(r#"join("|", split(",", "a,b,c,d", 3))"#),
        "a|b|c,d"
    );
}

#[test]
fn bit_not_sample_bq() {
    assert_eq!(eval_int("bit_not(0)"), -1);
}

#[test]
fn frequency_min_max_list_bq() {
    assert_eq!(eval_int("min(9, 3, 7)"), 3);
    assert_eq!(eval_int("max(9, 3, 7)"), 9);
}

#[test]
fn product_small_bq() {
    assert_eq!(eval_int("product(1, 2, 3, 4)"), 24);
}

#[test]
fn mean_median_mode_edge_bq() {
    assert_eq!(eval_int("mean(10, 20, 30)"), 20);
    assert_eq!(eval_int("median(1, 2, 9)"), 2);
}

#[test]
fn clamp_scalar_bq() {
    assert_eq!(eval_int("clamp(10, 100, 5)"), 10);
    assert_eq!(eval_int("clamp(10, 100, 50)"), 50);
    assert_eq!(eval_int("clamp(10, 100, 500)"), 100);
}

#[test]
fn signum_bq() {
    assert_eq!(eval_int("sign(-9)"), -1);
    assert_eq!(eval_int("sign(0)"), 0);
    assert_eq!(eval_int("sign(9)"), 1);
}

#[test]
fn divmod_neg_bq() {
    assert_eq!(
        eval_string(r#"join(",", divmod(-10, 3))"#),
        "-3,-1"
    );
}

#[test]
fn cons_prepend_append_contains_bq() {
    assert_eq!(eval_string(r#"join(",", prepend(0, 1, 2))"#), "0,1,2");
    assert_eq!(eval_string(r#"join(",", append_elem(9, 7, 8))"#), "7,8,9");
    assert_eq!(
        eval_string(r#"join(",", cons(9, 10, 11))"#),
        "9,10,11"
    );
    assert_eq!(eval_int("contains_elem(3, 1, 2, 3, 4)"), 1);
}

#[test]
fn flatten_factorial_fib_log_exp_bq() {
    assert_eq!(
        eval_string(r#"join(",", flatten_deep(1, [2, [3, 4]]))"#),
        "1,2,3,4"
    );
    assert_eq!(eval_int("factorial(0)"), 1);
    assert_eq!(eval_int("fibonacci(0)"), 0);
    assert_eq!(
        eval_string(r#"sprintf("%.6f", exp(log(12)))"#),
        "12.000000"
    );
}

#[test]
fn map_numeric_idiom_bq() {
    assert_eq!(
        eval_string(r#"join(",", map { -$_ } (10, -5, 0))"#),
        "-10,5,0"
    );
}

#[test]
fn logical_shortcut_bq() {
    assert_eq!(eval_int("!0"), 1);
    assert_eq!(eval_int("!1"), 0);
    assert_eq!(eval_int("1 && 1"), 1);
    assert_eq!(eval_int("1 || 0"), 1);
}
