//! Behavior-pinning batch DL (2026-05): **database / sketch micro-ops** (`math_wolfram48` — B-tree, LSM,
//! Bloom / cuckoo / quotient filters, count-min, HyperLogLog, min-hash, **SimHash sign quirk** — **BUG-204**),
//! **planner cost** stubs, **quantiles** (p99, KLL, t-digest, DD-sketch, reservoir), **multiset / multinomial /
//! Stirling / binomial**, **Carlson RF**, **elliptic F**, **Jacobi AM**, **polylog**, **Legendre Q**, **Gegenbauer**,
//! **Laguerre**, **Weierstrass ℘**, **Zernike**, **spherical harmonic**, **Chao / MinHash–Jaccard / LPC / HLL** estimates.

use crate::common::*;

// ── math_wolfram48: structures & sketches ───────────────────────────────

#[test]
fn db_b_tree_split_median_index_dl() {
    assert_eq!(eval_string(r#"sprintf("%d", db_b_tree_split(7))"#), "3");
}

#[test]
fn db_b_tree_merge_child_counts_dl() {
    assert_eq!(eval_string(r#"sprintf("%d", db_b_tree_merge(3, 5))"#), "8");
}

#[test]
fn db_lsm_compaction_scaled_size_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_lsm_compaction_step(2, 10))"#),
        "20"
    );
}

#[test]
fn db_skiplist_height_geometric_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_skiplist_height_pick(0.25, 0.5))"#),
        "3"
    );
}

#[test]
fn db_bloom_filter_modulo_bucket_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_bloom_filter_bit_index(12345, 64))"#),
        "57"
    );
}

#[test]
fn db_cuckoo_fingerprint_low_byte_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_cuckoo_filter_fingerprint(0x1234abcd))"#),
        "205"
    );
}

#[test]
fn db_quotient_filter_shifted_word_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_quotient_filter_canonical(0xffff0000, 8))"#),
        "0"
    );
}

#[test]
fn db_count_min_sketch_bin_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_count_min_sketch_bin(99, 17))"#),
        "14"
    );
}

#[test]
fn db_hyperloglog_rho_register_unity_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_hyperloglog_register_max(1))"#),
        "64"
    );
}

#[test]
fn db_min_hash_fold_vector_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_min_hash_value([4.5, 1.25, 9.0]))"#),
        "1.25"
    );
}

/// **BUG-204**: name / Rust doc say “bit **index**”; implementation is **sign bit** (**`≥ 0` → 1**).
#[test]
fn db_simhash_positive_is_one_bug204_dl() {
    assert_eq!(eval_string(r#"sprintf("%d", db_simhash_bit(0.5))"#), "1");
}

#[test]
fn db_simhash_negative_is_zero_bug204_dl() {
    assert_eq!(eval_string(r#"sprintf("%d", db_simhash_bit(-0.1))"#), "0");
}

#[test]
fn db_rendezvous_hash_score_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_rendezvous_hash_score(2, 3))"#),
        "-0.2794154982"
    );
}

#[test]
fn db_maglev_offset_wrap_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_maglev_hash_step(40, 15, 97))"#),
        "55"
    );
}

#[test]
fn db_lru_eviction_age_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_lru_cache_eviction_age(100, 30))"#),
        "70"
    );
}

#[test]
fn db_lfu_decay_exponential_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_lfu_cache_decay(10, 0.9, 2))"#),
        "8.1"
    );
}

#[test]
fn db_arc_adaptive_score_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_arc_cache_score(2, 8, 0.25))"#),
        "6.5"
    );
}

#[test]
fn db_clock_hand_advance_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_clock_cache_hand(3, 10))"#),
        "4"
    );
}

#[test]
fn db_tinylfu_admit_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_tinylfu_admit_score(5, 3))"#),
        "1"
    );
}

#[test]
fn db_buffer_pool_score_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_buffer_pool_score(4, 2.5))"#),
        "10"
    );
}

// ── Planner cost model ────────────────────────────────────────────────────

#[test]
fn db_query_plan_cost_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_query_plan_cost_step(1000, 0.02))"#),
        "20"
    );
}

#[test]
fn db_join_selectivity_inverse_max_distinct_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_join_selectivity_step(8, 12))"#),
        "0.08333333333"
    );
}

#[test]
fn db_index_seek_btree_depth_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_index_seek_cost(1000, 10))"#),
        "3"
    );
}

#[test]
fn db_index_scan_log_plus_matches_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_index_scan_cost(1000, 50))"#),
        "56.90775528"
    );
}

