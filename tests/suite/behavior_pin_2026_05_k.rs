//! Behavior-pinning batch K (2026-05-04): math constants, additional list
//! builtins (head/tail/counter/enumerate/find/zip), path/string padding,
//! Schwartzian-friendly variants (max_by/min_by/pairwise).

use crate::common::*;

// ── Math constants ──────────────────────────────────────────────────────────

#[test]
fn pi_constant_known_value() {
    assert_eq!(eval_string("pi"), "3.14159265358979");
}

#[test]
fn tau_constant_is_two_pi() {
    let s = eval_string("tau");
    assert!(
        s.starts_with("6.283185307179"),
        "expected ~2*pi, got {:?}",
        s
    );
}

#[test]
fn pi_uppercase_is_a_constant_alias() {
    // Uppercase `PI` / `TAU` / `E` are constants alongside the lowercase
    // `pi` / `tau` / `euler_e` aliases. The `open FH, ...` filehandle-slot
    // path in the parser takes precedence so the Perl idiom `open E, …`
    // keeps treating `E` as a literal handle name.
    let pi = eval_string("PI");
    assert!(pi.starts_with("3.14"), "expected pi, got {:?}", pi);
    let tau = eval_string("TAU");
    assert!(tau.starts_with("6.28"), "expected tau, got {:?}", tau);
    let e = eval_string("E");
    assert!(e.starts_with("2.718"), "expected e, got {:?}", e);
}

#[test]
fn e_alone_is_parse_error_today() {
    // `e` looks like `eq` / `each` start; standalone produces "Unexpected
    // token Eof" because the parser expects a continuation.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind("print e");
    assert!(
        matches!(kind, ErrorKind::Syntax),
        "expected syntax error, got {:?}",
        kind
    );
}

