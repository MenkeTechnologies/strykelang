//! Probabilistic-data-structure correctness pins. Sketches are
//! world-first as stryke stdlib primitives, so the math has to
//! actually behave on every refactor. These pins bound the error
//! distributions instead of asserting exact values (which is what
//! sketches deliberately give up for memory savings).

use crate::common::*;

// ── HyperLogLog: ±2% relative error on 100k distinct inputs ──────────
//
// Theoretical guarantee for precision=14 (2^14 registers) is
// ~1.04/sqrt(2^14) ≈ 0.81% standard error. Pin at 2% — comfortable
// upper bound; a regression to >2% indicates real breakage.

#[test]
fn hll_estimates_100k_within_two_percent() {
    let code = r#"
        my $h = hll(14);
        for my $i (1:100_000) { hll_add($h, "key:$i") }
        my $est = hll_count($h);
        my $rel = abs($est - 100_000) / 100_000;
        $rel < 0.02 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_union_estimate_within_two_percent_of_truth() {
    // Two disjoint streams of 50k each → union should estimate 100k.
    let code = r#"
        my $a = hll(14);
        my $b = hll(14);
        for my $i (1:50_000)       { hll_add($a, "k:$i") }
        for my $i (50_001:100_000) { hll_add($b, "k:$i") }
        my $u = $a + $b;
        my $est = hll_count($u);
        my $rel = abs($est - 100_000) / 100_000;
        $rel < 0.02 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_small_set_uses_linear_counting() {
    // Linear-counting fallback should be near-exact on small loads.
    let code = r#"
        my $h = hll(14);
        for my $i (1:50) { hll_add($h, "k:$i") }
        my $est = hll_count($h);
        # Tolerate up to 5% relative error at small N — linear counting
        # has its own floor on register-spread variance.
        my $rel = abs($est - 50) / 50;
        $rel < 0.05 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_handles_duplicate_inserts_idempotently() {
    // Same key inserted 1000× should still count as ONE distinct.
    let code = r#"
        my $h = hll(14);
        for (1:1000) { hll_add($h, "always-the-same-key") }
        my $est = hll_count($h);
        ($est >= 1 && $est <= 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Bloom filter: no false negatives + ≤ target FPR ──────────────────
//
// Bloom's defining property: zero false negatives. Inserted members
// MUST always test positive. False-positive rate should track the
// requested FPR within an order of magnitude.

#[test]
fn bloom_no_false_negatives_on_inserted_keys() {
    let code = r#"
        my $b = bloom_filter(10_000, 0.001);
        for my $i (1:1000) { bloom_add($b, "user:$i") }
        my $missed = 0;
        for my $i (1:1000) {
            $missed++ unless bloom_contains($b, "user:$i");
        }
        $missed == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bloom_false_positive_rate_within_target_at_designed_load() {
    // bloom_filter(10_000, 0.01) sized for 10k items at 1% FPR. Test
    // with 1k unrelated probes; FPR should be well under 5% (10× the
    // design target — generous regression bound).
    let code = r#"
        my $b = bloom_filter(10_000, 0.01);
        for my $i (1:1000) { bloom_add($b, "user:$i") }
        my $fp = 0;
        for my $i (1_000_001:1_001_000) {
            $fp++ if bloom_contains($b, "user:$i");
        }
        ($fp / 1000.0) < 0.05 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bloom_union_preserves_no_false_negatives() {
    let code = r#"
        my $a = bloom_filter(1000, 0.01);
        my $b = bloom_filter(1000, 0.01);
        bloom_add($a, $_) for ("alice", "bob", "carol");
        bloom_add($b, $_) for ("dave", "eve", "frank");
        my $u = $a + $b;
        my $missed = 0;
        for my $name ("alice", "bob", "carol", "dave", "eve", "frank") {
            $missed++ unless bloom_contains($u, $name);
        }
        $missed == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── CMS: never under-counts ──────────────────────────────────────────
//
// Count-Min Sketch can over-count due to hash collisions but never
// under-counts. Pin this invariant.

#[test]
fn cms_estimate_at_least_truth() {
    let code = r#"
        my $c = cms(2048, 5);
        cms_add($c, "hot") for (1:1000);
        cms_add($c, "warm") for (1:100);
        cms_add($c, "cold") for (1:10);
        (cms_count($c, "hot")  >= 1000
            && cms_count($c, "warm") >= 100
            && cms_count($c, "cold") >= 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cms_merge_is_pointwise_sum() {
    // Two CMS sketches summed → total count for shared key ≥ sum of
    // individuals (only over by collision noise, never under).
    let code = r#"
        my $a = cms(2048, 5);
        my $b = cms(2048, 5);
        cms_add($a, "x") for (1:100);
        cms_add($b, "x") for (1:200);
        my $u = $a + $b;
        cms_count($u, "x") >= 300 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cms_unknown_key_returns_low_or_zero() {
    let code = r#"
        my $c = cms(2048, 5);
        cms_add($c, "real") for (1:1000);
        # Width=2048, depth=5 → very low collision probability for an
        # unrelated key. Allow up to ~20 noise hits as the upper bound.
        cms_count($c, "never_inserted") < 20 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── TopK: SpaceSaving heavy hitters survive ───────────────────────────

#[test]
fn topk_keeps_the_dominant_hitter_against_noise() {
    // SpaceSaving's guarantee: any item with true count > total/k IS in
    // the top-k. For k=3 with 1000 alpha, 900 beta, 800 gamma, 200
    // singleton-noise items (total = 2900): threshold = 2900/3 ≈ 967.
    // Only alpha (1000) is strictly above the threshold so it's the
    // only guaranteed survivor. Pin that; don't over-claim beta/gamma
    // which are at the edge and can be displaced by noise churn.
    let code = r#"
        my $t = topk(3);
        topk_add($t, "alpha") for (1:1000);
        topk_add($t, "beta")  for (1:900);
        topk_add($t, "gamma") for (1:800);
        topk_add($t, "noise:" . $_) for (1:200);
        my @h = topk_heavies($t);
        my %seen = map { $_->[0] => 1 } @h;
        $seen{alpha} ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn topk_keeps_clear_winners_when_no_displacement_pressure() {
    // No noise → top-k is exact. Each true hitter is well above
    // threshold and can't be displaced.
    let code = r#"
        my $t = topk(3);
        topk_add($t, "alpha") for (1:1000);
        topk_add($t, "beta")  for (1:1000);
        topk_add($t, "gamma") for (1:1000);
        my @h = topk_heavies($t);
        my %seen = map { $_->[0] => 1 } @h;
        ($seen{alpha} && $seen{beta} && $seen{gamma}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn topk_merge_keeps_globally_heavy_keys() {
    let code = r#"
        my $a = topk(3);
        my $b = topk(3);
        topk_add($a, "alpha") for (1:500);
        topk_add($b, "alpha") for (1:400);
        topk_add($a, "beta")  for (1:200);
        topk_add($b, "gamma") for (1:600);
        my $u = $a + $b;
        my @h = topk_heavies($u);
        my %seen = map { $_->[0] => 1 } @h;
        ($seen{alpha} && $seen{gamma}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── t-digest: quantile accuracy on a uniform stream ──────────────────

#[test]
fn tdigest_median_within_one_percent_of_truth() {
    let code = r#"
        my $td = t_digest(100);
        td_add($td, $_) for (1:10_000);
        my $med = td_quantile($td, 0.5);
        # True median of 1:10000 is 5000.5
        (abs($med - 5000.5) / 5000.5) < 0.01 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tdigest_p99_accurate_at_tail() {
    let code = r#"
        my $td = t_digest(100);
        td_add($td, $_) for (1:10_000);
        my $p99 = td_quantile($td, 0.99);
        # True p99 = 9900
        (abs($p99 - 9900) / 9900) < 0.02 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tdigest_merge_preserves_quantiles() {
    let code = r#"
        my $a = t_digest(100);
        my $b = t_digest(100);
        td_add($a, $_) for (1:5000);
        td_add($b, $_) for (5001:10000);
        my $u = $a + $b;
        my $med = td_quantile($u, 0.5);
        (abs($med - 5000.5) / 5000.5) < 0.02 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Roaring: exact (not probabilistic) — but pin the contract ────────

#[test]
fn roaring_membership_is_exact() {
    let code = r#"
        my $rb = roaring();
        rb_add($rb, $_) for (1:1000);
        my $hit = bloom_filter(10, 0.01);   # noop — for the side effect
        my $miss = 0;
        $miss++ unless rb_contains($rb, 1);
        $miss++ unless rb_contains($rb, 500);
        $miss++ unless rb_contains($rb, 1000);
        $miss++ if rb_contains($rb, 1001);
        $miss == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn roaring_set_algebra_exact_counts() {
    let code = r#"
        my $a = roaring();
        my $b = roaring();
        rb_add($a, $_) for (1:10);
        rb_add($b, $_) for (5:15);
        (rb_len($a | $b) == 15
            && rb_len($a & $b) == 6
            && rb_len($a ^ $b) == 9
            && rb_len($a - $b) == 4) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
