//! Behavior-pinning batch DA (2026-05-09): **Savitzky–Golay** (`sg_coeffs`, `sg_filter`), **UUID**
//! (`is_uuid`), **inferential stats** (`welch_ttest`, `paired_ttest`, `kruskal_wallis_test`, `anova_oneway` nest
//! pitfall — **BUG-181**), **Wasserstein** (`wasserstein_1d`), **graphs** (`topo_sort_adj`, `max_flow`,
//! `is_bipartite_graph`, `min_cut`, `pagerank`, `connected_components`, **`dijkstra`** accepts **`HashRef`**),
//! **contingency / chi** (`chi_square_stat`, `fishers_exact`, `pchisq`), **hashes / encodings** (`xxh64`,
//! `blake2s`, `crc32`, `adler32`, `fnv1a`, `md5`, `sha1`, `sha256`, `ripemd160`, `blake2b`, `hex_encode`,
//! `from_hex`), **string metrics** (`hamming_distance`, `levenshtein`, `jaro_winkler`, `soundex`,
//! `metaphone`, `morse_encode`, `rot13`), **correlation / regression** (`pearsonr`, `corr`, `spearman`,
//! `kendall`, `linear_regression`, `shannon_entropy_rate`), **nonparametric tests** (`mann_whitney`,
//! `wilcoxon`, `ks_test`), **graph paths** (`bellman_ford`, `floyd_warshall`, **`dijkstra`**), **linear
//! algebra crumbs** (`matrix_det`, `matrix_trace`), **NT** (`gcd`, `lcm`, `euler_totient`, `collatz_length`,
//! `is_prime`, `binomial`), **special functions** (`erf`, `lambert_w0`, `airy_ai`, `bessel_j0`),
//! **quadrature** (`trapz`, `simpson` — second operand is **`dx`**, not **`XS`** — **BUG-182**).

use crate::common::*;

use stryke::vm_helper::VMHelper;

fn eval_runtime_message(code: &str) -> String {
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let program = stryke::parse(code).expect("parse");
    let mut interp = VMHelper::new();
    interp
        .execute(&program)
        .expect_err("expected runtime error")
        .message
}

#[test]
fn sg_coeffs_window_three_order_one_da() {
    assert_eq!(
        eval_string(r#"stringify(sg_coeffs(3, 1))"#),
        "(-0.142857142857142, 0.171428571428572, 0.342857142857143, 0.371428571428571, 0.257142857142857)"
    );
}

#[test]
fn sg_filter_small_series_da() {
    assert_eq!(
        eval_string(r#"stringify(sg_filter([1, 2, 3, 2, 1], 3, 1))"#),
        "(3, 6, 9, 6, 3)"
    );
}

#[test]
fn is_uuid_valid_lowercase_da() {
    assert_eq!(
        eval_int(r#"is_uuid("550e8400-e29b-41d4-a716-446655440000")"#),
        1
    );
}

#[test]
fn is_uuid_invalid_returns_zero_da() {
    assert_eq!(eval_int(r#"is_uuid("not-a-uuid")"#), 0);
}

#[test]
fn welch_ttest_two_small_groups_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", welch_ttest([1, 2, 3], [2, 3, 4]))"#),
        "-1.224744871"
    );
}

#[test]
fn paired_ttest_matched_samples_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", paired_ttest([1, 2, 3], [1.1, 2.2, 2.9]))"#),
        "-0.755928946"
    );
}

#[test]
fn wasserstein_1d_two_point_uniform_shift_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", wasserstein_1d([0, 1], [1, 2]))"#),
        "1"
    );
}

#[test]
fn topo_sort_adj_cycle_yields_empty_da() {
    assert_eq!(eval_string(r#"stringify(topo_sort_adj([[1], [0]]))"#), "()");
}

#[test]
fn topo_sort_adj_diamond_dag_order_da() {
    assert_eq!(
        eval_string(r#"stringify(topo_sort_adj([[1, 2], [], []]))"#),
        "(0, 1, 2)"
    );
}

#[test]
fn topo_sort_adj_linear_dag_order_da() {
    assert_eq!(
        eval_string(r#"stringify(topo_sort_adj([[1], [2], []]))"#),
        "(0, 1, 2)"
    );
}

#[test]
fn max_flow_two_node_capacity_da() {
    assert_eq!(eval_int(r#"max_flow([[0, 5], [0, 0]], 0, 1)"#), 5);
}

#[test]
fn is_bipartite_graph_two_clique_da() {
    assert_eq!(eval_int(r#"is_bipartite_graph([[0, 1], [1, 0]])"#), 1);
}

#[test]
fn chi_square_stat_two_bins_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chi_square_stat([10, 10], [12, 8]))"#),
        "0.8333333333"
    );
}

#[test]
fn fishers_exact_two_by_two_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", fishers_exact([[10, 5], [8, 12]]))"#),
        "0.1755831762"
    );
}

#[test]
fn pchisq_df_one_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pchisq(3.84, 1))"#),
        "0.9499564788"
    );
}

