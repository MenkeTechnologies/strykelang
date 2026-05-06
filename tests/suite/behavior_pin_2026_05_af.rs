//! Behavior-pinning batch AF (2026-05-05): Math Predicates, Advanced Stats.

use crate::common::*;

// ── Math Predicates ──────────────────────────────────────────────────────────

#[test]
fn math_predicates_af() {
    assert_eq!(eval_int("is_pow2(16)"), 1);
    assert_eq!(eval_int("is_pow2(15)"), 0);
    assert_eq!(eval_int("is_square(100)"), 1);
    assert_eq!(eval_int("is_square(99)"), 0);
    
    // cbrt(8) = 2
    assert_eq!(eval_int("cbrt(8)"), 2);
    // exp2(3) = 8
    assert_eq!(eval_int("exp2(3)"), 8);
}

// ── Percentage & Inverse ─────────────────────────────────────────────────────

#[test]
fn math_af_misc() {
    // percent(part, total)
    assert_eq!(eval_int("percent(25, 200)"), 12); // truncates to 12.5? Let's check
    
    let p = eval("percent(25, 200)").to_number();
    assert!((p - 12.5).abs() < 0.001);
    
    // inverse(2) = 0.5
    assert_eq!(eval_string("inverse(2)"), "0.5");
}

// ── Advanced Stats ───────────────────────────────────────────────────────────

#[test]
fn stats_af_advanced() {
    // median
    assert_eq!(eval_string("median(1, 2, 3, 100)"), "2.5");
    assert_eq!(eval_int("median(1, 10, 5)"), 5);
    
    // mode_val
    assert_eq!(eval_int("mode_val(1, 2, 2, 3)"), 2);
    
    // variance
    let v = eval("variance(1, 2, 3)").to_number();
    assert!((v - 0.666).abs() < 0.01);
}

// ── More Collection Aggregates ───────────────────────────────────────────────

#[test]
fn collection_af_extra() {
    // range_of was in Z, but let's check multi-arg behavior if any
    
    // quantile
    // (quantile is usually same as percentile in some libs)
}
