//! Behavior-pinning batch CQ (2026-05-09): word lists / path decomposition, **Bessel** / **erfi**,
//! running statistics, **`unzip` / `partition_by` / `each_cons` / `window_n`**, matrix **Frobenius** norm,
//! **`uri_resolve` / `uri_normalize`** (**BUG-164** — byte-vector input, not ordinary strings), and
//! **`running_reduce`** with **`$a`/`$b`** vs slot comparators (**BUG-163**).

use crate::common::*;

#[test]
fn qw_three_words_cq() {
    assert_eq!(eval_string(r#"stringify(qw(x y z))"#), r#"("x", "y", "z")"#);
}

#[test]
fn path_split_posix_cq() {
    assert_eq!(
        eval_string(r#"stringify(path_split("/tmp/foo.txt"))"#),
        r#"["/", "tmp", "foo.txt"]"#
    );
}

#[test]
fn fileparse_tmp_foo_txt_cq() {
    assert_eq!(
        eval_string(r#"stringify(fileparse("/tmp/foo.txt"))"#),
        r#"("foo.txt", "/tmp", "")"#
    );
}

#[test]
fn abs_float_cq() {
    assert_eq!(eval_string(r#"sprintf("%.10g", abs(-3.5))"#), "3.5");
}

#[test]
fn cbrt_twenty_seven_cq() {
    assert_eq!(eval_string(r#"sprintf("%.10g", cbrt(27))"#), "3");
}

#[test]
fn hypot_two_args_pythagoras_cq() {
    assert_eq!(eval_string(r#"sprintf("%.10g", hypot(3, 4))"#), "5");
}

#[test]
fn hypot_extra_arg_ignored_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", hypot(1, 2, 2))"#),
        "2.236067977"
    );
}

#[test]
fn bessel_j0_at_zero_cq() {
    assert_eq!(eval_string(r#"sprintf("%.10g", j0(0))"#), "1.000000003");
}

#[test]
fn bessel_j1_at_one_cq() {
    assert_eq!(eval_string(r#"sprintf("%.10g", j1(1))"#), "0.4400505857");
}

#[test]
fn erfi_one_half_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", erfi(0.5))"#),
        "0.6149520947"
    );
}

#[test]
fn quotemeta_dots_cq() {
    assert_eq!(eval_string(r#"quotemeta("a.b")"#), r#"a\.b"#);
}

#[test]
fn acronym_phrase_cq() {
    assert_eq!(
        eval_string(r#"acronym("Portable Network Graphics")"#),
        "PNG"
    );
}

#[test]
fn initials_name_cq() {
    assert_eq!(eval_string(r#"initials("John F. Kennedy")"#), "J.F.K.");
}

#[test]
fn chars_graphemes_greek_cq() {
    assert_eq!(eval_string(r#"stringify(chars("αβ"))"#), r#"("α", "β")"#);
}

#[test]
fn running_mean_partial_averages_cq() {
    assert_eq!(
        eval_string(r#"stringify(running_mean([10, 20, 30]))"#),
        "(10, 15, 20)"
    );
}

#[test]
fn running_variance_prefix_cq() {
    assert_eq!(
        eval_string(r#"stringify(running_variance([1, 2, 3, 4]))"#),
        "(0, 0.5, 1, 1.66666666666667)"
    );
}

#[test]
fn running_reduce_implicit_slot_add_cq() {
    assert_eq!(
        eval_string(r#"stringify(running_reduce(sub { $_0 + $_1 }, [1, 2, 3, 4]))"#,),
        "(1, 3, 6, 10)"
    );
}

#[test]
fn running_reduce_dollar_ab_zeros_after_first_bug_cq() {
    assert_eq!(
        eval_string(r#"stringify(running_reduce(sub { $a + $b }, [1, 2, 3, 4]))"#,),
        "(1, 3, 6, 10)"
    );
}

#[test]
fn string_cmp_lex_cq() {
    assert_eq!(eval_string(r#"sprintf("%.0f", ("a" cmp "b"))"#), "-1");
}

#[test]
fn defined_undef_and_scalar_cq() {
    assert_eq!(eval_string(r#"sprintf("%.0f", defined(undef))"#), "0");
    assert_eq!(eval_string(r#"sprintf("%.0f", defined("x"))"#), "1");
}

#[test]
fn numeric_equality_relaxed_strings_cq() {
    assert_eq!(eval_string(r#"sprintf("%.0f", ("02" == "2"))"#), "1");
}

#[test]
fn unzip_alternating_pairs_cq() {
    assert_eq!(
        eval_string(r#"stringify(unzip([1, 10, 2, 20]))"#),
        "([1, 2], [10, 20])"
    );
}

#[test]
fn partition_by_int_div_key_cq() {
    assert_eq!(
        eval_string(r#"stringify(partition_by(sub { int($_ / 2) }, [1, 2, 3, 4, 5, 6]))"#,),
        "([1], [2, 3], [4, 5], [6])"
    );
}

#[test]
fn each_cons_width_three_cq() {
    assert_eq!(
        eval_string(r#"stringify(each_cons(3, 1, 2, 3, 4, 5))"#),
        "([1, 2, 3], [2, 3, 4], [3, 4, 5])"
    );
}

#[test]
fn window_n_width_two_cq() {
    assert_eq!(
        eval_string(r#"stringify(window_n(2, 1, 2, 3, 4))"#),
        "([1, 2], [2, 3], [3, 4])"
    );
}

#[test]
fn frobenius_norm_two_by_two_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", frobenius_norm([[1, 2], [3, 4]]))"#),
        "5.477225575"
    );
}

#[test]
fn matrix_norm_matches_frobenius_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_norm([[1, 2], [3, 4]]))"#),
        "5.477225575"
    );
}

#[test]
fn uri_resolve_byte_vector_absolute_uri_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", uri_resolve([104, 116, 116, 112, 58, 47, 47, 120]))"#),
        "1"
    );
}

#[test]
fn uri_resolve_plain_string_misclassified_relative_bug_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", uri_resolve("http://example.com/x"))"#),
        "1"
    );
}

#[test]
fn uri_normalize_counts_upper_bytes_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", uri_normalize([72, 69, 82, 69]))"#),
        "4"
    );
}

#[test]
fn contains_elem_and_index_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", contains_elem(2, [1, 2, 3]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.0f", index_of_elem(2, [1, 2, 3]))"#),
        "1"
    );
}

#[test]
fn clamp_list_per_element_cq() {
    assert_eq!(
        eval_string(r#"stringify(clamp_list(0, 10, -1, 5, 12))"#),
        "(0, 5, 10)"
    );
}

#[test]
fn acosh_one_cq() {
    assert_eq!(eval_string(r#"sprintf("%.10g", acosh(1))"#), "0");
}

#[test]
fn inverse_hyperbolic_at_zero_cq() {
    assert_eq!(eval_string(r#"sprintf("%.10g", asinh(0))"#), "0");
    assert_eq!(eval_string(r#"sprintf("%.10g", atanh(0))"#), "0");
}

#[test]
fn span_range_max_minus_min_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", span(3, 1, 4, 1, 5, 9))"#),
        "8"
    );
}

#[test]
fn prepend_and_append_elem_cq() {
    assert_eq!(
        eval_string(r#"stringify(prepend(0, 1, 2, 3))"#),
        "(0, 1, 2, 3)"
    );
    assert_eq!(
        eval_string(r#"stringify(append_elem(4, 1, 2, 3))"#),
        "(1, 2, 3, 4)"
    );
}

#[test]
fn sliding_pairs_consecutive_cq() {
    assert_eq!(
        eval_string(r#"stringify(sliding_pairs(1, 2, 3, 4))"#),
        "([1, 2], [2, 3], [3, 4])"
    );
}

#[test]
fn interleave_two_lists_cq() {
    assert_eq!(
        eval_string(r#"stringify(interleave([1, 2], [10, 20]))"#),
        "(1, 10, 2, 20)"
    );
}

#[test]
fn rotate_left_by_one_cq() {
    assert_eq!(
        eval_string(r#"stringify(rotate(1, 1, 2, 3, 4))"#),
        "(2, 3, 4, 1)"
    );
}

#[test]
fn digamma_two_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", digamma(2))"#),
        "0.4227843351"
    );
}

#[test]
fn triangular_number_fourth_cq() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", triangular_number(4))"#),
        "10"
    );
}

#[test]
fn lucas_tenth_cq() {
    assert_eq!(eval_string(r#"sprintf("%.0f", lucas(10))"#), "123");
}

#[test]
fn lc_uc_ascii_cq() {
    assert_eq!(eval_string(r#"lc("PerL")"#), "perl");
    assert_eq!(eval_string(r#"uc("perL")"#), "PERL");
}

#[test]
fn sum0_empty_cq() {
    assert_eq!(eval_string(r#"sprintf("%.0f", sum0([]))"#), "0");
}

#[test]
fn join_empty_separator_cq() {
    assert_eq!(eval_string(r#"join("", qw(a b c))"#), "abc");
}

#[test]
fn compact_drops_undef_cq() {
    assert_eq!(
        eval_string(r#"stringify(compact(1, undef, 2, 0, 3))"#),
        "(1, 2, 0, 3)"
    );
}

#[test]
fn bell_number_fourth_cq() {
    assert_eq!(eval_string(r#"sprintf("%.0f", bell_number(4))"#), "15");
}

#[test]
fn sinc_at_zero_cq() {
    assert_eq!(eval_string(r#"sprintf("%.10g", sinc(0))"#), "1");
}

#[test]
fn repeat_elem_triple_cq() {
    assert_eq!(eval_string(r#"stringify(repeat_elem(7, 3))"#), "(7, 7, 7)");
}

#[test]
fn sum_product_mean_list_cq() {
    assert_eq!(eval_string(r#"sprintf("%.10g", sum_list([1, 2, 3]))"#), "6");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", product_list([2, 3, 4]))"#),
        "24"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mean_list([2, 4, 6]))"#),
        "4"
    );
}
