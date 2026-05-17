//! Behavior-pinning batch CC (2026-05-09): special functions (**Lambert \(W\) pathologies — BUG-128**),
//! integrals (Si, Ci, exponential integral \(E_1\)), combinatorics (factorial/Fibonacci/Harmonic partial sums/Stirling\(^2\) types/Bell numbers),
//! stats aggregates (`range_of`, `variance`, `mode_val`), analytic extras (`inverse_erf`, `lgamma`, `erfc`, `j0`, `digamma`,
//! `polygamma`, Struve/`sinhc`, `cosh_minus1_over_x2`, `acosh`/`asinh`/`copysign`), Euclidean helpers (`l2_norm`, `vector_normalize`),
//! spheres / Heron's formula, Euclidean `gcd`/`lcm`, and `mod_exp`.

use crate::common::*;

#[test]
fn lambert_w_omega_constant_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lambert_w(1))"#),
        "0.567143290410"
    );
}

#[test]
fn lambert_w_at_exp_two_known_branch_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lambert_w(exp(2)))"#),
        "1.557145598998"
    );
}

#[test]
fn lambert_w_lower_branch_near_neg_inv_e_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lambert_w(-exp(-1)))"#),
        "-0.999999992551"
    );
}

#[test]
fn lambert_w0_at_e_equals_one_principal_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lambert_w0(2.718281828459045))"#),
        "1.000000000000"
    );
}

/// BUG-128 — initial guess branch uses \(\ln x - \ln\ln x\); **`x == 1` ⇒ `\ln(\ln 1)` is undefined**, entire iterate becomes NaN.
#[test]
fn lambert_w0_at_exactly_one_is_nan_bug_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lambert_w0(1))"#),
        "0.567143290410"
    );
}

#[test]
fn lambert_w0_below_one_finite_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lambert_w0(0.5))"#),
        "0.351733711249"
    );
}

#[test]
fn lambert_w0_above_one_finite_two_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lambert_w0(2))"#),
        "0.852605502014"
    );
}

#[test]
fn lambert_w0_at_zero_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lambert_w0(0))"#),
        "0.000000000000"
    );
}

#[test]
fn wright_omega_exponential_branch_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", wright_omega(1))"#),
        "1.000000000000"
    );
}

/// Same NaN cascade as **`lambert_w0`** at argument **1**, because \(\omega(0)=W(e^0)=W(1)\).
#[test]
fn wright_omega_zero_is_nan_bug_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", wright_omega(0))"#),
        "0.567143290410"
    );
}

#[test]
fn digamma_two_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", digamma(2))"#),
        "0.422784335098"
    );
}

#[test]
fn polygamma_second_at_one_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", polygamma(2, 1))"#),
        "-2.404112807319"
    );
}

#[test]
fn struve_h0_two_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", struve_h0(2))"#),
        "0.790858849508"
    );
}

#[test]
fn sinhc_one_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", sinhc(1))"#),
        "1.175201193644"
    );
}

#[test]
fn cosh_minus1_over_xx_at_two_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", cosh_minus1_over_x2(2))"#),
        "0.690548922771"
    );
}

#[test]
fn sine_integral_si_near_pi_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", sine_integral_si(3.14159))"#),
        "1.851937051981"
    );
}

#[test]
fn cosine_integral_ci_ten_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", cosine_integral_ci(10))"#),
        "-11.082827837554"
    );
}

#[test]
fn exp_integral_e1_one_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", exp_integral_e1(1))"#),
        "0.367879441171"
    );
}

#[test]
fn inverse_erf_one_half_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", inverse_erf(0.5))"#),
        "0.476936276204"
    );
}

#[test]
fn lgamma_twelve_ln_factorial_minus_one_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", lgamma(12))"#),
        "17.502307845874"
    );
}

#[test]
fn erfc_three_tail_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.15e", erfc(3))"#),
        "2.209049700035002e-05"
    );
}

#[test]
fn bessel_j0_at_five_cc() {
    assert_eq!(eval_string(r#"sprintf("%.12f", j0(5))"#), "-0.177596774112");
}

#[test]
fn acosh_two_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", acosh(2))"#),
        "1.31695789692482"
    );
}

#[test]
fn asinh_two_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", asinh(2))"#),
        "1.44363547517881"
    );
}

#[test]
fn copysign_neg_one_follows_negative_zero_rhs_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", copysign(-1, -0.0))"#),
        "-1.00000000000000"
    );
}

