//! Behavior-pinning batch BN (2026-05-08): interpolation, correlations, quaternion/spline helpers,
//! music theory, combinatorics, trig/remap, SI constants, cumulatives, Levenshtein/base/digits,
//! covariance matrix, inverse hyperbolic funs, ML activations/softmax/norms, roots/logs/bits,
//! percentiles/IQR (second block), moon phase snapshot.

use crate::common::*;

#[test]
fn interpolation_and_correlations_bn() {
    assert_eq!(eval_string(r#"sprintf("%.4f", lerp(10, 20, 0.25))"#), "12.5000");
    assert_eq!(eval_string(r#"sprintf("%.4f", inv_lerp(0, 100, 25))"#), "0.2500");
    assert_eq!(
        eval_string(r#"sprintf("%.6f", inverse_lerp(0, 100, 25))"#),
        "0.250000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.6f", smoothstep(0, 1, 0.5))"#),
        "0.500000"
    );

    assert_eq!(
        eval_string(r#"sprintf("%.4f", covariance([1, 2, 3], [2, 4, 6]))"#),
        "2.0000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.6f", correlation([1, 2, 3], [2, 4, 6]))"#),
        "1.000000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.6f", pearsonr([1, 2, 3], [2, 4, 8]))"#),
        "0.981981"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.6f", spearman_correlation([1, 2, 3], [3, 2, 1]))"#),
        "-1.000000"
    );
    assert_eq!(eval_int("spearmanr([1, 2, 3], [3, 2, 1])"), -1);
    assert_eq!(
        eval_string(r#"sprintf("%.6f", spearman([1, 2, 3], [3, 2, 1]))"#),
        "-1.000000"
    );
}

#[test]
fn quaternion_splines_bn() {
    assert_eq!(
        eval_string(
            r#"my @q = slerp([1, 0, 0, 0], [0, 1, 0, 0], 0.5);
            join(",", map { sprintf("%.6f", $_) } @q)"#
        ),
        "0.707107,0.707107,0.000000,0.000000"
    );

    assert_eq!(
        eval_string(
            r#"my @iq = quat_inv([2, 0, 0, 0]);
            join(",", map { sprintf("%.3f", $_) } @iq)"#
        ),
        "0.500,-0.000,-0.000,-0.000"
    );

    assert_eq!(
        eval_string(
            r#"my @b = bezier_eval([[0, 0], [1, 1], [2, 0]], 0.5);
            join(",", map { sprintf("%.4f", $_) } @b)"#
        ),
        "1.0000,0.5000"
    );

    assert_eq!(
        eval_string(
            r#"my @c = catmull_rom_eval([0, 0], [1, 0], [2, 1], [3, 0], 0.5);
            join(",", map { sprintf("%.4f", $_) } @c)"#
        ),
        "1.5000,0.5625"
    );

    assert_eq!(
        eval_string(r#"sprintf("%.4f", cubic_hermite_eval(0, 1, 1, 1, 0.5))"#),
        "0.5000"
    );

    assert_eq!(
        eval_string(
            r#"my @q = quat_from_matrix([[0, 0, 1], [0, 1, 0], [-1, 0, 0]]);
            join(",", map { sprintf("%.6f", $_) } @q)"#
        ),
        "0.707107,0.000000,0.707107,0.000000"
    );
}

#[test]
fn color_blend_vector_bn() {
    assert_eq!(
        eval_string(r#"join(",", map { int($_) } color_blend_t([255, 0, 0], [0, 0, 255], 0.5))"#),
        "127,0,127"
    );
}

#[test]
fn music_intervals_bn() {
    assert_eq!(
        eval_string(r#"join(",", map { int($_) } chord_to_freqs("C4", "major"))"#),
        "261,329,391"
    );

    assert_eq!(
        eval_string(r#"join(",", scale_to_intervals("major"))"#),
        "0,2,4,5,7,9,11,12"
    );

    assert_eq!(eval_int(r#"interval_semitones("M3")"#), 3);
    assert_eq!(eval_int(r#"transpose_semi(440, 12)"#), 880);
    assert_eq!(eval_string(r#"sprintf("%.4f", bpm_to_period(120))"#), "0.5000");
    assert_eq!(eval_int("midi_to_pitch_class(60)"), 0);
    assert_eq!(eval_int(r#"key_signature_for("D")"#), 2);
    assert_eq!(eval_string(r#"circle_of_fifths_step("C", 1)"#), "G");
}

#[test]
fn combinatorics_sequences_bn() {
    assert_eq!(eval_int("ackermann_limited(3, 2)"), 29);
    assert_eq!(eval_int("fibonacci(12)"), 144);
    assert_eq!(eval_int("lucas(8)"), 47);
    assert_eq!(eval_int("catalan(4)"), 14);
    assert_eq!(eval_int("stirling_second(5, 2)"), 15);
    assert_eq!(
        eval_string(r#"stringify(bernoulli_number(2))"#),
        "0.166666666666667"
    );
    assert_eq!(eval_int("bell_number(5)"), 52);
    assert_eq!(eval_int("partition_number(6)"), 11);
}

#[test]
fn trig_remap_rotate_and_small_stats_bn() {
    assert_eq!(eval_string(r#"sprintf("%.10f", d2r(180))"#), "3.1415926536");
    assert_eq!(eval_string(r#"sprintf("%.2f", r2d(3.141592653589793))"#), "180.00");
    assert_eq!(eval_string(r#"sprintf("%.1f", angle_between_deg(1, 0))"#), "0.0");
    assert_eq!(eval_string(r#"sprintf("%.1f", angle_between_deg(0, 1))"#), "90.0");

    assert_eq!(eval_string(r#"sprintf("%.1f", remap(5, 0, 10, 100, 200))"#), "150.0");
    assert_eq!(eval_int("copysign(3, -2)"), -3);

    assert_eq!(
        eval_string(r#"sprintf("%.15f", atan2(1, 1))"#),
        "0.785398163397448"
    );

    assert_eq!(
        eval_string(r#"sprintf("%.6f", pearson_skewness_2([1, 2, 3, 4, 5]))"#),
        "0.000000"
    );

    assert_eq!(
        eval_string(r#"stringify(rotate_point(1, 0, 90))"#),
        "[6.12323399573677e-17, 1]"
    );

    assert_eq!(eval_string(r#"sprintf("%.2f", moon_phase(2451545))"#), "0.83");
}

#[test]
fn physical_constants_bn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", speed_of_light())"#), "299792458");
    assert_eq!(eval_string(r#"stringify(avogadro())"#), "6.02214076e+23");
    assert_eq!(eval_string(r#"stringify(planck())"#), "6.62607015e-34");
    assert_eq!(eval_string(r#"stringify(boltzmann())"#), "1.380649e-23");
    assert_eq!(
        eval_string(r#"stringify(elementary_charge())"#),
        "1.602176634e-19"
    );
}

#[test]
fn cumulatives_and_adjacent_bn() {
    assert_eq!(
        eval_string(r#"join(",", cumsum(1, 2, 3, 4))"#),
        "1,3,6,10"
    );
    assert_eq!(eval_string(r#"join(",", cumprod(2, 3, 4))"#), "2,6,24");
    assert_eq!(
        eval_string(r#"join(",", adjacent_difference(1, 4, 9, 16))"#),
        "3,5,7"
    );
}

#[test]
fn string_distance_radix_digits_bn() {
    assert_eq!(eval_int(r#"levenshtein("kitten", "sitting")"#), 3);
    assert_eq!(eval_int(r#"from_base('FF', 16)"#), 255);
    assert_eq!(eval_string(r#"to_base(255, 16)"#), "ff");
    assert_eq!(
        eval_string(r#"join(",", digits_of(12345))"#),
        "1,2,3,4,5"
    );
}

#[test]
fn erf_hyperbolic_cov_matrix_bn() {
    assert_eq!(eval_string(r#"sprintf("%.6f", erf(1))"#), "0.842701");
    assert_eq!(eval_string(r#"sprintf("%.6f", erfc(0))"#), "1.000000");

    assert_eq!(eval_string(r#"sprintf("%.6f", acosh(1))"#), "0.000000");
    assert_eq!(eval_string(r#"sprintf("%.6f", asinh(1))"#), "0.881374");
    assert_eq!(
        eval_string(r#"sprintf("%.6f", atanh(0.5))"#),
        "0.549306"
    );

    assert_eq!(
        eval_string(r#"stringify(covariance_matrix_pts([[1, 2], [3, 4], [5, 6]]))"#),
        "((4, 4), (4, 4))"
    );
}

#[test]
fn ml_activations_softmax_norms_bn() {
    assert_eq!(
        eval_string(r#"stringify(softmax(1, 2, 3))"#),
        "(0.0900305731703805, 0.244728471054798, 0.665240955774822)"
    );

    assert_eq!(eval_string(r#"sprintf("%.6f", sigmoid(0))"#), "0.500000");
    assert_eq!(
        eval_string(r#"sprintf("%.6f", sigmoid(2))"#),
        "0.880797"
    );

    assert_eq!(eval_string(r#"sprintf("%.6f", tanh(1))"#), "0.761594");
    assert_eq!(eval_string(r#"sprintf("%.6f", gelu(0))"#), "0.000000");

    assert_eq!(
        eval_string(r#"stringify(vec_normalize([3, 4]))"#),
        "[0.6, 0.8]"
    );
    assert_eq!(eval_int("l2_norm([3, 4])"), 5);
    assert_eq!(eval_int("l1_norm([1, -2, 3])"), 6);

    assert_eq!(eval_int("relu(-3)"), 0);
    assert_eq!(eval_int("relu(7)"), 7);
}

#[test]
fn roots_logs_bits_percentiles_bn() {
    assert_eq!(eval_int("cbrt(27)"), 3);
    assert_eq!(
        eval_string(r#"sprintf("%.6f", exp2(4))"#),
        "16.000000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10f", log2(1024))"#),
        "10.0000000000"
    );

    assert_eq!(eval_int("popcount(7)"), 3);
    assert_eq!(eval_int("is_power_of_two(64)"), 1);
    assert_eq!(eval_int("is_power_of_two(63)"), 0);

    assert_eq!(eval_int("percentile(50, 1, 2, 3, 4, 5)"), 3);
    assert_eq!(eval_int("median(1, 2, 3, 4, 5)"), 3);
    assert_eq!(eval_int("iqr(1, 2, 3, 100)"), 98);
}

#[test]
fn bitwise_shifts_bn() {
    assert_eq!(eval_int("bit_and(12, 10)"), 8);
    assert_eq!(eval_int("bit_xor(12, 10)"), 6);
    assert_eq!(eval_int("bit_or(12, 10)"), 14);
    assert_eq!(eval_int("shl(1, 8)"), 256);
    assert_eq!(eval_int("shr(256, 8)"), 1);
}
