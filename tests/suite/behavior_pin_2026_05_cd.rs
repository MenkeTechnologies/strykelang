//! Behavior-pinning batch CD (2026-05-09): DSP / numeric integration stubs and helpers — windows (**`hann_*`/`hamming_*`/`blackman*`/`kaiser_*`**),
//! **`sinc`**, smoothing (**`moving_average`**, **`exponential_moving_average`**, **`fir_moving_average`**), correlations (**`cross_correlation`**, **`autocorrelation`**,
//! **`fft_magnitude`**), **`zero_crossings`**, quadrature primitives (**`trapz`**, **`simpson`**, **`cumtrapz`**, composite **`simpson_rule`**),
//! naive conv-length stubs (**`convolve_*`/`correlate_full`/`kron_product`** — BUG-129), **`wiener_filter`**, **`detrend_linear`** (BUG-130),
//! **`peak_widths_at`**, geometry **`area_trapezoid`**, resampling (**`downsample`**, **`upsample`**), **`medfilt_1d`** (BUG-131), **`numerical_diff`**,
//! plus audio/physics stubs (**`spectral_centroid`**, **`spectral_density_estimate`**, **`planck_spectral_radiance`**, **`mfcc_coeff_step`**, **`goertzel`**, **`dct`**,
//! **`tukey_window`**, **`savgol_coef`**).

use crate::common::*;

#[test]
fn hann_window_quintet_symmetric_cd() {
    assert_eq!(
        eval_string(r#"stringify(hann_window(5))"#),
        "(0, 0.5, 1, 0.5, 0)"
    );
}

#[test]
fn sinc_at_origin_one_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", sinc(0))"#),
        "1.00000000000000"
    );
}

#[test]
fn sinc_near_pi_small_residue_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", sinc(3.14159))"#),
        "0.00000084466458"
    );
}

