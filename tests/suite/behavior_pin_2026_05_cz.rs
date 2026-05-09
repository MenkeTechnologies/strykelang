//! Behavior-pinning batch CZ (2026-05-09): **finance** (**`present_value`**, **`future_value`**, **`pmt`** — **BUG-179** wrong
//! permutation), **`amortization_schedule`**, **`bond_price`**, **`bond_convexity`**, **`macaulay_duration`**), **color** (**`hsl_to_rgb`**,
//! **`rgb_to_hsl`**), **set ops** (**`array_union`**, **`array_intersection`**), **meteorology** (**`wind_chill`**), **normal / binomial**
//! (**`pnorm`**, **`qnorm`**, **`dnorm`**, **`pbinom`**, **`ppois`**, **`pexp`**), **byte units** (**`kb_to_bytes`**, **`bytes_to_kb`**), **NT**
//! (**`modinv`**, **`mod_exp`**, **`crt`**, **`next_prime`**, **`prev_prime`**), **versions** (**`compare_versions`**), **bits** (**`bit_and`**, **`bit_or`**,
//! **`bit_xor`**), **formatting** (**`percent`**, **`format_percent`** — no **×100** for fractions — **BUG-180**), **strings** (**`trim`**, **`repeat_string`**,
//! **`strip_prefix`**, **`strip_suffix`**, **`html_encode`**, **`html_decode`**, **`roman_to_int`**, **`int_to_roman`**), **geometry** (**`point_in_polygon`**,
//! **`point_distance`**, **`area_circle`**, **`sphere_volume`**), **more finance** (**`npv`**), **`heat_index`**.

use crate::common::*;

#[test]
fn present_value_discount_three_periods_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", present_value(100, 0.05, 10))"#),
        "61.39132535"
    );
}

#[test]
fn future_value_growth_ten_periods_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", future_value(100, 0.05, 10))"#),
        "162.8894627"
    );
}

#[test]
fn pmt_monthly_loan_standard_order_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pmt(0.05/12, 360, 10000))"#),
        "-53.6821623"
    );
}

/// **`pmt(RATE, NPER, PV)`** — **not** **`(PV, RATE, NPER)`**; a **loan principal** in the rate slot explodes the payment (**BUG-179**).
#[test]
fn pmt_principal_first_slot_absurd_payment_bug_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pmt(10000, 0.05/12, 360))"#),
        "-95618102.42"
    );
}

#[test]
fn pmt_zero_rate_amortizes_principal_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pmt(0, 10, 1000))"#),
        "-100"
    );
}

#[test]
fn hsl_red_and_rgb_roundtrip_cz() {
    assert_eq!(
        eval_string(r#"stringify(hsl_to_rgb(0, 1, 0.5))"#),
        "(255, 0, 0)"
    );
    assert_eq!(
        eval_string(r#"stringify(rgb_to_hsl(255, 0, 0))"#),
        "(0, 1, 0.5)"
    );
}

#[test]
fn array_union_ordered_uniq_cz() {
    assert_eq!(
        eval_string(r#"stringify(array_union([1, 2], [2, 3]))"#),
        "(\"1\", \"2\", \"3\")"
    );
}

#[test]
fn array_intersection_numeric_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", array_intersection([1, 2], [2, 3]))"#),
        "2"
    );
}

#[test]
fn wind_chill_celsius_kmh_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", wind_chill(30, 10))"#),
        "32.52385588"
    );
}

#[test]
fn pnorm_qnorm_dnorm_pbinom_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pnorm(1.96))"#),
        "0.9750021739"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", qnorm(0.975))"#),
        "1.960394917"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dnorm(0))"#),
        "0.3989422804"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pbinom(5, 10, 0.5))"#),
        "0.623046875"
    );
}

#[test]
fn kb_bytes_roundtrip_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kb_to_bytes(1))"#),
        "1024"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bytes_to_kb(1024))"#),
        "1"
    );
}

#[test]
fn bond_price_four_period_coupon_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bond_price(100, 0.05, 0.04, 5))"#),
        "104.4518223"
    );
}

#[test]
fn bond_convexity_semiannual_convention_five_arg_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bond_convexity(100, 0.05, 5, 2, 0.04))"#),
        "6.75850123"
    );
}

