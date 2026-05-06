//! Behavior-pinning batch AA (2026-05-05): Color, Hash, Predicates, Paths, Random.

use crate::common::*;

// ── Color Helpers ───────────────────────────────────────────────────────────

#[test]
fn color_conversions_aa() {
    assert_eq!(eval_string("rgb_to_hex(255, 0, 0)"), "#ff0000");
    assert_eq!(eval_string("rgb_to_hex(0, 255, 0)"), "#00ff00");
    assert_eq!(eval_string("rgb_to_hex(0, 0, 255)"), "#0000ff");
    
    let code = r##"
        my $rgb = hex_to_rgb("#ff0000");
        join(",", @$rgb)
    "##;
    assert_eq!(eval_string(code), "255,0,0");
}

// ── Hash Operations ──────────────────────────────────────────────────────────

#[test]
fn hash_ops_aa() {
    let code = r#"
        my $h = hash_from_pairs("a", 1, "b", 2);
        hash_size($h)
    "#;
    assert_eq!(eval_int(code), 2);
    
    let code2 = r#"
        my $h = { a => 1, b => 2 };
        my @p = pairs_from_hash($h);
        # [[a,1], [b,2]] or [[b,2], [a,1]]
        join(":", sort(map { join(",", @$_) } @p))
    "#;
    assert_eq!(eval_string(code2), "a,1:b,2");
}

// ── Predicates ───────────────────────────────────────────────────────────────

#[test]
fn predicates_aa() {
    assert_eq!(eval_int("is_sorted(1, 2, 3)"), 1);
    assert_eq!(eval_int("is_sorted(1, 3, 2)"), 0);
    
    assert_eq!(eval_int("is_subset([1, 2], [1, 2, 3])"), 1);
    assert_eq!(eval_int("is_subset([1, 4], [1, 2, 3])"), 0);
    
    assert_eq!(eval_int("is_permutation([1, 2, 3], [3, 2, 1])"), 1);
    assert_eq!(eval_int("is_permutation([1, 2, 3], [1, 2, 4])"), 0);
}

// ── Path Helpers ─────────────────────────────────────────────────────────────

#[test]
fn path_helpers_aa() {
    assert_eq!(eval_string(r#"path_ext("test.tar.gz")"#), "gz");
    assert_eq!(eval_string(r#"path_stem("test.tar.gz")"#), "test.tar");
    assert_eq!(eval_string(r#"path_join("a", "b", "c")"#), "a/b/c");
}

// ── Random Helpers ───────────────────────────────────────────────────────────

#[test]
fn random_smoke_aa() {
    // coin_flip returns 0 or 1
    for _ in 0..10 {
        let n = eval_int("coin_flip()");
        assert!(n == 0 || n == 1);
    }
    
    // random_int(lo, hi)
    let n = eval_int("random_int(10, 20)");
    assert!((10..=20).contains(&n));
}

// ── Stats (Extended) ─────────────────────────────────────────────────────────

#[test]
fn stats_aa_smoke() {
    let code = r#"
        my $res = min_max(10, 5, 20, 15);
        join(",", @$res)
    "#;
    assert_eq!(eval_string(code), "5,20");
    
    // harmonic_mean(1, 2, 4) = 3 / (1/1 + 1/2 + 1/4) = 3 / 1.75 = 1.714
    let mean = eval("harmonic_mean(1, 2, 4)").to_number();
    assert!((mean - 1.714).abs() < 0.01);
}
