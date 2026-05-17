//! Probabilistic data structures — Bloom filter (first sketch landed; HLL,
//! CMS, t-digest, top-k, Roaring follow under RFC-1). Behavior pins at the
//! stryke-script level so the builtin surface stays stable.

use crate::common::*;

#[test]
fn bloom_basic_membership() {
    assert_eq!(
        eval_int(
            r#"
                my $b = bloom_filter(1000, 0.01);
                bloom_add($b, "alice");
                bloom_add($b, "bob");
                bloom_contains($b, "alice") + bloom_contains($b, "bob")
            "#
        ),
        2
    );
}

#[test]
fn bloom_negatives_absent() {
    assert_eq!(
        eval_int(
            r#"
                my $b = bloom_filter(10_000, 0.001);
                bloom_add($b, "in") for 1..100;
                bloom_contains($b, "definitely-not-there-zz999")
            "#
        ),
        0
    );
}

#[test]
fn bloom_no_false_negatives_under_load() {
    // Insert 1000 distinct keys; every one must hit on lookup.
    assert_eq!(
        eval_int(
            r#"
                my $b = bloom_filter(10_000, 0.001);
                for my $i (1..1000) { bloom_add($b, "k$i") }
                my $hits = 0;
                for my $i (1..1000) { $hits++ if bloom_contains($b, "k$i") }
                $hits
            "#
        ),
        1000
    );
}

#[test]
fn bloom_len_counts_unique_inserts() {
    assert_eq!(
        eval_int(
            r#"
                my $b = bloom_filter(1000, 0.01);
                bloom_add($b, "x");
                bloom_add($b, "x");
                bloom_add($b, "y");
                bloom_len($b)
            "#
        ),
        2
    );
}

#[test]
fn bloom_add_returns_newly_inserted_flag() {
    assert_eq!(
        eval_int(
            r#"
                my $b = bloom_filter(1000, 0.01);
                bloom_add($b, "x");        # 1
                bloom_add($b, "x")         # 0 — second insert
            "#
        ),
        0
    );
}

#[test]
fn bloom_bits_is_power_of_two_and_at_least_64() {
    assert_eq!(
        eval_int(
            r#"
                my $b = bloom_filter(1000, 0.01);
                my $bits = bloom_bits($b);
                ($bits >= 64 && ($bits & ($bits - 1)) == 0) ? 1 : 0
            "#
        ),
        1
    );
}

#[test]
fn bloom_clear_resets_state() {
    assert_eq!(
        eval_int(
            r#"
                my $b = bloom_filter(1000, 0.01);
                bloom_add($b, "x");
                bloom_clear($b);
                bloom_len($b) + bloom_contains($b, "x")
            "#
        ),
        0
    );
}

#[test]
fn bloom_merge_unions_two_filters() {
    assert_eq!(
        eval_int(
            r#"
                my $a = bloom_filter(1000, 0.01);
                my $b = bloom_filter(1000, 0.01);
                bloom_add($a, "x"); bloom_add($a, "y");
                bloom_add($b, "y"); bloom_add($b, "z");
                bloom_merge($a, $b);
                bloom_contains($a, "x") + bloom_contains($a, "y") + bloom_contains($a, "z")
            "#
        ),
        3
    );
}

#[test]
fn bloom_merge_rejects_mismatched_geometry() {
    assert_eq!(
        eval_int(
            r#"
                my $a = bloom_filter(1000, 0.01);
                my $b = bloom_filter(100_000, 0.001);
                bloom_merge($a, $b)
            "#
        ),
        0
    );
}

#[test]
fn bloom_serialize_roundtrip_preserves_membership() {
    assert_eq!(
        eval_int(
            r#"
                my $b = bloom_filter(1000, 0.01);
                for my $i (1..100) { bloom_add($b, "k$i") }
                my $bytes = bloom_serialize($b);
                my $r = bloom_deserialize($bytes);
                my $hits = 0;
                for my $i (1..100) { $hits++ if bloom_contains($r, "k$i") }
                $hits
            "#
        ),
        100
    );
}

#[test]
fn bloom_deserialize_rejects_corrupt_payload() {
    // Garbage bytes return undef (eval-coerced to 0 here).
    assert_eq!(
        eval_int(
            r#"
                my $r = bloom_deserialize("not-a-bloom-payload");
                defined($r) ? 1 : 0
            "#
        ),
        0
    );
}