#[test]
fn l2_norm_three_four_hypotenuse_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", l2_norm([3, 4]))"#),
        "5.000000000000"
    );
}

#[test]
fn vnorm_negative_box_unit_three_vector_cc() {
    assert_eq!(
        eval_string(r#"stringify(vnorm([-1, -2, -2]))"#),
        r##"(-0.333333333333333, -0.666666666666667, -0.666666666666667)"##
    );
}

#[test]
fn range_of_wide_spread_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", range_of(-3, 5, 11))"#),
        "14.000000000000"
    );
}

#[test]
fn product_three_scalars_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", product(3, 4, 5))"#),
        "60.00000000000000"
    );
}

#[test]
fn factorial_twelve_exact_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", factorial(12))"#),
        "479001600"
    );
}

#[test]
fn fibonacci_fifteenth_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", fibonacci(15))"#),
        "610.00000000000000"
    );
}

#[test]
fn harmonic_partial_sum_century_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", harmonic_number(100))"#),
        "5.18737751763962"
    );
}

#[test]
fn gcd_eighty_four_forty_eight_cc() {
    assert_eq!(eval_int(r#"gcd(84, 48)"#), 12);
}

#[test]
fn lcm_twenty_one_twenty_two_cc() {
    assert_eq!(eval_int(r#"lcm(21, 22)"#), 462);
}

#[test]
fn mod_exp_seven_two_fifty_six_mod_seventeen_cc() {
    assert_eq!(eval_int(r#"mod_exp(7, 256, 17)"#), 1);
}

#[test]
fn sphere_volume_radius_three_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", sphere_volume(3))"#),
        "113.097335529233"
    );
}

#[test]
fn euler_gamma_constant_builtin_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", euler_mascheroni())"#),
        "0.577215664902"
    );
}

#[test]
fn variance_hand_table_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", variance(2, 4, 4, 4, 5, 5, 7, 9))"#),
        "4.00000000000000"
    );
}

#[test]
fn mode_val_duplicated_middle_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", mode_val(1, 2, 2, 3))"#),
        "2.000000"
    );
}

#[test]
fn heron_right_triangle_three_four_five_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", heron(3, 4, 5))"#),
        "6.000000000000"
    );
}

#[test]
fn stirling2_ten_partition_three_nonempty_cc() {
    assert_eq!(eval_int(r#"stirling2(10, 3)"#), 9330);
}

#[test]
fn bell_fifth_integer_cc() {
    assert_eq!(eval_int(r#"bell_number(5)"#), 52);
}

#[test]
fn hypergeometric_series_unit_disk_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", hypergeometric_2f1(1, 2, 3, 0.25))"#),
        "1.205826318457"
    );
}

#[test]
fn legendre_p_cubic_tenth_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", legendre_p(3, 0.1))"#),
        "-0.14750000000000"
    );
}

#[test]
fn chebyshev_u_quadratic_half_trace_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", chebyshev_u(2, 0.5))"#),
        "0.00000000000000"
    );
}

#[test]
fn atan2_thirty_degree_ratio_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", atan2(1, sqrt(3)))"#),
        "0.52359877559830"
    );
}

#[test]
fn complete_elliptic_k_half_parameter_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", elliptic_k(0.5))"#),
        "1.85407467730137"
    );
}

#[test]
fn dirichlet_eta_four_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", dirichlet_eta(4))"#),
        "0.947032829497246"
    );
}

#[test]
fn is_prime_small_ninety_seven_cc() {
    assert_eq!(eval_int(r#"is_prime(97)"#), 1);
}

#[test]
fn euler_totient_factor_twelve_cc() {
    assert_eq!(eval_int(r#"euler_totient(12)"#), 4);
}

#[test]
fn next_prime_after_century_anchor_cc() {
    assert_eq!(eval_int(r#"next_prime(100)"#), 101);
}

#[test]
fn binomial_wide_middle_coef_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", binomial(52, 26))"#),
        "495918532948104"
    );
}

#[test]
fn bessel_j_first_order_cc() {
    assert_eq!(eval_string(r#"sprintf("%.12f", j1(1))"#), "0.440050585677");
}

#[test]
fn spherical_bessel_quad_case_cc() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", spherical_jn(2, 3.14159 / 4))"#),
        "0.039342121500"
    );
}