#[test]
fn macaulay_duration_from_explicit_cashflows_cz() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", macaulay_duration([5, 5, 5, 5, 105], 0.04))"#,
        ),
        "4.557086742"
    );
}

#[test]
fn amortization_three_month_schedule_cz() {
    assert_eq!(
        eval_string(r#"stringify(amortization_schedule(1000, 0.01, 3))"#),
        "((1, 340.02211148147, 330.02211148147, 10, 669.97788851853), (2, 340.02211148147, 333.322332596285, 6.6997788851853, 336.655555922245), (3, 340.02211148147, 336.655555922248, 3.36655555922245, 0))"
    );
}

#[test]
fn modinv_modexp_crt_cz() {
    assert_eq!(eval_string(r#"sprintf("%.10g", modinv(3, 7))"#), "5");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mod_exp(2, 10, 1000))"#),
        "24"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", crt([2, 3], [3, 5]))"#),
        "8"
    );
}

#[test]
fn compare_versions_semantic_patch_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", compare_versions("1.2.3", "1.2.10"))"#),
        "-1"
    );
}

#[test]
fn bit_and_or_xor_cz() {
    assert_eq!(eval_string(r#"sprintf("%.10g", bit_and(12, 10))"#), "8");
    assert_eq!(eval_string(r#"sprintf("%.10g", bit_or(12, 10))"#), "14");
    assert_eq!(eval_string(r#"sprintf("%.10g", bit_xor(12, 10))"#), "6");
}

#[test]
fn percent_of_total_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", percent(25, 200))"#),
        "12.5"
    );
}

#[test]
fn format_percent_appends_raw_value_cz() {
    assert_eq!(eval_string(r#"format_percent(12.5)"#), "12.5%");
}

/// **`format_percent(x)`** formats **`x`** with a **`%`** suffix — **no** multiply by **100** from a unit fraction (**BUG-180**).
#[test]
fn format_percent_unit_fraction_not_scaled_bug_cz() {
    assert_eq!(eval_string(r#"format_percent(0.125)"#), "0.1%");
}

#[test]
fn trim_and_repeat_string_cz() {
    assert_eq!(eval_string(r#"trim("  ab  ")"#), "ab");
    assert_eq!(eval_string(r#"repeat_string("x", 3)"#), "xxx");
}

#[test]
fn point_in_polygon_unit_square_cz() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", point_in_polygon(1, 1, [[0, 0], [2, 0], [2, 2], [0, 2]]))"#,
        ),
        "1"
    );
}

#[test]
fn point_distance_euclidean_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", point_distance(0, 0, 3, 4))"#),
        "5"
    );
}

#[test]
fn area_circle_sphere_volume_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", area_circle(2))"#),
        "12.56637061"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", sphere_volume(3))"#),
        "113.0973355"
    );
}

#[test]
fn npv_discount_rate_then_cashflows_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", npv(0.1, [-100, 40, 40, 40]))"#),
        "-0.5259203606"
    );
}

#[test]
fn heat_index_celsius_rh_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", heat_index(30, 50))"#),
        "31.04908144"
    );
}

#[test]
fn ppois_pexp_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ppois(4, 3))"#),
        "0.8152632445"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pexp(2, 0.5))"#),
        "0.6321205588"
    );
}

#[test]
fn strip_prefix_suffix_cz() {
    assert_eq!(eval_string(r#"strip_prefix("::foo", "::")"#), "foo");
    assert_eq!(
        eval_string(r#"strip_suffix("foo.pl", ".pl")"#),
        "foo"
    );
}

#[test]
fn next_prev_prime_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", next_prime(20))"#),
        "23"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", prev_prime(20))"#),
        "19"
    );
}

#[test]
fn roman_fourteen_roundtrip_cz() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", roman_to_int("XIV"))"#),
        "14"
    );
    assert_eq!(eval_string(r#"int_to_roman(14)"#), "XIV");
}

#[test]
fn html_encode_decode_ampersand_cz() {
    assert_eq!(eval_string(r#"html_encode("&")"#), "&amp;");
    assert_eq!(
        eval_string(r#"html_decode("&amp;")"#),
        "&"
    );
}