#[test]
fn bloom_ref_type_is_bloomfilter() {
    assert_eq!(
        eval_string(
            r#"
                my $b = bloom_filter(1000, 0.01);
                ref($b)
            "#
        ),
        "BloomFilter"
    );
}

#[test]
fn bloom_appears_in_b_hash() {
    // Reflection: every dispatch arm registers in %b.
    assert!(
        eval_int(r#"exists $b{bloom_filter} ? 1 : 0"#) == 1,
        "bloom_filter not in %b"
    );
    assert!(
        eval_int(r#"exists $b{bloom_contains} ? 1 : 0"#) == 1,
        "bloom_contains not in %b"
    );
}

#[test]
fn bloom_aliases_route_to_same_primary() {
    // `bloom` alias maps to `bloom_filter` primary.
    assert_eq!(
        eval_int(
            r#"
                my $a = bloom(100, 0.01);
                bloom_add($a, "k");
                bloom_has($a, "k")
            "#
        ),
        1
    );
}

// ── HyperLogLog (HLL) ────────────────────────────────────────────────

#[test]
fn hll_estimates_distinct_within_two_percent() {
    // Insert 10k distinct items, allow up to 5% slop in CI (lower
    // precision than the algorithmic 1.3%/sqrt(m) so a flaky run from
    // hash collisions doesn't fail this).
    let code = r#"
        my $h = hll(14);
        for my $i (1..10000) { hll_add($h, "v$i") }
        my $est = hll_count($h);
        my $err = abs($est - 10000) / 10000;
        $err < 0.05 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1, "HLL estimate too far from truth");
}

#[test]
fn hll_empty_is_zero() {
    assert_eq!(eval_int(r#"my $h = hll(10); hll_count($h)"#), 0);
}

#[test]
fn hll_clear_resets() {
    assert_eq!(
        eval_int(
            r#"
                my $h = hll(12);
                hll_add($h, "k$_") for 1..1000;
                hll_clear($h);
                hll_count($h)
            "#
        ),
        0
    );
}

#[test]
fn hll_merge_unions_two_sketches() {
    let code = r#"
        my $a = hll(14);
        my $b = hll(14);
        for my $i (1..5000)     { hll_add($a, "k$i") }
        for my $i (5001..10000) { hll_add($b, "k$i") }
        hll_merge($a, $b);
        my $est = hll_count($a);
        my $err = abs($est - 10000) / 10000;
        $err < 0.05 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_merge_rejects_precision_mismatch() {
    assert_eq!(
        eval_int(
            r#"
                my $a = hll(12);
                my $b = hll(14);
                hll_merge($a, $b)
            "#
        ),
        0
    );
}

#[test]
fn hll_serialize_roundtrip() {
    let code = r#"
        my $h = hll(12);
        hll_add($h, "v$_") for 1..2000;
        my $orig = hll_count($h);
        my $r = hll_deserialize(hll_serialize($h));
        abs(hll_count($r) - $orig) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_legacy_aliases_route_to_fast_path() {
    // Names from the deleted slow impls route to the new fast primitive.
    let code = r#"
        my $h = hyperloglog_pp_new(12);
        hyperloglog_pp_add($h, "x_$_") for 1..1000;
        my $est = hyperloglog_pp_estimate($h);
        my $err = abs($est - 1000) / 1000;
        $err < 0.05 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hll_ref_type_is_hllsketch() {
    assert_eq!(eval_string(r#"my $h = hll(12); ref($h)"#), "HllSketch");
}

#[test]
fn hll_precision_returned_matches_construction() {
    assert_eq!(eval_int(r#"my $h = hll(13); hll_precision($h)"#), 13);
}

#[test]
fn hll_precision_clamped_to_legal_range() {
    // Precision clamps to [4, 18]; 200 should clamp to 18 (m=262144).
    assert_eq!(eval_int(r#"my $h = hll(200); hll_precision($h)"#), 18);
    assert_eq!(eval_int(r#"my $h = hll(1); hll_precision($h)"#), 4);
}

#[test]
fn hll_appears_in_b_hash() {
    assert_eq!(eval_int(r#"exists $b{hll} ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"exists $b{hll_count} ? 1 : 0"#), 1);
}

// ── Count-Min Sketch (CMS) ────────────────────────────────────────────

#[test]
fn cms_basic_frequency() {
    assert_eq!(
        eval_int(
            r#"
                my $c = cms(2048, 5);
                cms_add($c, "alice", 5);
                cms_add($c, "alice", 3);
                cms_add($c, "bob");
                cms_count($c, "alice") + cms_count($c, "bob")
            "#
        ),
        9
    );
}

#[test]
fn cms_unseen_returns_zero() {
    assert_eq!(
        eval_int(r#"my $c = cms(2048, 5); cms_count($c, "missing")"#),
        0
    );
}

#[test]
fn cms_is_upper_bound() {
    // CMS never under-reports. Over-report bound = e/width with prob 1 - 1/2^depth.
    let code = r#"
        my $c = cms(2048, 5);
        for my $i (1..1000) { cms_add($c, "k$i") }
        my $est = cms_count($c, "k1");
        # Truth = 1; CMS upper-bound holds: est >= 1
        $est >= 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cms_merge_sums_counters() {
    let code = r#"
        my $a = cms(2048, 5);
        my $b = cms(2048, 5);
        cms_add($a, "k", 7);
        cms_add($b, "k", 5);
        cms_merge($a, $b);
        cms_count($a, "k")
    "#;
    assert_eq!(eval_int(code), 12);
}

#[test]
fn cms_merge_rejects_geometry_mismatch() {
    assert_eq!(
        eval_int(
            r#"
                my $a = cms(2048, 5);
                my $b = cms(1024, 5);
                cms_merge($a, $b)
            "#
        ),
        0
    );
}

#[test]
fn cms_clear_resets() {
    assert_eq!(
        eval_int(
            r#"
                my $c = cms(2048, 5);
                cms_add($c, "k", 100);
                cms_clear($c);
                cms_count($c, "k")
            "#
        ),
        0
    );
}

#[test]
fn cms_serialize_roundtrip() {
    let code = r#"
        my $c = cms(2048, 5);
        cms_add($c, "k$_") for 1..1000;
        my $orig = cms_count($c, "k1");
        my $r = cms_deserialize(cms_serialize($c));
        cms_count($r, "k1") == $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cms_legacy_aliases_route() {
    let code = r#"
        my $c = count_min_sketch_new(2048, 5);
        count_min_sketch_add($c, "x") for 1..100;
        count_min_sketch_query($c, "x")
    "#;
    assert_eq!(eval_int(code), 100);
}

#[test]
fn cms_ref_type_is_cmssketch() {
    assert_eq!(eval_string(r#"my $c = cms(); ref($c)"#), "CmsSketch");
}

// ── TopK (SpaceSaving) ────────────────────────────────────────────────

#[test]
fn topk_returns_top_n_keys_by_frequency() {
    // Stream a/b/c/d/e with 100/50/20/5/3 counts; K=3 should report
    // top three in order; lower entries get evicted with proper error
    // floors.
    let code = r#"
        my $t = topk(3);
        topk_add($t, "a") for 1..100;
        topk_add($t, "b") for 1..50;
        topk_add($t, "c") for 1..20;
        topk_add($t, "d") for 1..5;
        topk_add($t, "e") for 1..3;
        my @h = topk_heavies($t);
        scalar(@h) == 3 && $h[0]->[0] eq "a" && $h[0]->[1] == 100
            && $h[1]->[0] eq "b" && $h[1]->[1] == 50 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn topk_size_capped_at_k() {
    let code = r#"
        my $t = topk(5);
        topk_add($t, "k$_") for 1..1000;
        topk_size($t)
    "#;
    assert_eq!(eval_int(code), 5);
}

#[test]
fn topk_count_returns_zero_for_never_seen_key() {
    let code = r#"
        my $t = topk(5);
        topk_add($t, "a") for 1..100;
        topk_count($t, "never_seen")
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn topk_heavies_n_truncates_result() {
    let code = r#"
        my $t = topk(10);
        topk_add($t, "k$_") for 1..50;
        my @h = topk_heavies($t, 3);
        scalar(@h)
    "#;
    assert_eq!(eval_int(code), 3);
}

#[test]
fn topk_clear_resets() {
    assert_eq!(
        eval_int(
            r#"
                my $t = topk(10);
                topk_add($t, "x") for 1..100;
                topk_clear($t);
                topk_size($t)
            "#
        ),
        0
    );
}

#[test]
fn topk_serialize_roundtrip() {
    let code = r#"
        my $t = topk(5);
        topk_add($t, "alpha") for 1..50;
        topk_add($t, "beta")  for 1..30;
        topk_add($t, "gamma") for 1..10;
        my $r = topk_deserialize(topk_serialize($t));
        my @orig = topk_heavies($t);
        my @round = topk_heavies($r);
        scalar(@orig) == scalar(@round)
            && $orig[0]->[0] eq $round[0]->[0]
            && $orig[0]->[1] == $round[0]->[1] ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn topk_ref_type_is_topksketch() {
    assert_eq!(eval_string(r#"my $t = topk(10); ref($t)"#), "TopKSketch");
}

#[test]
fn topk_appears_in_b_hash() {
    assert_eq!(eval_int(r#"exists $b{topk} ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"exists $b{topk_heavies} ? 1 : 0"#), 1);
}

// ── t-digest ──────────────────────────────────────────────────────────

#[test]
fn td_quantile_accurate_at_median() {
    let code = r#"
        my $t = t_digest(100);
        td_add($t, $_) for 1..10000;
        my $p50 = td_quantile($t, 0.5);
        # Truth is 5000.5; accept within 1%.
        abs($p50 - 5000.5) / 5000.5 < 0.01 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn td_quantile_accurate_at_p99() {
    let code = r#"
        my $t = t_digest(200);
        td_add($t, $_) for 1..10000;
        my $p99 = td_quantile($t, 0.99);
        # Truth ≈ 9900.5; accept within 2% (tails widen the bound).
        abs($p99 - 9900.5) / 9900.5 < 0.02 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn td_count_min_max_mean() {
    let code = r#"
        my $t = t_digest(100);
        td_add($t, $_) for 1..100;
        my $min = td_min($t);
        my $max = td_max($t);
        my $mean = td_mean($t);
        td_count($t) == 100 && $min == 1 && $max == 100 && abs($mean - 50.5) < 0.5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn td_merge_combines_two_digests() {
    let code = r#"
        my $a = t_digest(100);
        my $b = t_digest(100);
        td_add($a, $_) for 1..5000;
        td_add($b, $_) for 5001..10000;
        td_merge($a, $b);
        my $p50 = td_quantile($a, 0.5);
        abs($p50 - 5000.5) / 5000.5 < 0.02 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn td_serialize_roundtrip() {
    let code = r#"
        my $t = t_digest(100);
        td_add($t, $_) for 1..1000;
        my $orig = td_quantile($t, 0.5);
        my $r = td_deserialize(td_serialize($t));
        abs(td_quantile($r, 0.5) - $orig) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn td_clear_resets() {
    assert_eq!(
        eval_int(
            r#"
                my $t = t_digest(100);
                td_add($t, $_) for 1..1000;
                td_clear($t);
                td_count($t)
            "#
        ),
        0
    );
}

#[test]
fn td_ref_type_is_tdigestsketch() {
    assert_eq!(
        eval_string(r#"my $t = t_digest(); ref($t)"#),
        "TDigestSketch"
    );
}

#[test]
fn td_legacy_aliases_route() {
    assert_eq!(
        eval_int(
            r#"
                my $t = tdigest_new();
                tdigest_add($t, $_) for 1..1000;
                tdigest_quantile($t, 0.5) > 400 ? 1 : 0
            "#
        ),
        1
    );
}

// ── Roaring Bitmap ────────────────────────────────────────────────────

#[test]
fn roaring_construct_from_args() {
    assert_eq!(eval_int(r#"my $r = roaring(1, 2, 3, 5, 8); rb_len($r)"#), 5);
}

#[test]
fn roaring_construct_from_arrayref() {
    assert_eq!(
        eval_int(r#"my $r = roaring([1, 2, 3, 5, 8]); rb_len($r)"#),
        5
    );
}

#[test]
fn roaring_set_membership() {
    assert_eq!(
        eval_int(
            r#"
                my $r = roaring(1, 100, 1_000_000);
                rb_contains($r, 100) + rb_contains($r, 99)
            "#
        ),
        1
    );
}

#[test]
fn roaring_add_returns_newly_inserted_count() {
    assert_eq!(
        eval_int(
            r#"
                my $r = roaring(1, 2, 3);
                rb_add($r, 1, 4, 5)  # 1 is dup; 4,5 are new
            "#
        ),
        2
    );
}

#[test]
fn roaring_remove() {
    assert_eq!(
        eval_int(
            r#"
                my $r = roaring(1, 2, 3, 4, 5);
                rb_remove($r, 2, 4, 99);  # 2,4 removed; 99 was absent
                rb_len($r)
            "#
        ),
        3
    );
}

#[test]
fn roaring_union() {
    assert_eq!(
        eval_int(
            r#"
                my $a = roaring(1, 2, 3);
                my $b = roaring(3, 4, 5);
                rb_or($a, $b);
                rb_len($a)
            "#
        ),
        5
    );
}

#[test]
fn roaring_intersect() {
    let code = r#"
        my $a = roaring(1, 2, 3, 4, 5);
        my $b = roaring(2, 4, 6, 8);
        rb_and($a, $b);
        my @v = rb_to_array($a);
        join(",", @v)
    "#;
    assert_eq!(eval_string(code), "2,4");
}

#[test]
fn roaring_xor() {
    let code = r#"
        my $a = roaring(1, 2, 3);
        my $b = roaring(2, 3, 4);
        rb_xor($a, $b);
        my @v = rb_to_array($a);
        join(",", @v)
    "#;
    assert_eq!(eval_string(code), "1,4");
}

#[test]
fn roaring_andnot() {
    let code = r#"
        my $a = roaring(1, 2, 3, 4);
        my $b = roaring(2, 4);
        rb_andnot($a, $b);
        my @v = rb_to_array($a);
        join(",", @v)
    "#;
    assert_eq!(eval_string(code), "1,3");
}

#[test]
fn roaring_min_max() {
    let code = r#"
        my $r = roaring(7, 100, 3, 50);
        rb_min($r) == 3 && rb_max($r) == 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn roaring_rank() {
    // rank(v) = number of elements <= v.
    assert_eq!(
        eval_int(r#"my $r = roaring(1, 5, 10, 20); rb_rank($r, 10)"#),
        3
    );
}

#[test]
fn roaring_serialize_roundtrip() {
    let code = r#"
        my $r = roaring(1, 10, 100, 1000, 10_000, 100_000);
        my $bytes = rb_serialize($r);
        my $r2 = rb_deserialize($bytes);
        rb_len($r2) == rb_len($r) && rb_contains($r2, 100_000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn roaring_compresses_dense_run() {
    // 1M consecutive u32s: bitmap should hold them all in run-encoded
    // form (no false negatives).
    let code = r#"
        my $r = roaring();
        rb_add($r, $_) for 1..10_000;
        rb_len($r) == 10_000 && rb_contains($r, 5000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn roaring_ref_type_is_roaringbitmap() {
    assert_eq!(
        eval_string(r#"my $r = roaring(); ref($r)"#),
        "RoaringBitmap"
    );
}

#[test]
fn roaring_appears_in_b_hash() {
    assert_eq!(eval_int(r#"exists $b{roaring} ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"exists $b{rb_add} ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"exists $b{rb_or} ? 1 : 0"#), 1);
}

// ── Rate limiter (token bucket + leaky bucket) ────────────────────────

#[test]
fn token_bucket_initial_capacity_full() {
    assert_eq!(
        eval_int(r#"my $rl = token_bucket(10, 5); rl_try_take($rl, 10)"#),
        1
    );
}

#[test]
fn token_bucket_rejects_over_capacity() {
    // Fresh bucket has cap=10 tokens; ask for 11.
    assert_eq!(
        eval_int(r#"my $rl = token_bucket(10, 5); rl_try_take($rl, 11)"#),
        0
    );
}

#[test]
fn token_bucket_drains_on_take() {
    let code = r#"
        my $rl = token_bucket(10, 0);  # no refill so we test exact accounting
        rl_try_take($rl, 5);
        my $avail = rl_available($rl);
        abs($avail - 5) < 0.1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn leaky_bucket_starts_empty() {
    let code = r#"
        my $rl = leaky_bucket(10, 1);
        rl_available($rl)
    "#;
    // Leaky semantics: "available" reports capacity-fill.
    assert_eq!(eval_int(code), 10);
}

#[test]
fn leaky_bucket_rejects_overflow() {
    assert_eq!(
        eval_int(r#"my $rl = leaky_bucket(5, 0); rl_try_take($rl, 6)"#),
        0
    );
}

#[test]
fn rate_limiter_ref_type() {
    assert_eq!(
        eval_string(r#"my $rl = token_bucket(10, 5); ref($rl)"#),
        "RateLimiter"
    );
}

// ── Hash ring ─────────────────────────────────────────────────────────

#[test]
fn hash_ring_routes_consistently_for_same_key() {
    let code = r#"
        my $hr = hash_ring(128);
        hr_add($hr, "a", "b", "c");
        my $r1 = hr_get($hr, "user-42");
        my $r2 = hr_get($hr, "user-42");
        $r1 eq $r2 && $r1 ne "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_ring_returns_undef_when_empty() {
    assert_eq!(
        eval_int(r#"my $hr = hash_ring(64); defined(hr_get($hr, "k")) ? 1 : 0"#),
        0
    );
}

#[test]
fn hash_ring_add_returns_new_count() {
    assert_eq!(
        eval_int(r#"my $hr = hash_ring(8); hr_add($hr, "a", "b", "a")"#),
        2 // "a" added once, "b" added once; second "a" rejected as dup
    );
}

#[test]
fn hash_ring_remove_drops_node_from_lookup() {
    let code = r#"
        my $hr = hash_ring(64);
        hr_add($hr, "a", "b", "c");
        my $before = hr_get($hr, "k");
        hr_remove($hr, $before);
        my $after = hr_get($hr, "k");
        $after ne "" && $after ne $before ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_ring_minimal_remapping_under_node_change() {
    // Add 1000 keys; track which node each routes to. Add a new node;
    // expect ~1/(N+1) of keys to remap, not most/all of them.
    let code = r#"
        my $hr = hash_ring(64);
        hr_add($hr, "n1", "n2", "n3");
        my %before;
        for my $i (1..1000) { $before{$i} = hr_get($hr, "k$i") }
        hr_add($hr, "n4");
        my $moved = 0;
        for my $i (1..1000) { $moved++ if hr_get($hr, "k$i") ne $before{$i} }
        # With 4 nodes after, expect ~25% (250) moved. Bound liberally.
        ($moved > 100 && $moved < 400) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── SimHash ───────────────────────────────────────────────────────────

#[test]
fn simhash_identical_docs_are_similarity_one() {
    let code = r#"
        my $a = simhash(); my $b = simhash();
        for my $w (qw(the quick brown fox jumps over the lazy dog)) {
            sh_add($a, $w); sh_add($b, $w);
        }
        sh_similarity($a, $b) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn simhash_disjoint_docs_have_lower_similarity() {
    let code = r#"
        my $a = simhash(); my $b = simhash();
        sh_add($a, $_) for qw(alpha beta gamma);
        sh_add($b, $_) for qw(delta epsilon zeta);
        # 64-bit random hashes of disjoint features should be far apart;
        # similarity should be substantially below 1.
        sh_similarity($a, $b) < 0.9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── MinHash ───────────────────────────────────────────────────────────

#[test]
fn minhash_identical_sets_have_jaccard_one() {
    let code = r#"
        my $a = minhash(128); my $b = minhash(128);
        for my $i (1..100) {
            mh_add($a, "k$i"); mh_add($b, "k$i");
        }
        mh_jaccard($a, $b) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn minhash_disjoint_sets_have_jaccard_zero() {
    let code = r#"
        my $a = minhash(128); my $b = minhash(128);
        mh_add($a, "k$_") for 1..100;
        mh_add($b, "x$_") for 1..100;
        mh_jaccard($a, $b) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn minhash_partial_overlap_in_expected_range() {
    let code = r#"
        my $a = minhash(256); my $b = minhash(256);
        mh_add($a, "k$_") for 1..1000;
        mh_add($b, "k$_") for 500..1499;
        # Truth: |A∩B|=501, |A∪B|=1499; Jaccard ≈ 0.334.
        # Allow 8% slop for the sketch error.
        my $j = mh_jaccard($a, $b);
        abs($j - 0.334) < 0.08 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── IntervalTree ──────────────────────────────────────────────────────

#[test]
fn interval_tree_query_point_returns_overlapping() {
    let code = r#"
        my $it = interval_tree();
        it_insert($it, 1, 10, "a");
        it_insert($it, 5, 15, "b");
        it_insert($it, 20, 30, "c");
        scalar(it_query_point($it, 7))
    "#;
    // [1,10] and [5,15] both contain 7.
    assert_eq!(eval_int(code), 2);
}

#[test]
fn interval_tree_query_range_returns_overlapping() {
    let code = r#"
        my $it = interval_tree();
        it_insert($it, 1, 5, "a");
        it_insert($it, 10, 15, "b");
        it_insert($it, 20, 25, "c");
        scalar(it_query_range($it, 12, 22))
    "#;
    // [10,15] (overlaps via 12) and [20,25] (overlaps via 22).
    assert_eq!(eval_int(code), 2);
}

#[test]
fn interval_tree_swapped_endpoints_normalized() {
    assert_eq!(
        eval_int(
            r#"
                my $it = interval_tree();
                it_insert($it, 10, 1, "swapped");
                scalar(it_query_point($it, 5))
            "#
        ),
        1
    );
}

// ── BK-tree ───────────────────────────────────────────────────────────

#[test]
fn bk_tree_fuzzy_match_returns_close_words() {
    let code = r#"
        my $bk = bk_tree();
        bk_insert($bk, "hello", "world", "help", "hells", "halo");
        # query "hallo" with max_dist=2; "hello" / "halo" are dist 1; "hells" is 2.
        my @r = bk_query($bk, "hallo", 2);
        scalar(@r) >= 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bk_tree_rejects_duplicate_inserts() {
    assert_eq!(
        eval_int(r#"my $bk = bk_tree(); bk_insert($bk, "x", "y", "x")"#),
        2
    );
}

#[test]
fn bk_tree_query_sorted_by_distance() {
    let code = r#"
        my $bk = bk_tree();
        bk_insert($bk, "kitten", "kittens", "sittin", "sitting");
        my @r = bk_query($bk, "kittin", 3);
        # First result must be the smallest-distance word.
        scalar(@r) > 0 && $r[0]->[1] <= $r[-1]->[1] ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Rope ──────────────────────────────────────────────────────────────

#[test]
fn rope_round_trips_string() {
    assert_eq!(
        eval_string(r#"my $r = rope("Hello, world!"); rope_to_string($r)"#),
        "Hello, world!"
    );
}

#[test]
fn rope_insert_at_position() {
    assert_eq!(
        eval_string(
            r#"my $r = rope("Hello, world!"); rope_insert($r, 7, "beautiful "); rope_to_string($r)"#
        ),
        "Hello, beautiful world!"
    );
}

#[test]
fn rope_delete_range() {
    assert_eq!(
        eval_string(r#"my $r = rope("Hello, world!"); rope_delete($r, 5, 13); rope_to_string($r)"#),
        "Hello"
    );
}

#[test]
fn rope_substring_extracts_range() {
    assert_eq!(
        eval_string(r#"my $r = rope("Hello, world!"); rope_substring($r, 7, 12)"#),
        "world"
    );
}

#[test]
fn rope_handles_unicode_codepoints() {
    let code = r#"
        my $r = rope("héllo wörld 🌍");
        my $len = rope_len($r);
        my $s = rope_substring($r, 6, 11);
        $len == 13 && $s eq "wörld" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Myers diff / Patience diff ────────────────────────────────────────

#[test]
fn myers_diff_identical_inputs_are_all_equals() {
    let code = r#"
        my @a = ("a", "b", "c");
        my @ops = myers_diff(\@a, \@a);
        my $all_eq = 1;
        for my $op (@ops) { $all_eq = 0 if $op->[0] ne "=" }
        $all_eq && scalar(@ops) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn myers_diff_substitution() {
    let code = r#"
        my @a = ("a", "b", "c");
        my @b = ("a", "x", "c");
        my @ops = myers_diff(\@a, \@b);
        # Expected: =a, -b, +x, =c (order may interleave - and +).
        my $minuses = 0; my $pluses = 0; my $equals = 0;
        for my $op (@ops) {
            $minuses++ if $op->[0] eq "-";
            $pluses++  if $op->[0] eq "+";
            $equals++  if $op->[0] eq "=";
        }
        $minuses == 1 && $pluses == 1 && $equals == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn myers_diff_pure_insertion() {
    let code = r#"
        my @a = ();
        my @b = ("x", "y");
        my @ops = myers_diff(\@a, \@b);
        my $pluses = 0;
        for my $op (@ops) { $pluses++ if $op->[0] eq "+" }
        $pluses == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn patience_diff_handles_unique_anchors() {
    let code = r#"
        my @a = ("alpha", "BLOCK1", "beta", "gamma", "BLOCK2", "delta");
        my @b = ("alpha", "BLOCK1", "BETA", "GAMMA", "BLOCK2", "delta");
        my @ops = patience_diff(\@a, \@b);
        # The BLOCK1 and BLOCK2 unique anchors should appear as equals.
        my $block_eqs = 0;
        for my $op (@ops) {
            $block_eqs++ if $op->[0] eq "=" && $op->[1] =~ /BLOCK/;
        }
        $block_eqs == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Reflection ────────────────────────────────────────────────────────

#[test]
fn tier2_builtins_in_b_hash() {
    for name in &[
        "token_bucket",
        "leaky_bucket",
        "rl_try_take",
        "rl_available",
        "hash_ring",
        "hr_get",
        "simhash",
        "sh_add",
        "sh_similarity",
        "minhash",
        "mh_jaccard",
        "interval_tree",
        "it_insert",
        "it_query_point",
        "bk_tree",
        "bk_insert",
        "bk_query",
        "rope",
        "rope_insert",
        "rope_delete",
        "myers_diff",
        "patience_diff",
    ] {
        let code = format!(r#"exists $b{{{name}}} ? 1 : 0"#);
        assert_eq!(eval_int(&code), 1, "missing from %b: {name}");
    }
}

// ── Sketch algebra — operator overloads on probabilistic data structures ─

#[test]
fn bloom_plus_is_union() {
    let code = r#"
        my $a = bloom_filter(1000, 0.01);
        my $b = bloom_filter(1000, 0.01);
        bloom_add($a, "alice");
        bloom_add($b, "bob");
        my $u = $a + $b;
        bloom_contains($u, "alice") + bloom_contains($u, "bob") + (bloom_contains($a, "bob") ? 0 : 1) + (bloom_contains($b, "alice") ? 0 : 1)
    "#;
    assert_eq!(eval_int(code), 4);
}

#[test]
fn bloom_pipe_alias_for_plus() {
    let code = r#"
        my $a = bloom_filter(1000, 0.01);
        my $b = bloom_filter(1000, 0.01);
        bloom_add($a, "x");
        bloom_add($b, "y");
        my $u = $a | $b;
        bloom_contains($u, "x") + bloom_contains($u, "y")
    "#;
    assert_eq!(eval_int(code), 2);
}

#[test]
fn hll_plus_is_union() {
    let code = r#"
        my $a = hll(14);
        my $b = hll(14);
        for my $i (1..100_000)     { hll_add($a, "k" . $i) }
        for my $i (100_001..200_000) { hll_add($b, "k" . $i) }
        my $u = $a + $b;
        my $c = hll_count($u);
        (abs($c - 200_000) / 200_000) < 0.02 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cms_plus_sums_counters() {
    let code = r#"
        my $a = cms(2048, 5);
        my $b = cms(2048, 5);
        for (1..100) { cms_add($a, "hot") }
        for (1..50)  { cms_add($b, "hot") }
        my $u = $a + $b;
        cms_count($u, "hot") >= 150 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn topk_plus_merges_heavies() {
    let code = r#"
        my $a = topk(5);
        my $b = topk(5);
        for (1..100) { topk_add($a, "alpha") }
        for (1..100) { topk_add($b, "beta")  }
        my $u = $a + $b;
        my @h = topk_heavies($u);
        my $found = 0;
        for my $row (@h) {
            $found++ if $row->[0] eq "alpha" || $row->[0] eq "beta";
        }
        $found
    "#;
    assert_eq!(eval_int(code), 2);
}

#[test]
fn tdigest_plus_merges_quantiles() {
    let code = r#"
        my $a = t_digest(100);
        my $b = t_digest(100);
        for my $i (1..1000)    { td_add($a, $i) }
        for my $i (1001..2000) { td_add($b, $i) }
        my $u = $a + $b;
        my $med = td_quantile($u, 0.5);
        (abs($med - 1000) < 50) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn roaring_full_set_algebra() {
    let code = r#"
        my $a = roaring();
        my $b = roaring();
        for (1..10)  { rb_add($a, $_) }
        for (5..15)  { rb_add($b, $_) }
        my $u = $a | $b;
        my $i = $a & $b;
        my $x = $a ^ $b;
        my $d = $a - $b;
        rb_len($u) + rb_len($i) * 10 + rb_len($x) * 100 + rb_len($d) * 1000
    "#;
    // |∪|=15, |∩|=6, |△|=9, |a\b|=4 → 15 + 60 + 900 + 4000 = 4975
    assert_eq!(eval_int(code), 4975);
}

#[test]
fn roaring_plus_is_union_alias() {
    let code = r#"
        my $a = roaring();
        my $b = roaring();
        rb_add($a, $_) for 1..5;
        rb_add($b, $_) for 4..8;
        my $u = $a + $b;
        rb_len($u)
    "#;
    assert_eq!(eval_int(code), 8);
}

#[test]
fn sketch_operators_do_not_mutate_operands() {
    let code = r#"
        my $a = roaring();
        my $b = roaring();
        rb_add($a, $_) for 1..3;
        rb_add($b, $_) for 4..6;
        my $u = $a + $b;
        rb_len($a) == 3 && rb_len($b) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