#[test]
fn hamming_window_seventh_string_cd() {
    assert_eq!(
        eval_string(r#"stringify(hamming_window(7))"#),
        "(0.08, 0.31, 0.77, 1, 0.77, 0.31, 0.08)"
    );
}

#[test]
fn blackman_window_three_taps_cd() {
    assert_eq!(
        eval_string(r#"stringify(blackman_window(3))"#),
        "(-1.38777878078145e-17, 1, -1.38777878078145e-17)"
    );
}

#[test]
fn blackman_harris_fifth_normalized_cd() {
    assert_eq!(
        eval_string(r#"stringify(blackman_harris_window(5))"#),
        "(6.0000000000001e-05, 0.21747, 1, 0.21747, 6.0000000000001e-05)"
    );
}

#[test]
fn kaiser_window_odd_length_cd() {
    assert_eq!(
        eval_string(r#"stringify(kaiser_window(5, 8.6))"#),
        "(0.00133251399790242, 0.340393622440189, 1, 0.340393622440189, 0.00133251399790242)"
    );
}

#[test]
fn moving_average_width_three_two_outputs_cd() {
    assert_eq!(
        eval_string(r#"stringify(moving_average(3, 2, 3, 4, 11))"#),
        "(3, 6)"
    );
}

#[test]
fn exponential_smoothing_half_alpha_cd() {
    assert_eq!(
        eval_string(r#"stringify(exponential_moving_average(0.5, [100, 110, 115]))"#),
        "(100, 105, 110)"
    );
}

#[test]
fn fir_moving_average_causal_smooth_cd() {
    assert_eq!(
        eval_string(r#"stringify(fir_moving_average([1, 2, 3, 10], 3))"#),
        "(1, 1.5, 2, 5)"
    );
}

// BUG-129 — length-only stubs, not algebraic convolution tensors.
#[test]
fn convolve_full_reports_output_length_minus_one_stub_cd() {
    assert_eq!(eval_int(r#"convolve_full([1, 2, 3], [0, 10])"#), 4);
}

#[test]
fn convolve_valid_reports_overlap_extent_stub_cd() {
    assert_eq!(eval_int(r#"convolve_valid([1, 2, 3, 4], [1, 2])"#), 3);
}

#[test]
fn correlate_full_same_impl_as_conv_stub_cd() {
    assert_eq!(eval_int(r#"correlate_full([1], [9, 10])"#), 2);
}

#[test]
fn kron_product_cardinality_multiplier_stub_cd() {
    assert_eq!(eval_int(r#"kron_product([1, 3], [10, 100])"#), 4);
}

#[test]
fn cross_correlation_sliding_sumdefinition_cd() {
    assert_eq!(
        eval_string(r#"stringify(cross_correlation([1, -1], [1]))"#),
        "(1, -1)"
    );
}

#[test]
fn simpson_rule_uniform_three_bins_quadratic_mass_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", simpson_rule([0, 4, 16]))"#),
        "10.66666666666667"
    );
}

#[test]
fn trapz_default_spacing_three_samples_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", trapz([0, 1, 4]))"#),
        "3.00000000000000"
    );
}

#[test]
fn simpson_three_points_polynomial_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", simpson([0, 1, 4]))"#),
        "2.66666666666667"
    );
}

#[test]
fn cumtrapz_quadratic_sampling_cd() {
    assert_eq!(
        eval_string(r#"stringify(cumtrapz([1, 2, 4]))"#),
        "(0, 1.5, 4.5)"
    );
}

#[test]
fn numerical_diff_three_point_spacing_two_cd() {
    assert_eq!(
        eval_string(r#"stringify(numerical_diff([0, 1, 9], 2))"#),
        "(0.5, 2.25, 4)"
    );
}

#[test]
fn autocorrelation_alternate_sign_pattern_cd() {
    assert_eq!(
        eval_string(r#"stringify(autocorrelation(1, -1, 1, -1))"#),
        "(1, -0.75, 0.5, -0.25)"
    );
}

#[test]
fn fft_magnitude_four_point_square_carrier_cd() {
    assert_eq!(
        eval_string(r#"stringify(fft_magnitude(1, 0, -1, 0))"#),
        "(0, 2, 2.44929359829471e-16)"
    );
}

#[test]
fn zero_crossings_three_flips_cd() {
    assert_eq!(eval_int(r#"zero_crossings(1, -1, 1, -1, -1)"#), 3);
}

#[test]
fn wiener_filter_signal_dominates_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", wiener_filter(40, 10))"#),
        "0.80000000000000"
    );
}

// BUG-130 — returns regression slope scalar, **not** a detrended sample vector.
#[test]
fn detrend_linear_pure_ramp_slope_one_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", detrend_linear([0, 1, 2]))"#),
        "1.00000000000000"
    );
}

// BUG-131 — global median after sorting entire series; not a sliding (**2k+1**) filter.
#[test]
fn medfilt_one_d_global_sorted_median_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", medfilt_1d([10, 100, 40, 110, 42]))"#),
        "55.00000000000000"
    );
}

#[test]
fn peak_width_at_half_via_slopes_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", peak_widths_at(12, 2, 3))"#),
        "5.00000000000000"
    );
}

#[test]
fn planar_trapezoid_area_parallel_sides_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", area_trapezoid(10, 12, 22))"#),
        "242.00000000000000"
    );
}

#[test]
fn downsample_by_two_keeps_odd_indices_cd() {
    assert_eq!(
        eval_string(r#"stringify(downsample([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 2))"#),
        "(1, 3, 5, 7, 9)"
    );
}

#[test]
fn upsample_inserts_zeros_factor_three_cd() {
    assert_eq!(
        eval_string(r#"stringify(upsample([1, 2], 3))"#),
        "(1, 0, 0, 2, 0, 0)"
    );
}

#[test]
fn hamming_distance_int_three_bit_flips_cd() {
    assert_eq!(eval_int(r#"hamming_distance_int(7, 4)"#), 2);
}

#[test]
fn hamming_weight_popcnt_nibble_cd() {
    assert_eq!(eval_int(r#"hamming_weight(0b1011)"#), 3);
}

#[test]
fn spectral_centroid_four_tap_carrier_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", spectral_centroid([0, 1, 0, -1]))"#),
        "11025.000000000000"
    );
}

#[test]
fn spectral_density_estimate_degenerate_energy_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", spectral_density_estimate([1, 0, -1]))"#),
        "0.000000000000"
    );
}

#[test]
fn planck_radiance_visible_band_hot_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.12e", planck_spectral_radiance(500e-9, 3000))"#),
        "2.602683395541e+11"
    );
}

#[test]
fn mfcc_hyper_params_forward_reference_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", mfcc_coeff_step(7, 256, 128, 0))"#),
        "7.00000000000000"
    );
}

#[test]
fn goertzel_frequency_bin_one_four_samples_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", goertzel([1, 0, -1, 0], 1, 4))"#),
        "2.000000000000"
    );
}

#[test]
fn dct_type_two_half_cycle_cd() {
    assert_eq!(
        eval_string(r#"stringify(dct([1.0, 2.0]))"#),
        "(3, -0.707106781186547)"
    );
}

#[test]
fn tukey_window_eighth_smooth_quarter_frac_cd() {
    assert_eq!(
        eval_string(r#"stringify(tukey_window(8, 0.25))"#),
        "(0, 1, 1, 1, 1, 1, 1, 0)"
    );
}

#[test]
fn savgol_quadratic_smooth_scalar_gain_cd() {
    assert_eq!(
        eval_string(r#"sprintf("%.17f", savgol_coef(5, 2))"#),
        "0.00878738672468148"
    );
}
