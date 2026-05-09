//! Behavior-pinning batch DJ (2026-05): **Prim MST** (**disconnected / zero-weight** pitfalls — **BUG-202**),
//! **interpolation & fits** (`lagrange_interp`, `cubic_spline`, `poly_eval`, `polynomial_fit`),
//! **DSP & spectral** (`convolution`, `cross_correlation`, `goertzel`, `dct`, `energy`, `window_hann`),
//! **elliptic / Jacobi**, **effect size & CI** (`cohen_d`, `confidence_interval`),
//! **combinatorics** (`stirling_approx`, **`double_factorial` / `rising_factorial` / `falling_factorial`**),
//! **recreational** (`look_and_say`, `gray_code_sequence`, `game_of_life_step`, `pascals_triangle`,
//! `roman_numeral_list`), **orthogonal polynomials**, **extended trig / activations**, **geo & physics**
//! (`bearing`, `bmi`, `momentum`), **range helpers** (`map_range`, `inverse_lerp`), **`inverse_erf`**,
//! **`exponential_pdf`**, **`quadratic_discriminant`**.

use crate::common::*;

#[test]
fn prim_mst_triangle_unit_weights_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", prim_mst([[0, 1, 1], [1, 0, 1], [1, 1, 0]]))"#),
        "2"
    );
}

#[test]
fn prim_mst_single_edge_k2_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", prim_mst([[0, 1], [1, 0]]))"#),
        "1"
    );
}

/// Locks **BUG-202**: both vertices isolated under **`w > 0`** rule — returns **`0`**, not non-finite / error.
#[test]
fn prim_mst_disconnected_all_zero_matrix_bug202_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", prim_mst([[0, 0], [0, 0]]))"#),
        "0"
    );
}

/// Locks **BUG-202**: third vertex ignored; total matches **partial** tree on the **`0`‑component only.
#[test]
fn prim_mst_path_plus_isolated_vertex_silent_bug202_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", prim_mst([[0, 1, 0], [1, 0, 0], [0, 0, 0]]))"#),
        "1"
    );
}

#[test]
fn lagrange_interp_quadratic_midpoint_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lagrange_interp([0, 1, 2], [0, 1, 4], 1.5))"#),
        "2.25"
    );
}

#[test]
fn cubic_spline_natural_mid_segment_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cubic_spline([0, 1, 2], [0, 1, 4], 1.5))"#),
        "2.3125"
    );
}

#[test]
fn poly_eval_horner_quadratic_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", poly_eval([1, 2, 3], 2))"#),
        "17"
    );
}

#[test]
fn polynomial_fit_linear_perfect_dj() {
    assert_eq!(
        eval_string(r#"stringify(polynomial_fit([0, 1, 2], [1, 3, 5], 1))"#),
        "(1, 2)"
    );
}

#[test]
fn signal_energy_sum_squares_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", energy([0, 3, 4]))"#),
        "25"
    );
}

#[test]
fn game_of_life_vertical_blinker_to_horizontal_dj() {
    assert_eq!(
        eval_string(
            r#"stringify(game_of_life_step([[0, 1, 0], [0, 1, 0], [0, 1, 0]]))"#
        ),
        "([0, 0, 0], [1, 1, 1], [0, 0, 0])"
    );
}

#[test]
fn legendre_p_third_degree_half_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", legendre_p(3, 0.5))"#),
        "-0.4375"
    );
}

#[test]
fn chebyshev_t_third_degree_half_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chebyshev_t(3, 0.5))"#),
        "-1"
    );
}

