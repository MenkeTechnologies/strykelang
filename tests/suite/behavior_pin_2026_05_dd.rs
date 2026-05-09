//! Behavior-pinning batch DD (2026-05-10): **R-style stats** (`dbinom`, `pbinom`, `dnorm`, `pnorm`, `qnorm`, `dt`, `pt`), **angle
//! units** (`radians`, `degrees`, `rad_to_deg`, `deg_to_rad`), **matrices** (`matrix_power`, `cholesky`, `matrix_solve`,
//! `dist_matrix`), **KDE** (`kde_epanechnikov`, `kde_silverman_bw`), **set / combinatorics** (`sorensen_dice` on numeric codes,
//! `combinations`, `power_set`), **IPv4** (`ipv4_to_int`, `int_to_ipv4`, `is_valid_ipv4`), **float / list** (`fmod`, `copysign`,
//! `trunc`, `product`, `sum0`), **calendar / constants** (`julian_day`, `golden_ratio`, `supergolden_ratio`), **datetime**
//! **`datetime_strftime(EPOCH, FMT)`** vs swapped args (**BUG-188**), **distances** (`cosine_distance`, `canberra_distance`,
//! **`mahalanobis`** row-shaped data — contrast **BUG-189** panic), **ranks** (`rank`, `dense_rank`), **`gray_code_sequence`**, **`argsort`**, **`log2` / `log10` / `exp`**, **`pascal_row`**, **`multinomial`**, **Fibonacci family** (`fibonacci`, `tribonacci`, `lucas`), **ML loss** (`cross_entropy`, `categorical_cross_entropy`).

use crate::common::*;

#[test]
fn dbinom_pbinom_normal_student_t_dd() {
    assert_eq!(eval_string(r#"sprintf("%.10g", dbinom(5, 10, 0.5))"#), "0.24609375");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pbinom(4, 10, 0.5))"#),
        "0.376953125"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dnorm(0))"#),
        "0.3989422804"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", pnorm(0))"#), "0.5000000005");
    assert_eq!(eval_string(r#"sprintf("%.10g", qnorm(0.5))"#), "0");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dt(0, 5))"#),
        "0.3796066898"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pt(1.96, 10))"#),
        "0.9607823881"
    );
}

#[test]
fn rad_deg_conversions_dd() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", radians(180))"#),
        "3.141592654"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", degrees(3.141592653589793))"#),
        "180"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", rad_to_deg(1))"#),
        "57.29577951"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", deg_to_rad(45))"#),
        "0.7853981634"
    );
}

#[test]
fn matrix_power_cholesky_solve_dd() {
    assert_eq!(
        eval_string(r#"stringify(matrix_power([[1, 2], [3, 4]], 2))"#),
        "([7, 10], [15, 22])"
    );
    assert_eq!(
        eval_string(r#"stringify(cholesky([[4, 2], [2, 3]]))"#),
        "((2, 0), (1, 1.4142135623731))"
    );
    assert_eq!(
        eval_string(r#"stringify(matrix_solve([[2, 0], [0, 3]], [4, 9]))"#),
        "(2, 3)"
    );
}

#[test]
fn dist_matrix_euclidean_two_points_dd() {
    assert_eq!(
        eval_string(r#"stringify(dist_matrix([[0, 0], [3, 4]], "euclidean"))"#),
        "((0, 5), (5, 0))"
    );
}

#[test]
fn kde_epanechnikov_silverman_bw_dd() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kde_epanechnikov(0, 1, 0))"#),
        "0.75"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kde_silverman_bw([1, 2, 3, 4, 5]))"#),
        "0"
    );
}

#[test]
fn sorensen_dice_numeric_char_codes_dd() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", sorensen_dice([97, 98, 99], [97, 98, 100]))"#),
        "0.6666666667"
    );
}

