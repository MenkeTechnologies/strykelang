//! Behavior-pinning batch CE (2026-05-09): corporate-finance / derivatives helpers — **NPV**, **IRR**, **XIRR**, **payback**
//! (incl. BUG-126 truncated variadic cashflow buckets), **bond** duration family, **annuity** PV/FV, common **retail** builtins
//! (**`roi`**, **`cagr`**, **`markup`/`margin`/`tip`/`tax`/`discount`**, **`break_even`**), **CAPM** ratios, **Sharpe**/**Sortino**/**max_drawdown**,
//! **Black–Scholes** prices + **BS greeks** (BUG-132 call-only formulas), **put–call parity**, **YTM**/accrued helpers,
//! **loan** amortization (**`loan_payment_pmt`**, **`loan_balance`**, **`amortization_total_interest`**, **`apr_to_apy`**), **depreciation_linear** vs **`depreciation_double`**
//! (BUG-133 middle-arg salvage ignored).

use crate::common::*;

#[test]
fn npv_array_discounts_four_uniform_periods_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", npv(0.1, [-1000, 300, 420, 500]))"#),
        "-4.50788880540961"
    );
}

#[test]
fn npv_variadic_second_bucket_only_counts_lead_outflow_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", npv(0.1, -1000, 300, 420, 500))"#),
        "-1000.00000000000000"
    );
}

#[test]
fn irr_array_newton_positive_rate_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", irr([-1000, 220, 281, 350, 400]))"#),
        "0.08668363372381"
    );
}

#[test]
fn irr_variadic_first_flow_only_interprets_second_as_guess_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", irr(-1000, 220, 281, 350, 400))"#),
        "220.00000000000000"
    );
}

#[test]
fn irr_satisfies_npv_near_zero_residual_ce() {
    assert_eq!(
        eval_string(
            r#"sprintf("%d", abs(npv(irr([-1000, 220, 281, 350, 400]), [-1000, 220, 281, 350, 400])) < 1e-10)"#
        ),
        "1"
    );
}

#[test]
fn duration_macaulay_two_coupon_bond_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", duration([60, 1060], 0.05))"#),
        "1.94390026714159"
    );
}

#[test]
fn modified_duration_divides_mac_by_one_plus_yield_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", modified_duration([60, 1060], 0.05))"#),
        "1.85133358775389"
    );
}

#[test]
fn bond_duration_alias_matches_duration_helper_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", bond_duration([60, 1060], 0.05))"#),
        "1.94390026714159"
    );
}

#[test]
fn macauley_duration_wolfram_sig_yields_first_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", macauley_duration(0.05, [60, 1060]))"#),
        "1.94390026714159"
    );
}

#[test]
fn convexity_two_payment_stream_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", convexity(0.05, [60, 1060]))"#),
        "5.23864042500348"
    );
}

#[test]
fn future_value_compound_decennial_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", future_value(1000, 0.05, 10))"#),
        "1628.89462677744200"
    );
}

#[test]
fn present_value_inverts_decennial_compound_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", present_value(1628.89462677744, 0.05, 10))"#),
        "999.99999999999875"
    );
}

#[test]
fn annuity_present_value_ten_coupons_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", annuity_present_value(60, 0.05, 10))"#),
        "463.30409575108877"
    );
}

#[test]
fn annuity_future_value_ten_coupons_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", annuity_future_value(60, 0.05, 10))"#),
        "754.67355213292990"
    );
}

#[test]
fn cagr_three_year_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", cagr(100, 150, 3))"#),
        "0.14471424255333"
    );
}

#[test]
fn roi_percent_return_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", roi(150, 100))"#),
        "0.50000000000000"
    );
}

#[test]
fn break_even_units_ceil_margin_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", break_even(5000, 25, 10))"#),
        "334.00000000000000"
    );
}

#[test]
fn markup_twenty_five_percent_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", markup(40, 50))"#),
        "25.00000000000000"
    );
}

#[test]
fn margin_twenty_percent_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", margin(40, 50))"#),
        "20.00000000000000"
    );
}

#[test]
fn tip_eighteen_percent_bill_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", tip(80, 18))"#),
        "14.40000000000000"
    );
}

#[test]
fn tax_pct_adds_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", tax(100, 8.25))"#),
        "108.25000000000000"
    );
}

#[test]
fn discount_twenty_percent_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", discount(100, 20))"#),
        "80.00000000000000"
    );
}

#[test]
fn perpetuity_value_cash_over_rate_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", perpetuity_value(120, 0.06))"#),
        "2000.00000000000000"
    );
}

#[test]
fn growing_perpetuity_spread_denominator_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", growing_perpetuity(100, 0.10, 0.03))"#),
        "1428.57142857142844"
    );
}

#[test]
fn continuous_compound_twelve_horizon_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", continuous_compound(1000, 0.05, 12))"#),
        "1822.11880039050902"
    );
}

#[test]
fn capm_expected_return_beta_point_two_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", capm_expected_return(0.02, 1.2, 0.09))"#),
        "0.10400000000000"
    );
}

#[test]
fn treynor_ratio_excess_over_beta_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", treynor_ratio(0.11, 0.02, 1.15))"#),
        "0.07826086956522"
    );
}

#[test]
fn jensens_alpha_four_args_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", jensens_alpha(0.12, 0.03, 0.9, 0.095))"#),
        "0.03150000000000"
    );
}

