//! Behavior-pinning batch S (2026-05-05): AI, Number Theory, Statistics, Geometry, Financial.
//!
//! This batch pins the behavior of the massive influx of specialized built-ins
//! added to Stryke in the v0.1.x series. These tests ensure that as the JIT
//! and VM evolve, the mathematical and domain-specific correctness is preserved.

use crate::common::*;

// ── AI Primitives (Mocked) ──────────────────────────────────────────────────

#[test]
fn ai_prompt_with_mock_returns_expected_response() {
    // ai_mock_install uses regex for matching prompts.
    let s = eval_string(
        r#"ai_mock_install("what is 2\+2", "2+2 is 4");
           ai("what is 2+2")"#,
    );
    assert_eq!(s, "2+2 is 4");
}

#[test]
fn ai_extract_with_mock_and_schema() {
    // schema => +{...} forces ai_extract. The mock should still work.
    // Parentheses are needed to ensure the whole call is treated as the RHS
    // in scalar assignment context.
    let s = eval_string(
        r#"ai_mock_install("extract user", '{"name":"Alice","age":30}');
           my $r = ai("extract user", schema => +{ name => "string", age => "int" });
           $r->{name} . ":" . $r->{age}"#,
    );
    assert_eq!(s, "Alice:30");
}

#[test]
fn ai_vision_with_mock_image_path() {
    let s = eval_string(
        r#"ai_mock_install("describe", "a red apple");
           ai("describe", image => "/tmp/test.jpg")"#,
    );
    assert_eq!(s, "a red apple");
}

#[test]
fn ai_cost_reports_zero_for_mocked_calls() {
    let c = eval_int(
        r#"ai_mock_install("cost test", "foo");
           ai("cost test");
           ai_cost()"#,
    );
    assert_eq!(c, 0);
}

// ── Number Theory ───────────────────────────────────────────────────────────