#[test]
fn exp_one_yields_e() {
    let s = eval_string(r#"sprintf("%.10f", exp(1))"#);
    assert_eq!(s, "2.7182818285");
}

// ── Math: hypot, cbrt, log10, log2, sin/cos/tan ────────────────────────────

#[test]
fn hypot_three_four_yields_five() {
    assert_eq!(eval_int("hypot(3, 4)"), 5);
}

#[test]
fn cbrt_twenty_seven_yields_three() {
    assert_eq!(eval_int("cbrt(27)"), 3);
}

#[test]
fn log10_hundred_yields_two() {
    assert_eq!(eval_int("log10(100)"), 2);
}

#[test]
fn log2_eight_yields_three() {
    assert_eq!(eval_int("log2(8)"), 3);
}

#[test]
fn sin_zero_is_zero() {
    assert_eq!(eval_int("sin(0)"), 0);
}

#[test]
fn cos_zero_is_one() {
    assert_eq!(eval_int("cos(0)"), 1);
}

#[test]
fn tan_zero_is_zero() {
    assert_eq!(eval_int("tan(0)"), 0);
}

// ── List slicing helpers: head/tail (LIST, N) shape ─────────────────────────

#[test]
fn head_list_then_n_returns_first_n() {
    assert_eq!(
        eval_string(r#"my @r = head(qw(a b c d e), 3); "@r""#),
        "a b c"
    );
}

#[test]
fn head_n_first_returns_just_n_today() {
    // BUG-065: passing `head(N, LIST)` (Perl-ish ordering) returns just the
    // count `N`, not the first N elements.
    assert_eq!(eval_string(r#"my @r = head(3, qw(a b c d e)); "@r""#), "3");
}

#[test]
fn head_no_args_returns_first_element_only() {
    // `head(LIST)` with no count returns the first element.
    assert_eq!(eval_int(r#"head(1..10)"#), 1);
}

#[test]
fn tail_no_count_returns_last_element_only() {
    assert_eq!(eval_int(r#"tail(1..10)"#), 10);
}

#[test]
fn tail_list_then_count_returns_last_n() {
    // `tail(LIST, N)` returns the last N items, mirroring `head(LIST, N)`.
    assert_eq!(eval_string(r#"my @r = tail(1..5, 2); "@r""#), "4 5");
}

// ── enumerate / counter / find / max_by / min_by ────────────────────────────

#[test]
fn enumerate_returns_index_value_pairs() {
    let out = eval_string(
        r#"my @r = enumerate(qw(a b c));
           join(",", map { "$_->[0]:$_->[1]" } @r)"#,
    );
    assert_eq!(out, "0:a,1:b,2:c");
}

#[test]
fn counter_returns_frequency_hashref() {
    assert_eq!(
        eval_string(
            r#"my $c = counter(1, 2, 3, 1, 2);
               join(",", map { "$_=$c->{$_}" } sort keys %$c)"#
        ),
        "1=2,2=2,3=1"
    );
}

#[test]
fn find_with_block_returns_first_match() {
    assert_eq!(eval_int(r#"find { $_ > 5 } 1..10"#), 6);
}

#[test]
fn max_by_with_block_orders_by_length() {
    assert_eq!(
        eval_string(r#"max_by { length $_ } qw(a bbb cc dddd ee)"#),
        "dddd"
    );
}

#[test]
fn min_by_with_block_orders_by_length() {
    assert_eq!(
        eval_string(r#"min_by { length $_ } qw(a bbb cc dddd ee)"#),
        "a"
    );
}

#[test]
fn first_with_block_alias_of_find() {
    // `first` is the Perl-classic name; both work and return the same.
    assert_eq!(eval_int(r#"first { $_ > 5 } 1..10"#), 6);
}

// ── zip handles equal- and unequal-length inputs ───────────────────────────

#[test]
fn zip_three_equal_arrays_yields_three_triplets() {
    assert_eq!(
        eval_int(r#"my @r = zip [1,2,3], [10,20,30], [100,200,300]; scalar @r"#),
        3
    );
}

#[test]
fn zip_unequal_arrays_pads_to_longer() {
    // Stryke pads shorter input with undef so the result has max-length rows.
    assert_eq!(eval_int(r#"my @r = zip [1,2], [10,20,30]; scalar @r"#), 3);
}

// ── Path manipulation ──────────────────────────────────────────────────────

#[test]
fn basename_strips_directory_prefix() {
    assert_eq!(eval_string(r#"basename("/usr/local/bin/foo")"#), "foo");
}

#[test]
fn dirname_returns_directory_part() {
    assert_eq!(
        eval_string(r#"dirname("/usr/local/bin/foo")"#),
        "/usr/local/bin"
    );
}

#[test]
fn path_join_concatenates_with_separator() {
    assert_eq!(
        eval_string(r#"path_join("foo", "bar", "baz.txt")"#),
        "foo/bar/baz.txt"
    );
}

// ── String padding and centering ───────────────────────────────────────────

#[test]
fn rpad_pads_on_right() {
    assert_eq!(eval_string(r#"rpad("hi", 10, ".")"#), "hi........");
}

#[test]
fn lpad_pads_on_left() {
    assert_eq!(eval_string(r#"lpad("hi", 10, "0")"#), "00000000hi");
}

#[test]
fn center_pads_both_sides() {
    assert_eq!(eval_string(r#"center("hi", 10, "-")"#), "----hi----");
}

// ── repeat() string-repetition function ─────────────────────────────────────

#[test]
fn repeat_with_count_repeats_string() {
    assert_eq!(eval_string(r#"repeat("ab", 3)"#), "ababab");
}

// ── pairwise / Schwartzian-style block forms ────────────────────────────────
//
// `pairwise` with the block form fails today; pin the failure so we don't
// regress when the user writes the natural Perl-style code.

#[test]
fn pairwise_block_form_returns_empty_today() {
    // BUG-066: `pairwise { $a + $b } @a, @b` returns an empty list whether
    // the arrays are named or built inline. Pin until the block-form is
    // wired up.
    assert_eq!(
        eval_int(
            r#"my @a = (1,2,3); my @b = (10,20,30);
               my @r = pairwise { $a + $b } @a, @b;
               scalar @r"#
        ),
        0
    );
}

// ── Sum default to 0 ────────────────────────────────────────────────────────

#[test]
fn sum0_returns_zero_for_empty_list() {
    assert_eq!(eval_int("sum0()"), 0);
}

#[test]
fn sum_returns_zero_for_empty_list() {
    // Stryke's `sum()` returns 0 for the empty list (Perl's List::Util sum
    // returns undef, sum0 returns 0).
    assert_eq!(eval_int("sum()"), 0);
}

// ── @-prefix as a sub-call surface is a runtime error from -e source ───────

#[test]
fn at_prefix_undeclared_array_evaluates_to_empty_list() {
    // From inside the library `eval` API, `@undeclared_name` is treated as
    // an empty array, not as a stryke shell-dispatch. The `@`-prefix is a
    // CLI/embedding feature, not a source-level construct.
    assert_eq!(eval_int(r#"my @r = @stk_test_undefined_xx; scalar @r"#), 0);
}

// ── `find_index` not implemented ────────────────────────────────────────────

#[test]
fn find_index_returns_first_matching_index() {
    // `find_index { BLOCK } LIST` returns the zero-based index of the first
    // match, or -1 if no element matches.
    assert_eq!(eval_int(r#"find_index { $_ > 5 } 1..10"#), 5);
    assert_eq!(eval_int(r#"find_index { $_ > 99 } 1..10"#), -1);
}

// ── Math sanity ─────────────────────────────────────────────────────────────

#[test]
fn sqrt_two_squared_round_trip() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", sqrt(2) ** 2)"#),
        "2.0000000000"
    );
}

#[test]
fn atan2_one_zero_is_half_pi() {
    let s = eval_string(r#"sprintf("%.4f", atan2(1, 0))"#);
    assert_eq!(s, "1.5708");
}

// ── Integer divmod / modulus identities ─────────────────────────────────────

#[test]
fn integer_divmod_identity_for_positive_args() {
    // (a / b) * b + (a % b) == a, modulo float quirks.
    assert_eq!(eval_int("int(17 / 5) * 5 + 17 % 5"), 17);
}

// ── A slice of stryke's parallel built-ins to confirm they exist ───────────

#[test]
fn pgrep_filters_via_block() {
    assert_eq!(
        eval_string(r#"my @r = pgrep { _ % 2 == 0 } 1..10; "@r""#),
        "2 4 6 8 10"
    );
}

#[test]
fn psort_sorts_via_comparator() {
    assert_eq!(
        eval_string(r#"my @r = psort { $a <=> $b } 5,3,1,4,2; "@r""#),
        "1 2 3 4 5"
    );
}

// ── String repeat operator vs builtin ───────────────────────────────────────

#[test]
fn x_operator_and_repeat_builtin_agree() {
    assert_eq!(
        eval_string(r#""ab" x 3"#),
        eval_string(r#"repeat("ab", 3)"#)
    );
}

// ── enumerate edge case: empty list ─────────────────────────────────────────

#[test]
fn enumerate_empty_list_yields_empty_array() {
    assert_eq!(eval_int(r#"my @r = enumerate(); scalar @r"#), 0);
}