/// Variadic groups — correct `anova_oneway` calling convention (**contrast BUG-181** nest trap).
#[test]
fn anova_oneway_variadic_two_groups_da() {
    assert_eq!(
        eval_string(r#"stringify(anova_oneway([1, 2, 3], [2, 3, 4]))"#),
        "(1.5, 1, 4)"
    );
}

/// Single nested AoA is **one** group bucket → misleading **`anova: need at least 2 groups`** (**BUG-181**).
#[test]
fn anova_oneway_nested_aoa_error_message_da() {
    let msg = eval_runtime_message(r#"anova_oneway([[1, 2, 3], [2, 3, 4]])"#);
    assert!(
        msg.contains("anova: need at least 2 groups"),
        "unexpected message: {}",
        msg
    );
}

#[test]
fn kruskal_wallis_two_groups_da() {
    assert_eq!(
        eval_string(r#"stringify(kruskal_wallis_test([1, 2, 9], [2, 3, 4]))"#),
        "(2, 2, 0.367879441171442)"
    );
}

#[test]
fn xxh64_hello_is_hex_string_not_truncated_uint_da() {
    assert_eq!(
        eval_string(r#"stringify(xxh64("hello"))"#),
        "\"26c7827d889f6da3\""
    );
}

#[test]
fn blake2s_abc_full_digest_da() {
    assert_eq!(
        eval_string(r#"blake2s("abc")"#),
        "508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982"
    );
}

#[test]
fn crc32_adler32_fnv1a_abc_da() {
    assert_eq!(eval_string(r#"sprintf("%d", crc32("abc"))"#), "891568578");
    assert_eq!(eval_string(r#"sprintf("%d", adler32("abc"))"#), "38600999");
    assert_eq!(
        eval_string(r#"sprintf("%.0f", fnv1a("abc"))"#),
        "-1792535898324117760"
    );
}

#[test]
fn hamming_levenshtein_jaro_winkler_da() {
    assert_eq!(eval_int(r#"hamming_distance("abc", "abd")"#), 1);
    assert_eq!(eval_int(r#"levenshtein("kitten", "sitting")"#), 3);
    assert_eq!(
        eval_string(r#"sprintf("%.15g", jaro_winkler("foo", "food"))"#),
        "0.941666666666667"
    );
}

#[test]
fn soundex_metaphone_washington_da() {
    assert_eq!(eval_string(r#"soundex("Washington")"#), "W252");
    assert_eq!(eval_string(r#"metaphone("Washington")"#), "WXHNTN");
}

#[test]
fn pearson_corr_spearman_kendall_unit_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pearsonr([1, 2, 3], [2, 4, 6]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", corr([1, 2, 3], [2, 4, 6]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", spearman([1, 2, 3], [3, 2, 1]))"#),
        "-1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kendall([1, 2, 3], [1, 2, 3]))"#),
        "1"
    );
}

#[test]
fn linear_regression_perfect_line_da() {
    assert_eq!(
        eval_string(r#"stringify(linear_regression([1, 2, 3], [2, 4, 6]))"#),
        "(2, 0, 1)"
    );
}

#[test]
fn shannon_entropy_rate_small_pattern_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", shannon_entropy_rate([1, 1, 2, 2, 3]))"#),
        "0.3313741931"
    );
}

#[test]
fn connected_components_two_cycle_da() {
    assert_eq!(
        eval_string(r#"stringify(connected_components([[0, 1], [1, 0]]))"#),
        "(0, 0)"
    );
}

#[test]
fn min_cut_triangle_graph_da() {
    assert_eq!(
        eval_int(r#"min_cut([[0, 5, 0], [5, 0, 5], [0, 5, 0]], 0, 2)"#),
        5
    );
}

#[test]
fn pagerank_two_node_cycle_da() {
    assert_eq!(
        eval_string(r#"stringify(pagerank([[0, 1], [1, 0]], 0.85))"#),
        "(0.5, 0.5)"
    );
}

#[test]
fn bellman_ford_simple_chain_da() {
    assert_eq!(
        eval_string(r#"stringify(bellman_ford([[0, 1, 2], [1, 2, 3]], 3, 0))"#),
        "(0, 2, 5)"
    );
}

#[test]
fn floyd_warshall_updates_distance_zero_to_two_da() {
    assert_eq!(
        eval_string(
            r#"my $m = floyd_warshall([[0,2,1e100],[1e100,0,1],[1e100,1e100,0]]); sprintf("%.10g", $m->[0][2])"#
        ),
        "3"
    );
}

#[test]
fn matrix_det_and_trace_two_by_two_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_det([[1, 2], [3, 4]]))"#),
        "-2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_trace([[1, 2], [3, 4]]))"#),
        "5"
    );
}

#[test]
fn mann_whitney_wilcox_ks_two_sample_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mann_whitney([1, 2, 9], [2, 3, 4]))"#),
        "3"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", wilcoxon([1, 2, 3], [1.1, 2.1, 2.9]))"#),
        "6"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ks_test([1, 2, 3], [1, 2, 4]))"#),
        "0.3333333333"
    );
}

#[test]
fn morse_rot13_roundtrip_style_da() {
    assert_eq!(eval_string(r#"morse_encode("SOS")"#), "... --- ...");
    assert_eq!(eval_string(r#"rot13("uryyb")"#), "hello");
}

#[test]
fn md5_sha1_sha256_abc_da() {
    assert_eq!(
        eval_string(r#"md5("abc")"#),
        "900150983cd24fb0d6963f7d28e17f72"
    );
    assert_eq!(
        eval_string(r#"sha1("abc")"#),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(
        eval_string(r#"sha256("abc")"#),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn ripemd160_blake2b_prefix_hex_from_hex_da() {
    assert_eq!(
        eval_string(r#"ripemd160("abc")"#),
        "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.16s", blake2b("abc"))"#),
        "ba80a53f981c4d0d"
    );
    assert_eq!(eval_string(r#"hex_encode("abc")"#), "616263");
    assert_eq!(
        eval_string(r#"sprintf("%.0f", from_hex("616263"))"#),
        "6382179"
    );
}

#[test]
fn trapz_simpson_evenly_spaced_y_with_dx_one_da() {
    assert_eq!(eval_string(r#"sprintf("%.10g", trapz([0, 1, 4], 1))"#), "3");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", simpson([0, 1, 4], 1))"#),
        "2.666666667"
    );
}

/// **`trapz Y [, dx]`** — second operand is **`dx`** scalar; passing a second **array** numifies to **0** → area **0** (**polish** footgun).
#[test]
fn trapz_two_array_operands_second_becomes_dx_zero_da() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", trapz([0, 1, 2], [0, 1, 4]))"#),
        "0"
    );
}

#[test]
fn erf_lambert_airy_bessel_samples_da() {
    assert_eq!(eval_string(r#"sprintf("%.10g", erf(1))"#), "0.8427007929");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lambert_w0(2))"#),
        "0.852605502"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", airy_ai(0))"#),
        "0.3550280539"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bessel_j0(1))"#),
        "0.7651976838"
    );
}

#[test]
fn gcd_lcm_binomial_totient_collatz_prime_da() {
    assert_eq!(eval_int(r#"gcd(84, 30)"#), 6);
    assert_eq!(eval_int(r#"lcm(4, 6)"#), 12);
    assert_eq!(eval_int(r#"binomial(5, 2)"#), 10);
    assert_eq!(eval_int(r#"euler_totient(12)"#), 4);
    assert_eq!(eval_int(r#"collatz_length(7)"#), 16);
    assert_eq!(eval_int(r#"is_prime(17)"#), 1);
}

#[test]
fn dijkstra_accepts_anon_hashref_plus_block_da() {
    assert_eq!(
        eval_string(
            r#"my %dist = dijkstra( +{ "0" => [[1, 2]], "1" => [[2, 3]], "2" => [] }, "0"); sprintf("%.10g", $dist{"2"})"#
        ),
        "5"
    );
}

#[test]
fn dijkstra_accepts_named_hash_variable_ref_da() {
    assert_eq!(
        eval_string(
            r#"my %g = ("0" => [[1, 2]], "1" => [[2, 3]], "2" => []); my %dist = dijkstra(\%g, "0"); sprintf("%.10g", $dist{"1"})"#
        ),
        "2"
    );
}