#[test]
fn prime_factors_basic() {
    assert_eq!(eval_string(r#"join ",", prime_factors(28)"#), "2,2,7");
    assert_eq!(eval_string(r#"join ",", prime_factors(13)"#), "13");
}

#[test]
fn is_perfect_numbers() {
    assert_eq!(eval_int("is_perfect(6)"), 1);
    assert_eq!(eval_int("is_perfect(28)"), 1);
    assert_eq!(eval_int("is_perfect(10)"), 0);
}

#[test]
fn divisors_list() {
    assert_eq!(eval_string(r#"join ",", sort { $a <=> $b } divisors(12)"#), "1,2,3,4,6,12");
}

#[test]
fn collatz_sequence_length() {
    // 13 -> 40 -> 20 -> 10 -> 5 -> 16 -> 8 -> 4 -> 2 -> 1 (length 10, steps 9)
    // Stryke's collatz_length returns number of steps.
    assert_eq!(eval_int("collatz_length(13)"), 9);
}

#[test]
fn nth_prime_lookup() {
    assert_eq!(eval_int("nth_prime(1)"), 2);
    assert_eq!(eval_int("nth_prime(10)"), 29);
}

#[test]
fn primes_up_to_sieve() {
    assert_eq!(eval_string(r#"join ",", primes_up_to(20)"#), "2,3,5,7,11,13,17,19");
}

#[test]
fn triangular_number_formula() {
    // T_n = n(n+1)/2; T_5 = 5*6/2 = 15
    assert_eq!(eval_int("triangular_number(5)"), 15);
}

#[test]
fn lucas_sequence() {
    // L_0=2, L_1=1, L_2=3, L_3=4, L_4=7, L_5=11
    assert_eq!(eval_int("lucas(5)"), 11);
}

#[test]
fn tribonacci_sequence() {
    // T_0=0, T_1=0, T_2=1, T_3=1, T_4=2, T_5=4, T_6=7
    assert_eq!(eval_int("tribonacci(6)"), 7);
}

#[test]
fn next_prev_prime() {
    assert_eq!(eval_int("next_prime(14)"), 17);
    assert_eq!(eval_int("prev_prime(13)"), 11);
}

// ── Statistics ──────────────────────────────────────────────────────────────

#[test]
fn statistics_basic_ops() {
    let code = r#"
        my @data = (1, 2, 3, 4, 5);
        my $m = mean(@data);
        my $v = variance(@data);
        my $s = stddev(@data);
        "$m,$v,$s"
    "#;
    let out = eval_string(code);
    let parts: Vec<&str> = out.split(',').collect();
    assert_eq!(parts[0], "3");
    assert!((parts[1].parse::<f64>().unwrap() - 2.0).abs() < 1e-9);
    assert!((parts[2].parse::<f64>().unwrap() - 2.0f64.sqrt()).abs() < 1e-9);
}

#[test]
fn median_odd_even() {
    assert_eq!(eval_int("median(1, 3, 2)"), 2);
    assert_eq!(eval_string("median(1, 2, 3, 4)"), "2.5");
}

#[test]
fn mode_single_and_multi() {
    assert_eq!(eval_int("mode(1, 2, 2, 3)"), 2);
    assert_eq!(eval_string(r#"join ",", sort { $a <=> $b } mode(1, 1, 2, 2, 3)"#), "1,2");
}

#[test]
fn stats_skewness_kurtosis() {
    // For a symmetric distribution like (1, 2, 3), skewness should be 0.
    assert_eq!(eval_int("skewness(1, 2, 3)"), 0);
}

#[test]
fn stats_z_score() {
    // z = (x - mean) / sd. For (20, 15, 5), z = (20-15)/5 = 1.
    assert_eq!(eval_int("z_score(20, 15, 5)"), 1);
    // 0.5 case
    assert_eq!(eval_string("z_score(20, 10, 20)"), "0.5");
}

// ── Algorithms & Text ───────────────────────────────────────────────────────

#[test]
fn levenshtein_distance() {
    assert_eq!(eval_int(r#"levenshtein("kitten", "sitting")"#), 3);
    assert_eq!(eval_int(r#"levenshtein("flaw", "lawn")"#), 2);
}

#[test]
fn soundex_encoding() {
    assert_eq!(eval_string(r#"soundex("Robert")"#), "R163");
    assert_eq!(eval_string(r#"soundex("Rupert")"#), "R163");
    // Pin current behavior (which has a known limitation regarding h/w separators).
    assert_eq!(eval_string(r#"soundex("Ashcraft")"#), "A226");
}

#[test]
fn luhn_check_validation() {
    assert_eq!(eval_int("luhn_check(79927398713)"), 1);
    assert_eq!(eval_int("luhn_check(79927398710)"), 0);
}

#[test]
fn ordinalize_numbers() {
    assert_eq!(eval_string("ordinalize(1)"), "1st");
    assert_eq!(eval_string("ordinalize(2)"), "2nd");
    assert_eq!(eval_string("ordinalize(3)"), "3rd");
    assert_eq!(eval_string("ordinalize(4)"), "4th");
    assert_eq!(eval_string("ordinalize(11)"), "11th");
    assert_eq!(eval_string("ordinalize(21)"), "21st");
}

// ── Geometry ────────────────────────────────────────────────────────────────

#[test]
fn geometry_area_circle() {
    let a = eval_string("area_circle(10)");
    let val = a.parse::<f64>().unwrap();
    assert!((val - 314.1592653589793).abs() < 1e-9);
}

#[test]
fn point_distance_2d() {
    // distance between (0,0) and (3,4) is 5
    assert_eq!(eval_int("point_distance(0, 0, 3, 4)"), 5);
}

#[test]
fn haversine_distance_earth() {
    // distance between London (51.5, -0.1) and Paris (48.8, 2.3) is approx 340km
    let d = eval_int("haversine_distance(51.5, -0.1, 48.8, 2.3)");
    assert!(d > 300 && d < 400);
}

// ── Encoding ────────────────────────────────────────────────────────────────

#[test]
fn base64_roundtrip() {
    let s = eval_string(
        r#"my $encoded = base64_encode("hello world");
           base64_decode($encoded)"#,
    );
    assert_eq!(s, "hello world");
}

#[test]
fn sha256_hex_output() {
    // sha256 of empty string
    let s = eval_string(r#"sha256("")"#);
    assert_eq!(s, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}

#[test]
fn url_encode_decode() {
    assert_eq!(eval_string(r#"url_encode("a b/c")"#), "a%20b%2Fc");
    assert_eq!(eval_string(r#"url_decode("a%20b%2Fc")"#), "a b/c");
}

// ── Financial ───────────────────────────────────────────────────────────────

#[test]
fn financial_pmt_calculation() {
    // PMT(rate, nper, pv)
    // PMT(0.05/12, 10*12, 10000)
    let s = eval_string("pmt(0.05/12, 120, 10000)");
    let val = s.parse::<f64>().unwrap();
    // expected approx -106.06
    assert!((val + 106.0655).abs() < 0.01);
}

#[test]
fn financial_future_value_calculation() {
    // future_value(pv, r, n) in Stryke
    // future_value(1000, 0.05, 2) -> 1000 * (1.05)^2 = 1102.5
    let fv = eval_string("future_value(1000, 0.05, 2)");
    assert_eq!(fv, "1102.5");
}

// ── Misc & System ──────────────────────────────────────────────────────────

#[test]
fn system_env_access() {
    assert_eq!(eval_string(r#"$ENV{STRYKE_NONEXISTENT} // "missing""#), "missing");
}

#[test]
fn cyberpunk_banner_runs() {
    // confirm it doesn't crash and returns some blocky art
    let s = eval_string(r#"cyber_banner("TEST")"#);
    assert!(s.contains("█") || s.contains("▀") || s.contains("▄"));
}

#[test]
fn boxplot_svg_runs() {
    let s = eval_string(r#"boxplot_svg(1, 2, 3, 4, 5)"#);
    assert!(s.contains("<svg"));
}
