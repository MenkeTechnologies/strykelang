//! Sketch operations beyond `sketch_correctness_pin.rs`. Cover:
//!   * Sketch algebra: `+` (union), `&` (intersection), `-` (diff)
//!   * Large-N HLL accuracy
//!   * t-digest extreme quantiles (0.01, 0.99, 1.0)
//!   * CMS over heavy-hitter workloads
//!   * Roaring set operations
//!   * Bloom union via `+`

use crate::common::*;

// ── HLL union via `+` ──────────────────────────────────────────────

#[test]
fn hll_union_via_plus_operator_distinct_sets() {
    let code = r#"
        my $a = hll(14);
        my $b = hll(14);
        hll_add($a, "user_$_") for (1:1000);
        hll_add($b, "user_$_") for (500:1500);
        # Union has 1500 distinct users (1..1500).
        my $u = $a + $b;
        my $est = hll_count($u);
        ($est >= 1400 && $est <= 1600) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_union_associative() {
    let code = r#"
        my $a = hll(14);
        my $b = hll(14);
        my $c = hll(14);
        hll_add($a, "x_$_") for (1:300);
        hll_add($b, "x_$_") for (200:500);
        hll_add($c, "x_$_") for (400:700);
        my $way_1 = ($a + $b) + $c;
        my $way_2 = $a + ($b + $c);
        # Both should be roughly equal (HLL union is associative).
        my $diff = abs(hll_count($way_1) - hll_count($way_2));
        $diff < 50 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── HLL at large cardinality ───────────────────────────────────────

#[test]
fn hll_accuracy_at_10k_distinct() {
    let code = r#"
        my $h = hll(14);
        hll_add($h, "u_$_") for (1:10000);
        my $est = hll_count($h);
        # Allow 2% error at this precision.
        ($est >= 9800 && $est <= 10200) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_distinct_count_zero_for_empty() {
    let code = r#"
        my $h = hll(14);
        hll_count($h) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_does_not_double_count_duplicates() {
    let code = r#"
        my $h = hll(14);
        for my $i (1:1000) {
            hll_add($h, "user-42");   # all the same value
        }
        my $est = hll_count($h);
        ($est >= 1 && $est <= 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Bloom union via `+` ────────────────────────────────────────────

#[test]
fn bloom_union_via_plus_operator() {
    let code = r#"
        my $a = bloom_filter(1000, 0.01);
        my $b = bloom_filter(1000, 0.01);
        bloom_add($a, "alice");
        bloom_add($a, "bob");
        bloom_add($b, "carol");
        bloom_add($b, "dave");
        my $u = $a + $b;
        (bloom_contains($u, "alice")
            && bloom_contains($u, "bob")
            && bloom_contains($u, "carol")
            && bloom_contains($u, "dave")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bloom_no_false_negatives_on_added_items() {
    let code = r#"
        my $b = bloom_filter(10000, 0.001);
        bloom_add($b, "item_$_") for (1:5000);
        my $missed = 0;
        for my $i (1:5000) {
            $missed++ unless bloom_contains($b, "item_$i");
        }
        $missed == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── CMS heavy-hitters ──────────────────────────────────────────────

#[test]
fn cms_count_at_least_actual_count() {
    let code = r#"
        my $c = cms(2048, 5);
        for my $i (1:1000) {
            cms_add($c, "hot");
        }
        for my $i (1:100) {
            cms_add($c, "warm");
        }
        # CMS is a count-min sketch: estimate >= actual always.
        my $hot  = cms_count($c, "hot");
        my $warm = cms_count($c, "warm");
        ($hot >= 1000 && $warm >= 100) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cms_returns_zero_for_unseen() {
    let code = r#"
        my $c = cms(2048, 5);
        cms_add($c, "seen");
        # Unseen keys typically return 0 (some CMS variants slack a bit).
        my $unseen = cms_count($c, "never_seen");
        ($unseen >= 0 && $unseen <= 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── TopK heavy-hitters ─────────────────────────────────────────────

#[test]
fn topk_identifies_clear_winners() {
    let code = r#"
        my $tk = topk(3);
        topk_add($tk, "winner") for (1:1000);
        topk_add($tk, "second") for (1:500);
        topk_add($tk, "third") for (1:200);
        topk_add($tk, "noise_$_") for (1:50);
        my @top = topk_top($tk);
        # Top is "winner" with high count.
        ($top[0]->[0] eq "winner" && $top[0]->[1] >= 900) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── t-digest extreme quantiles ─────────────────────────────────────

#[test]
fn t_digest_median_of_uniform_distribution() {
    let code = r#"
        my $td = t_digest(100);
        for my $i (1:1000) {
            td_add($td, $i);
        }
        # Median of 1..1000 is ~500.5.
        my $p50 = td_quantile($td, 0.5);
        ($p50 >= 450 && $p50 <= 550) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn t_digest_p99_near_top() {
    let code = r#"
        my $td = t_digest(100);
        for my $i (1:1000) {
            td_add($td, $i);
        }
        my $p99 = td_quantile($td, 0.99);
        ($p99 >= 950 && $p99 <= 1000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn t_digest_p01_near_bottom() {
    let code = r#"
        my $td = t_digest(100);
        for my $i (1:1000) {
            td_add($td, $i);
        }
        my $p01 = td_quantile($td, 0.01);
        ($p01 >= 1 && $p01 <= 50) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn t_digest_handles_negative_values() {
    let code = r#"
        my $td = t_digest(100);
        for my $i (-500:500) {
            td_add($td, $i);
        }
        my $median = td_quantile($td, 0.5);
        (abs($median) < 50) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Roaring bitmap set algebra ─────────────────────────────────────
// Real API: `roaring()` constructor, `rb_add($r, $x)`, `rb_len($r)`,
// operators: `|` union, `&` intersection, `^` sym-diff, `-` andnot.

#[test]
fn roaring_union_via_pipe() {
    let code = r#"
        my $a = roaring();
        my $b = roaring();
        rb_add($a, $_) for (1:100);
        rb_add($b, $_) for (50:150);
        my $u = $a | $b;
        rb_len($u) == 150 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn roaring_intersection_via_amp() {
    let code = r#"
        my $a = roaring();
        my $b = roaring();
        rb_add($a, $_) for (1:100);
        rb_add($b, $_) for (50:150);
        my $i = $a & $b;
        # Intersection 50..100 = 51 elements.
        rb_len($i) == 51 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn roaring_andnot_via_minus() {
    let code = r#"
        my $a = roaring();
        my $b = roaring();
        rb_add($a, $_) for (1:100);
        rb_add($b, $_) for (50:150);
        my $d = $a - $b;
        # 1..49 = 49 elements.
        rb_len($d) == 49 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn roaring_symmetric_diff_via_xor() {
    let code = r#"
        my $a = roaring();
        my $b = roaring();
        rb_add($a, $_) for (1:100);
        rb_add($b, $_) for (50:150);
        my $sd = $a ^ $b;
        # 1..49 + 101..150 = 49 + 50 = 99 elements.
        rb_len($sd) == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sketch persistence across calls ────────────────────────────────

#[test]
fn hll_state_persists_across_function_calls() {
    let code = r#"
        mysync $h = hll(14);
        fn Demo::Sk::record($x) { hll_add($h, $x) }
        Demo::Sk::record("a");
        Demo::Sk::record("b");
        Demo::Sk::record("c");
        Demo::Sk::record("a");   # duplicate
        # Expect ~3 distinct.
        my $est = hll_count($h);
        ($est >= 2 && $est <= 4) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── HLL + Bloom + TopK combined ────────────────────────────────────

#[test]
fn combined_sketch_workflow_correctness() {
    let code = r#"
        my $hll = hll(14);
        my $bloom = bloom_filter(10000, 0.01);
        my $tk = topk(5);
        for my $i (1:500) {
            my $key = "item_" . ($i % 100);   # 100 distinct, 5x each
            hll_add($hll, $key);
            bloom_add($bloom, $key);
            topk_add($tk, $key);
        }
        my $distinct = hll_count($hll);
        my @top = topk_top($tk);
        # 100 distinct, all in bloom, top 5 each ~5 hits.
        ($distinct >= 95 && $distinct <= 105
            && bloom_contains($bloom, "item_42")
            && len(@top) == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Roaring cardinality on empty ───────────────────────────────────

#[test]
fn roaring_empty_cardinality_zero() {
    let code = r#"
        my $r = roaring();
        rb_len($r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn roaring_add_increases_cardinality() {
    let code = r#"
        my $r = roaring();
        rb_add($r, 1);
        rb_add($r, 2);
        rb_add($r, 3);
        rb_len($r) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── HLL precision sweep ────────────────────────────────────────────

#[test]
fn hll_precision_10_works() {
    let code = r#"
        my $h = hll(10);
        hll_add($h, "u_$_") for (1:1000);
        my $est = hll_count($h);
        # Lower precision = wider error bound, ~5%.
        ($est >= 900 && $est <= 1100) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_precision_16_more_accurate() {
    let code = r#"
        my $h = hll(16);
        hll_add($h, "u_$_") for (1:1000);
        my $est = hll_count($h);
        # 0.4% error → 996..1004.
        ($est >= 990 && $est <= 1010) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Bloom false-positive rate empirically ──────────────────────────

#[test]
fn bloom_false_positive_rate_below_target() {
    let code = r#"
        my $b = bloom_filter(10000, 0.01);
        bloom_add($b, "actual_$_") for (1:5000);
        my $fps = 0;
        for my $i (1:5000) {
            $fps++ if bloom_contains($b, "phantom_$i");
        }
        # Target FPR 1%, allow some slack at the ratio between
        # capacity (10000) and load (5000).
        my $fpr = $fps / 5000.0;
        $fpr < 0.05 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
