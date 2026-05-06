//! Behavior-pinning batch V (2026-05-05): Cyberpunk, Advanced Math, Statistics, DSP, Geometry.

use crate::common::*;

// ── Cyberpunk Terminal Art ──────────────────────────────────────────────────

#[test]
fn cyber_glitch_distorts_text() {
    let s = eval_string(r#"cyber_glitch("HELLO", 5)"#);
    // Should contain original characters and some ANSI escapes or corruption
    assert!(s.contains("H") || s.contains("E") || s.contains("L") || s.contains("O"));
    assert!(s.contains("\x1b["));
}

#[test]
fn cyber_circuit_generates_traces() {
    let s = eval_string("cyber_circuit(40, 10, 42)");
    assert!(s.contains("\x1b["));
    // Circuit characters: ┌ ┐ └ ┘ ─ │ ┼ ═ ║ ⊙
    assert!(s.contains("─") || s.contains("│") || s.contains("┼") || s.contains("⊙"));
}

#[test]
fn cyber_skull_small_and_large() {
    let s1 = eval_string("cyber_skull()");
    let s2 = eval_string(r#"cyber_skull("large")"#);
    assert!(s1.contains("█"));
    assert!(s2.len() > s1.len());
}

#[test]
fn cyber_eye_motif() {
    let s = eval_string("cyber_eye()");
    assert!(s.contains("\x1b["));
    assert!(s.contains("█") || s.contains("▄") || s.contains("▀"));
}

// ── Advanced Number Theory ──────────────────────────────────────────────────

#[test]
fn number_theory_euler_totient() {
    // phi(10) = 4 (1, 3, 7, 9)
    assert_eq!(eval_int("euler_totient(10)"), 4);
    // phi(13) = 12
    assert_eq!(eval_int("euler_totient(13)"), 12);
}

#[test]
fn number_theory_divisor_sums() {
    // sum_divisors(6) = 1 + 2 + 3 = 6
    assert_eq!(eval_int("sum_divisors(6)"), 6);
    // sum_divisors(12) = 1 + 2 + 3 + 4 + 6 = 16
    assert_eq!(eval_int("sum_divisors(12)"), 16);
}

#[test]
fn number_theory_abundant_deficient() {
    assert_eq!(eval_int("is_abundant(12)"), 1); // 16 > 12
    assert_eq!(eval_int("is_deficient(10)"), 1); // 1 + 2 + 5 = 8 < 10
}

#[test]
fn number_theory_smith_numbers() {
    // 4: 4 = 2+2, is_prime(4)=false, smith(4)=true
    assert_eq!(eval_int("is_smith(4)"), 1);
    assert_eq!(eval_int("is_smith(22)"), 1); // 2+2=4, factors [2, 11], 2 + (1+1) = 4
}

#[test]
fn number_theory_partition_and_bell() {
    assert_eq!(eval_int("partition_number(5)"), 7);
    assert_eq!(eval_int("bell_number(3)"), 5);
}

#[test]
fn number_theory_prime_pi_and_totient_sum() {
    assert_eq!(eval_int("prime_pi(10)"), 4); // 2, 3, 5, 7
                                             // totient_sum(3) = phi(1) + phi(2) + phi(3) = 1 + 1 + 2 = 4
    assert_eq!(eval_int("totient_sum(3)"), 4);
}

#[test]
fn number_theory_goldbach_conjecture() {
    // 10 = 3 + 7
    let code = r#"
        my @res = goldbach(10);
        join(",", @res)
    "#;
    assert_eq!(eval_string(code), "3,7");
}

// ── Advanced Statistics ─────────────────────────────────────────────────────

#[test]
fn stats_linear_regression() {
    let code = r#"
        my @res = linear_regression([1, 2, 3], [2, 4, 6]);
        join(",", @res)
    "#;
    // slope=2, intercept=0, r2=1
    assert_eq!(eval_string(code), "2,0,1");
}

#[test]
fn stats_moving_averages() {
    let code = r#"
        my @data = (1, 2, 3, 4, 5);
        my @ma = moving_average(3, @data);
        join(",", @ma)
    "#;
    // (1+2+3)/3 = 2, (2+3+4)/3 = 3, (3+4+5)/3 = 4
    assert_eq!(eval_string(code), "2,3,4");
}

#[test]
fn stats_quartiles_and_mad() {
    let code = r#"
        my @data = (1, 2, 3, 4, 5, 6, 7, 8);
        my @q = quartiles(@data);
        my $mad = median_absolute_deviation(@data);
        join(",", @q) . ":" . $mad
    "#;
    // q1=3, q2=5, q3=7; mad=2
    assert_eq!(eval_string(code), "3,5,7:2");
}

#[test]
fn stats_z_scores_list() {
    let code = r#"
        my @res = z_scores(10, 20, 30);
        # mean=20, stddev=sqrt((100+0+100)/3) = sqrt(66.66) = 8.16
        # z = (-10/8.16, 0, 10/8.16) = (-1.22, 0, 1.22)
        # Check signs and middle zero
        join(",", map { int($_) } @res)
    "#;
    // -1,0,1 (approx)
    assert_eq!(eval_string(code), "-1,0,1");
}

// ── DSP / Signal Processing ─────────────────────────────────────────────────

#[test]
fn dsp_convolution_impulse() {
    let code = r#"
        my @res = convolution([1, 2, 3], [1, 0, 0]);
        join(",", @res)
    "#;
    assert_eq!(eval_string(code), "1,2,3,0,0");
}

#[test]
fn dsp_fft_magnitude_sine() {
    let code = r#"
        # Constant signal should have peak at bin 0
        my @m = fft_magnitude(1, 1, 1, 1);
        # re = 4, im = 0 -> mag = 4
        $m[0]
    "#;
    assert_eq!(eval_int(code), 4);
}

#[test]
fn dsp_peak_detect_simple() {
    let code = r#"
        my @peaks = peak_detect(1, 5, 2, 8, 3);
        join(",", @peaks)
    "#;
    // index 1 (value 5), index 3 (value 8)
    assert_eq!(eval_string(code), "1,3");
}

#[test]
fn dsp_zero_crossings_count() {
    let code = r#"zero_crossings(1, -1, 1, -1, 1)"#;
    assert_eq!(eval_int(code), 4);
}

// ── Geometry ────────────────────────────────────────────────────────────────

#[test]
fn geometry_polygon_area_square() {
    // 2x2 square
    let code = r#"polygon_area([[0,0], [2,0], [2,2], [0,2]])"#;
    assert_eq!(eval_int(code), 4);
}

#[test]
fn geometry_point_in_polygon_test() {
    let code = r#"
        my $poly = [[0,0], [10,0], [10,10], [0,10]];
        my $in = point_in_polygon(5, 5, $poly);
        my $out = point_in_polygon(15, 5, $poly);
        "$in,$out"
    "#;
    assert_eq!(eval_string(code), "1,0");
}

#[test]
fn geometry_polygon_perimeter_square() {
    // Takes arrayref of arrayrefs
    let code = r#"polygon_perimeter([[0,0], [2,0], [2,2], [0,2]])"#;
    assert_eq!(eval_int(code), 8);
}

#[test]
fn geometry_convex_hull_graham() {
    let code = r#"
        my @hull = convex_hull([[0,0], [2,0], [2,2], [0,2], [1,1]]);
        # Hull should be the 4 corners, excluding the interior point [1,1]
        len(@hull)
    "#;
    assert_eq!(eval_int(code), 4);
}

// ── Color Conversion ────────────────────────────────────────────────────────

#[test]
fn color_hsl_rgb_roundtrip() {
    let code = r#"
        my @rgb = hsl_to_rgb(0, 1, 0.5); # Pure Red
        my @hsl = rgb_to_hsl(@rgb);
        "$rgb[0],$rgb[1],$rgb[2]:" . int($hsl[0]) . "," . int($hsl[1]) . "," . $hsl[2]
    "#;
    assert_eq!(eval_string(code), "255,0,0:0,1,0.5");
}

#[test]
fn color_hsv_to_rgb_blue() {
    let code = r#"
        my @rgb = hsv_to_rgb(240, 1, 1); # Pure Blue
        join(",", @rgb)
    "#;
    assert_eq!(eval_string(code), "0,0,255");
}

// ── Algorithms ──────────────────────────────────────────────────────────────

#[test]
fn algorithm_balanced_parens() {
    assert_eq!(eval_int(r#"is_balanced_parens("([{}])")"#), 1);
    assert_eq!(eval_int(r#"is_balanced_parens("([)]")"#), 0);
}

#[test]
fn algorithm_next_permutation() {
    let code = r#"
        my @p = next_permutation(1, 2, 3);
        join(",", @p)
    "#;
    assert_eq!(eval_string(code), "1,3,2");
}

// ── Validation ──────────────────────────────────────────────────────────────

#[test]
fn validation_formats() {
    assert_eq!(eval_int(r#"is_valid_email('test@example.com')"#), 1);
    assert_eq!(eval_int(r#"is_valid_email('invalid')"#), 0);
    assert_eq!(eval_int(r#"is_valid_ipv4("127.0.0.1")"#), 1);
    assert_eq!(eval_int(r#"is_valid_ipv4("256.0.0.1")"#), 0);
}

#[test]
fn validation_mime_and_cidr() {
    assert_eq!(eval_int(r#"is_valid_mime("application/json")"#), 1);
    assert_eq!(eval_int(r#"is_valid_cidr("10.0.0.0/8")"#), 1);
}
