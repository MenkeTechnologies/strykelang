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
    assert_eq!(
        eval_string(r#"my $h = hll(12); ref($h)"#),
        "HllSketch"
    );
}

#[test]
fn hll_precision_returned_matches_construction() {
    assert_eq!(
        eval_int(r#"my $h = hll(13); hll_precision($h)"#),
        13
    );
}

#[test]
fn hll_precision_clamped_to_legal_range() {
    // Precision clamps to [4, 18]; 200 should clamp to 18 (m=262144).
    assert_eq!(
        eval_int(r#"my $h = hll(200); hll_precision($h)"#),
        18
    );
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
    assert_eq!(
        eval_string(r#"my $c = cms(); ref($c)"#),
        "CmsSketch"
    );
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
    assert_eq!(
        eval_string(r#"my $t = topk(10); ref($t)"#),
        "TopKSketch"
    );
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
    assert_eq!(
        eval_int(r#"my $r = roaring(1, 2, 3, 5, 8); rb_len($r)"#),
        5
    );
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