#[test]
fn look_and_say_step_one_dj() {
    assert_eq!(eval_string(r#"look_and_say("1")"#), "11");
}

#[test]
fn gray_code_sequence_three_bits_dj() {
    assert_eq!(
        eval_string(r#"stringify(gray_code_sequence(3))"#),
        "(0, 1, 3, 2, 6, 7, 5, 4)"
    );
}

#[test]
fn pascals_triangle_four_rows_dj() {
    assert_eq!(
        eval_string(r#"stringify(pascals_triangle(4))"#),
        "([1], [1, 1], [1, 2, 1], [1, 3, 3, 1])"
    );
}

#[test]
fn roman_numeral_list_three_dj() {
    assert_eq!(
        eval_string(r#"stringify(roman_numeral_list(3))"#),
        r#"("I", "II", "III")"#
    );
}

#[test]
fn extended_trig_cot_sec_csc_sinc_versin_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cot(3.141592653589793/4))"#),
        "1"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", sec(0))"#), "1");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", csc(3.141592653589793/2))"#),
        "1"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", sinc(0))"#), "1");
    assert_eq!(eval_string(r#"sprintf("%.10g", versin(0))"#), "0");
}

#[test]
fn ml_activations_leaky_hard_mish_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", leaky_relu(-2, 0.1))"#),
        "-0.2"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", hard_sigmoid(0))"#), "0.5");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mish(1))"#),
        "0.8650983883"
    );
}

#[test]
fn map_range_inverse_lerp_linear_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", map_range(7.5, 5, 10, 0, 100))"#),
        "50"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", inverse_lerp(0, 10, 7))"#),
        "0.7"
    );
}

#[test]
fn bearing_north_on_meridian_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bearing(0, 0, 0, 1))"#),
        "90"
    );
}

#[test]
fn bmi_weight_kg_height_m_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bmi(70, 1.75))"#),
        "22.85714286"
    );
}

#[test]
fn momentum_mass_times_velocity_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", momentum(2, 3))"#),
        "6"
    );
}

#[test]
fn double_rising_falling_factorial_triple_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", double_factorial(7))"#),
        "105"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", rising_factorial(3, 4))"#),
        "360"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", falling_factorial(10, 3))"#),
        "720"
    );
}

#[test]
fn quadratic_discriminant_monic_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%d", quadratic_discriminant(1, -5, 6))"#),
        "1"
    );
}

#[test]
fn window_hann_length_four_dj() {
    assert_eq!(
        eval_string(r#"stringify(window_hann(4))"#),
        "(0, 0.75, 0.75, 0)"
    );
}

#[test]
fn stirling_approx_factorial_ten_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", stirling_approx(10))"#),
        "3598695.619"
    );
}

#[test]
fn inverse_erf_half_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", inverse_erf(0.5))"#),
        "0.4769362762"
    );
}

#[test]
fn exponential_pdf_rate_one_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", exponential_pdf(0, 1))"#),
        "1"
    );
}

#[test]
fn convolution_auto_correlation_box_two_dj() {
    assert_eq!(
        eval_string(r#"stringify(convolution([1, 1], [1, 1]))"#),
        "(1, 2, 1)"
    );
}

#[test]
fn cross_correlation_triangle_with_ones_dj() {
    assert_eq!(
        eval_string(r#"stringify(cross_correlation([1, 2, 1], [1, 1]))"#),
        "(1, 3, 3, 1)"
    );
}

#[test]
fn goertzel_single_bin_cosine_carrier_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", goertzel([1, 0, -1, 0], 1))"#),
        "0"
    );
}

#[test]
fn jacobi_sn_modular_argument_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jacobi_sn(0.3, 0.7))"#),
        "0.3095201077"
    );
}

#[test]
fn elliptic_k_parameter_half_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", elliptic_k(0.5))"#),
        "1.854074677"
    );
}

#[test]
fn cohen_d_well_separated_triples_dj() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cohen_d([1, 2, 3], [4, 5, 6]))"#),
        "-3"
    );
}

#[test]
fn dct_type2_four_point_cosine_dj() {
    assert_eq!(
        eval_string(r#"stringify(dct([1, 0, -1, 0]))"#),
        "(0, 1.30656296487638, 1.4142135623731, -0.541196100146197)"
    );
}

#[test]
fn confidence_interval_mean_five_uniform_dj() {
    assert_eq!(
        eval_string(
            r#"my @c = confidence_interval([1, 2, 3, 4, 5], 0.95); sprintf("%.10g:%.10g", $c[0], $c[1])"#
        ),
        "1.614070709:4.385929291"
    );
}