#[test]
fn db_seq_scan_pages_plus_cpu_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_seq_scan_cost(250, 100, 1, 0.01))"#),
        "5.5"
    );
}

#[test]
fn db_sort_cost_n_log_n_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_sort_cost_estimate(1000))"#),
        "6907.755279"
    );
}

#[test]
fn db_hash_join_build_probe_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_hash_join_cost(100, 200))"#),
        "300"
    );
}

#[test]
fn db_nested_loop_product_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_nested_loop_cost(50, 80))"#),
        "4000"
    );
}

#[test]
fn db_merge_join_both_sorted_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_merge_join_cost(500, 500, 64, 1, 1))"#),
        "1000"
    );
}

#[test]
fn db_merge_join_external_sort_penalty_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_merge_join_cost(500, 500, 2))"#),
        "16931.56857"
    );
}

// ── Quantiles & sketches ──────────────────────────────────────────────────

#[test]
fn db_histogram_bucket_floor_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_histogram_bucket_index(23, 10, 5))"#),
        "2"
    );
}

#[test]
fn db_query_cardinality_product_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_query_cardinality(1000, 0.25))"#),
        "250"
    );
}

#[test]
fn db_quantile_p99_sorted_support_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_quantile_estimate_p99([1, 2, 3, 4, 5, 100]))"#),
        "5"
    );
}

#[test]
fn db_kll_quantile_median_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_kll_quantile_step(0.5, [1, 10, 2, 9]))"#),
        "2"
    );
}

#[test]
fn db_t_digest_centroid_update_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_t_digest_centroid(10, 2, 4))"#),
        "8"
    );
}

#[test]
fn db_dd_sketch_log_bin_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_dd_sketch_bin(10, 0.1))"#),
        "12"
    );
}

#[test]
fn db_reservoir_sample_index_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_reservoir_sample_index(100, 0.253))"#),
        "25"
    );
}

// ── Combinatorics ─────────────────────────────────────────────────────────

#[test]
fn multiset_permutations_trinomial_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", multiset_permutations_count([2, 1, 1]))"#),
        "12"
    );
}

#[test]
fn multinomial_coeff_four_two_one_one_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", multinomial(4, 2, 1, 1))"#),
        "12"
    );
}

#[test]
fn stirling_second_partition_count_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", stirling_second(5, 3))"#),
        "25"
    );
}

#[test]
fn binomial_ten_choose_four_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%d", binomial(10, 4))"#),
        "210"
    );
}

// ── Elliptic, polylog, special functions ─────────────────────────────────

#[test]
fn carlson_rf_symmetric_triple_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", carlson_rf(1, 2, 3))"#),
        "0.7269459355"
    );
}

#[test]
fn incomplete_elliptic_f_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", elliptic_f(0.5, 0.3))"#),
        "0.506140212"
    );
}

#[test]
fn jacobi_amplitude_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jacobi_am(0.4, 0.2))"#),
        "0.2910501244"
    );
}

#[test]
fn polylog_dilog_half_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", polylog(2, 0.5))"#),
        "0.5822405265"
    );
}

#[test]
fn legendre_q_second_kind_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", legendre_q(2, 0.5))"#),
        "-0.818663268"
    );
}

#[test]
fn gegenbauer_ultraspherical_cubic_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gegenbauer_c(3, 2.0, 0.5))"#),
        "-2"
    );
}

#[test]
fn laguerre_generalized_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", laguerre_l(2, 1.5))"#),
        "-0.875"
    );
}

#[test]
fn weierstrass_p_near_origin_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", weierstrass_p(0.1, 0.5, 0.25))"#),
        "100.0002509"
    );
}

#[test]
fn zernike_radial_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", zernike_r(2, 2, 0.5))"#),
        "0.25"
    );
}

#[test]
fn spherical_harmonic_y_m_zero_dl() {
    assert_eq!(
        eval_string(r#"stringify(spherical_harmonic_y(1, 0, 0.5, 0.25))"#),
        "(0.428789044141836, 0)"
    );
}

#[test]
fn db_chao_richness_estimator_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_chao_estimator_step(10, 4, 2))"#),
        "14"
    );
}

#[test]
fn db_jaccard_minhash_ratio_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_jaccard_minhash_estimate(3, 10))"#),
        "0.3"
    );
}

#[test]
fn db_linear_probabilistic_counting_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_distinct_estimate_lpc(100, 50))"#),
        "69.31471806"
    );
}

#[test]
fn db_hyperloglog_distinct_formula_dl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", db_distinct_estimate_hll(64, 10))"#),
        "290.5460551"
    );
}