#[test]
fn information_ratio_tracking_noise_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", information_ratio(0.10, 0.07, 0.012))"#),
        "2.50000000000000"
    );
}

#[test]
fn sharpe_ratio_array_rf_one_percent_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", sharpe_ratio([-0.01, 0.04, -0.02, 0.06, 0.01], 0.01))"#),
        "0.19955703157132"
    );
}

#[test]
fn sortino_ratio_downside_dev_only_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", sortino_ratio([-0.01, 0.04, -0.02, 0.06, 0.01], 0.01))"#),
        "0.23533936216582"
    );
}

#[test]
fn max_drawdown_percent_decimal_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", max_drawdown([100, 110, 95, 105, 80, 90]))"#),
        "0.27272727272727"
    );
}

#[test]
fn payback_period_fractionates_final_coupon_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", payback_period(420, [200, 100, 200, 50]))"#),
        "2.60000000000000"
    );
}

#[test]
fn payback_requires_array_bucket_second_arg_ce() {
    assert_eq!(
        eval_string(r#"is_defined(payback_period(420, 200, 100, 200, 50))"#),
        "0"
    );
}

#[test]
fn discounted_payback_positive_discount_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", discounted_payback(420, [200, 100, 200, 50], 0.1))"#),
        "3.15444000000000"
    );
}

#[test]
fn discounted_payback_requires_array_middle_bucket_ce() {
    assert_eq!(
        eval_string(r#"is_defined(discounted_payback(420, 200, 100, 200, 50, 0.1))"#),
        "0"
    );
}

#[test]
fn black_scholes_call_near_atm_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", black_scholes_call(100, 105, 1, 0.05, 0.2))"#),
        "8.02135223514318"
    );
}

#[test]
fn black_scholes_put_near_atm_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", black_scholes_put(100, 105, 1, 0.05, 0.2))"#),
        "7.90044180771815"
    );
}

#[test]
fn black_scholes_put_call_parity_residual_zero_ce() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.14f", black_scholes_call(100, 105, 1, 0.05, 0.2) - black_scholes_put(100, 105, 1, 0.05, 0.2) - (100 - 105 * exp(-0.05)))"#
        ),
        "0.00000000000000"
    );
}

#[test]
fn bs_delta_returns_call_delta_cdf_d1_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", bs_delta(100, 105, 1, 0.05, 0.2))"#),
        "0.54222838675953"
    );
}

#[test]
fn bs_put_delta_equals_call_delta_minus_one_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.17f", bs_delta(100, 105, 1, 0.05, 0.2) - 1)"#),
        "-0.45777161324046822"
    );
}

#[test]
fn bs_gamma_positive_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", bs_gamma(100, 105, 1, 0.05, 0.2))"#),
        "0.01983526190421"
    );
}

#[test]
fn bs_vega_positive_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", bs_vega(100, 105, 1, 0.05, 0.2))"#),
        "39.67052380842653"
    );
}

#[test]
fn bs_theta_call_style_negative_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", bs_theta(100, 105, 1, 0.05, 0.2))"#),
        "-6.27712613411200"
    );
}

#[test]
fn bs_rho_call_style_positive_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", bs_rho(100, 105, 1, 0.05, 0.2))"#),
        "46.20147506538684"
    );
}

#[test]
fn yield_to_maturity_discount_bond_semiannual_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", yield_to_maturity(95, 100, 0.05, 5, 2))"#),
        "0.06177624640903"
    );
}

#[test]
fn accrued_interest_half_year_face_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", accrued_interest(100000, 0.05, 180))"#),
        "2465.75342465753420"
    );
}

#[test]
fn clean_price_subtracts_accrued_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", clean_price(98.5, 1.23287671232877))"#),
        "97.26712328767123"
    );
}

#[test]
fn dirty_price_adds_accrued_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", dirty_price(98.5, 1.23287671232877))"#),
        "99.73287671232877"
    );
}

#[test]
fn depreciation_linear_with_salvage_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", depreciation_linear(10000, 1000, 5))"#),
        "1800.00000000000000"
    );
}

#[test]
fn depreciation_double_ignores_salvage_middle_arg_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", depreciation_double(10000, 1000, 5))"#),
        "4000.00000000000000"
    );
}

#[test]
fn depreciation_double_middle_arg_does_not_affect_rate_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", depreciation_double(10000, 999999, 5))"#),
        "0.00000000000000"
    );
}

#[test]
fn xirr_three_equal_annual_steps_ce() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.14f", xirr([-10000, 2750, 4250, 3250], [0, 365, 730, 1095], 0.1))"#
        ),
        "0.01214626375078"
    );
}

#[test]
fn loan_payment_pmt_three_six_zero_monthly_five_pct_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", loan_payment_pmt(200000, 0.05 / 12, 360))"#),
        "1073.6432460243"
    );
}

#[test]
fn loan_balance_after_twelve_payments_ce() {
    assert_eq!(
        eval_string(
            r#"my $pmt = loan_payment_pmt(200000, 0.05 / 12, 360);
sprintf("%.8f", loan_balance(200000, 0.05 / 12, $pmt, 12))"#
        ),
        "197049.26930887"
    );
}

#[test]
fn amortization_total_interest_full_schedule_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", amortization_total_interest(200000, 0.05 / 12, 360))"#),
        "186511.56856873945799"
    );
}

#[test]
fn apr_to_apy_monthly_compounding_ce() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", apr_to_apy(0.048, 12))"#),
        "0.04907020753481"
    );
}