#[test]
fn combinations_two_of_three_dd() {
    assert_eq!(
        eval_string(r#"stringify(combinations(2, [1, 2, 3]))"#),
        "([1, 2], [1, 3], [2, 3])"
    );
}

#[test]
fn power_set_three_elements_dd() {
    assert_eq!(
        eval_string(r#"stringify(power_set([1, 2]))"#),
        "([], [1], [2], [1, 2])"
    );
}

#[test]
fn ipv4_roundtrip_and_invalid_dd() {
    assert_eq!(
        eval_string(r#"sprintf("%d", ipv4_to_int("192.168.1.1"))"#),
        "3232235777"
    );
    assert_eq!(eval_string(r#"int_to_ipv4(3232235777)"#), "192.168.1.1");
    assert_eq!(eval_int(r#"is_valid_ipv4("256.1.1.1")"#), 0);
}

#[test]
fn fmod_copysign_trunc_product_sum0_dd() {
    assert_eq!(eval_string(r#"sprintf("%.10g", fmod(7, 3))"#), "1");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", copysign(1, -5))"#),
        "-1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", trunc(-2.7))"#),
        "-2"
    );
    // Lone **`[...]`** operand: **`product`** numifies the ref (**BUG-140**); comma factors multiply.
    assert_eq!(eval_string(r#"sprintf("%.10g", product([2, 3, 4]))"#), "0");
    assert_eq!(eval_string(r#"sprintf("%.10g", product(2, 3, 4))"#), "24");
    assert_eq!(eval_string(r#"sprintf("%.10g", sum0([]))"#), "0");
}

#[test]
fn julian_golden_constants_dd() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", julian_day(2020, 1, 1))"#),
        "2458849.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", golden_ratio())"#),
        "1.618033989"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", supergolden_ratio())"#),
        "1.465571232"
    );
}

/// Correct: **`datetime_strftime(EPOCH, FMT)`**. Swapping prints the **numeric epoch** back (**BUG-188**).
#[test]
fn datetime_strftime_epoch_then_fmt_dd() {
    assert_eq!(
        eval_string(r#"datetime_strftime(1700000000, "%Y")"#),
        "2023"
    );
}

#[test]
fn datetime_strftime_swapped_args_returns_epoch_dd() {
    assert_eq!(
        eval_string(r#"datetime_strftime("%Y", 1700000000)"#),
        "1700000000"
    );
}

#[test]
fn cosine_canberra_distance_dd() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cosine_distance([1, 0, 0], [1, 1, 0]))"#),
        "0.2928932188"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", canberra_distance([1, 2, 3], [2, 3, 4]))"#),
        "0.6761904762"
    );
}

#[test]
fn mahalanobis_two_row_obs_dd() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", mahalanobis([[0, 0]], [1, 1], [[1, 0], [0, 1]])->[0])"#
        ),
        "1.414213562"
    );
}

#[test]
fn rank_dense_rank_order_dd() {
    assert_eq!(
        eval_string(r#"stringify(rank([3, 1, 2]))"#),
        "(3, 1, 2)"
    );
    assert_eq!(
        eval_string(r#"stringify(dense_rank([3, 1, 2]))"#),
        "(3, 1, 2)"
    );
}

#[test]
fn gray_code_sequence_three_bits_dd() {
    assert_eq!(
        eval_string(r#"stringify(gray_code_sequence(3))"#),
        "(0, 1, 3, 2, 6, 7, 5, 4)"
    );
}

#[test]
fn argsort_permutation_dd() {
    assert_eq!(
        eval_string(r#"stringify(argsort([30, 10, 20]))"#),
        "(1, 2, 0)"
    );
}

#[test]
fn log2_log10_exp_dd() {
    assert_eq!(eval_string(r#"sprintf("%.10g", log2(8))"#), "3");
    assert_eq!(eval_string(r#"sprintf("%.10g", log10(100))"#), "2");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", exp(1))"#),
        "2.718281828"
    );
}

#[test]
fn pascal_row_binomial_multinomial_dd() {
    assert_eq!(
        eval_string(r#"stringify(pascal_row(4))"#),
        "(1, 4, 6, 4, 1)"
    );
    assert_eq!(eval_string(r#"sprintf("%.0f", multinomial(6, [2, 2, 2]))"#), "90");
}

#[test]
fn fibonacci_tribonacci_lucas_dd() {
    assert_eq!(eval_int(r#"fibonacci(10)"#), 55);
    assert_eq!(eval_int(r#"tribonacci(8)"#), 24);
    assert_eq!(eval_int(r#"lucas(6)"#), 18);
}

#[test]
fn cross_entropy_categorical_dd() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cross_entropy([0.5, 0.5], [0.6, 0.4]))"#),
        "0.7135581778"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", categorical_cross_entropy([0, 1, 0], [0.2, 0.7, 0.1]))"#
        ),
        "0.118891648"
    );
}
