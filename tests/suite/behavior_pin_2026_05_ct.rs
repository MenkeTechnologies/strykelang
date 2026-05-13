//! Behavior-pinning batch CT (2026-05-09): classical **cdf/pmf** bridges (**`pbinom`**, **`dbinom`**, **`ppois`**,
//! **`punif`**, **`pexp`**, **`qnorm`**), **moments** (**`skewness`**, **`kurtosis`**, **`rms`**), **buckets**
//! (**`bucket`**), **winsor** / **Herfindahl** (**BUG-169** — **`hhi`** first-arg-only shares), **set overlap**
//! (**`array_intersection`**, **`jaccard_index`**), **inequality** (**`gini_coefficient`**, **`atkinson_index`**),
//! **quadrature & geo** (**`trapz`**, **`romberg_quad`**, **`simpson_rule`**, **`great_circle_bearing`**,
//! **`sphere_volume`**), **CRT**, **Collatz**, **norms** (**`minkowski_distance`**).

use crate::common::*;

#[test]
fn pbinom_five_ten_half_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pbinom(5, 10, 0.5))"#),
        "0.623046875"
    );
}

#[test]
fn dbinom_five_ten_half_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dbinom(5, 10, 0.5))"#),
        "0.24609375"
    );
}

#[test]
fn ppois_four_lambda_two_five_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ppois(4, 2.5))"#),
        "0.8911780189"
    );
}

#[test]
fn punif_quarter_unit_interval_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", punif(0.25, 0, 1))"#),
        "0.25"
    );
}

#[test]
fn pexp_one_unit_rate_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pexp(1, 1))"#),
        "0.6321205588"
    );
}

#[test]
fn qnorm_ninety_seven_five_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", qnorm(0.975))"#),
        "1.959963985"
    );
}

#[test]
fn skewness_right_tail_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", skewness([1, 2, 3, 4, 10]))"#),
        "2.371708245"
    );
}

#[test]
fn kurtosis_uniformish_short_list_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kurtosis([1, 2, 3, 4, 5]))"#),
        "-1.3"
    );
}

#[test]
fn rms_three_four_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", rms([3, 4]))"#),
        "3.535533906"
    );
}

#[test]
fn bucket_width_ten_three_bins_ct() {
    assert_eq!(
        eval_string(r#"stringify(bucket(10, 5, 15, 25))"#),
        r#"("0", (5), "10", (15), "20", (25))"#
    );
}

#[test]
fn winsorize_ten_percent_ten_integers_ct() {
    assert_eq!(
        eval_string(r#"stringify(winsorize(10, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10))"#),
        "(2, 2, 3, 4, 5, 6, 7, 8, 9, 10)"
    );
}

#[test]
fn herfindahl_three_shares_array_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", hhi([0.3, 0.3, 0.4]))"#),
        "0.34"
    );
}

/// **`hhi(…)`** only reads **`args[0]`** — pass a single array of shares (**BUG-169**).
#[test]
fn herfindahl_variadic_uses_first_share_only_bug_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", hhi(0.3, 0.3, 0.4))"#),
        "0.34"
    );
}

#[test]
fn array_intersection_two_lists_ct() {
    assert_eq!(
        eval_string(r#"stringify(array_intersection([1, 2, 3], [2, 3, 4]))"#),
        r#"("2", "3")"#
    );
}

#[test]
fn jaccard_index_two_pairs_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaccard_index([1, 2], [2, 3]))"#),
        "0.3333333333"
    );
}

#[test]
fn gini_three_incomes_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gini_coefficient([10, 20, 30]))"#),
        "0.2222222222"
    );
}

#[test]
fn atkinson_eps_half_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", atkinson_index([1, 2, 3, 4], 0.5))"#),
        "0.05558585737"
    );
}

#[test]
fn trapz_three_samples_half_step_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", trapz([0, 2, 4], 0.5))"#),
        "2"
    );
}

#[test]
fn romberg_quad_two_step_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", romberg_quad(1.5, 1.4, 2))"#),
        "1.506666667"
    );
}

#[test]
fn simpson_rule_parabola_mass_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", simpson_rule([1, 4, 1], 1))"#),
        "6"
    );
}

#[test]
fn great_circle_bearing_north_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", great_circle_bearing(0, 0, 0, 1))"#),
        "90"
    );
}

#[test]
fn sphere_volume_radius_two_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", sphere_volume(2))"#),
        "33.51032164"
    );
}

#[test]
fn degrees_atan2_one_one_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", degrees(atan2(1, 1)))"#),
        "45.0000000000"
    );
}

#[test]
fn kron_product_lengths_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", kron_product([1, 2], [3, 4, 5]))"#),
        "3"
    );
}

#[test]
fn mode_repeated_twice_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", mode_val([1, 2, 2, 3]))"#),
        "2"
    );
}

#[test]
fn minkowski_l1_cityblock_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", minkowski_distance([0, 0], [3, 4], 1))"#),
        "7.0000000000"
    );
}

#[test]
fn digital_root_12345_ct() {
    assert_eq!(eval_string(r#"sprintf("%.0f", digital_root(12345))"#), "6");
}

#[test]
fn collatz_length_seven_ct() {
    assert_eq!(eval_string(r#"sprintf("%.0f", collatz_length(7))"#), "16");
}

#[test]
fn chinese_remainder_two_moduli_ct() {
    assert_eq!(eval_string(r#"sprintf("%.0f", crt([2, 3], [5, 7]))"#), "17");
}

#[test]
fn iqr_one_to_ten_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", iqr([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]))"#),
        "5"
    );
}

#[test]
fn bowley_skew_symmetric_seven_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bowley_skewness([1, 2, 3, 4, 5, 6, 7]))"#),
        "0"
    );
}

#[test]
fn trimmed_mean_ten_percent_outlier_ct() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", trimmed_mean(10, [1, 2, 3, 4, 5, 6, 7, 8, 9, 100]))"#),
        "5.5"
    );
}
