//! Behavior-pinning batch CY (2026-05-09): **crypto & encoding** (**`crc32`**, **`adler32`**, **`sha256`**, **`md5`**, **`hex_encode`**,
//! **`hmac_sha256`**, **`fnv1a_32`**, **`fnv1a_64`**, **`base64`**, **`uri_escape` / `uri_unescape`**), **alignment scores** (**`needleman_wunsch_score`**,
//! **`smith_waterman_score`**), **strings** (**`soundex`**, **`metaphone`**, **`slugify`**, **`camel_case`**, **`snake_case`**, **`title_case`**,
//! **`xor_strings`**), **graphs** (**`graph_density`** — **BUG-177** spurious **`(N, E)`** call, **`graph_average_degree`**, **`degree_centrality`**,
//! **`pagerank`**, **`eigenvector_centrality`**, **`connected_components`**), **geo** (**`haversine`**, **`haversine_distance`**), **combinatorics**
//! (**`lucas`**, **`pell`**, **`catalan`**, **`subfactorial`**, **`multinomial`**, **`multiset_permutations_count`**), **geometry** (**`centroid`**,
//! **`polygon_centroid`**), **ML steps** (**`ml_logsumexp_step`**, **`ml_softmax_temperature`**), **special functions** (**`beta_fn`**, **`lgamma`**,
//! **`gamma`**, **`erf`**, **`erfc`**, **`betainc`**, **`inverse_erf`**, **`inverse_erfc`**), **spectral** (**`dft`**), **LA** (**`frobenius_norm`**,
//! **`matrix_rank`**, **`cholesky`**, **`diag`**, **`identity_matrix`**, **`zeros_matrix`**, **`matrix_transpose`** vs **`transpose`** — **BUG-178**),
//! **interpolation** (**`smoothstep`**), **rank correlation** (**`spearman`**, **`kendall_tau`**), **numeric** (**`mahalanobis_1d`**, **`nth_root`**,
//! **`is_power_of_two`**, **`log2`**, **`trailing_zeros`**).

use crate::common::*;

#[test]
fn soundex_ashcraft_cy() {
    assert_eq!(eval_string(r#"soundex("Ashcraft")"#), "A226");
}

#[test]
fn metaphone_knight_cy() {
    assert_eq!(eval_string(r#"metaphone("knight")"#), "NT");
}

#[test]
fn crc32_and_adler32_hello_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", crc32("hello"))"#),
        "907060870"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", adler32("hello"))"#),
        "103547413"
    );
}

#[test]
fn sha256_md5_hex_of_abc_cy() {
    assert_eq!(
        eval_string(r#"sha256("abc")"#),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        eval_string(r#"md5("abc")"#),
        "900150983cd24fb0d6963f7d28e17f72"
    );
}

#[test]
fn hex_encode_lowercase_cy() {
    assert_eq!(eval_string(r#"hex_encode("abc")"#), "616263");
}

#[test]
fn hmac_sha256_key_msg_cy() {
    assert_eq!(
        eval_string(r#"hmac_sha256("key", "msg")"#),
        "2d93cbc1be167bcb1637a4a23cbff01a7878f0c50ee833954ea5221bb1b8c628"
    );
}

#[test]
fn fnv1a_32_hello_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%u", fnv1a_32("hello"))"#),
        "1335831723"
    );
}

#[test]
fn fnv1a_64_hello_stringify_cy() {
    assert_eq!(
        eval_string(r#"stringify(fnv1a_64("hello"))"#),
        "-6615550055289275125"
    );
}

#[test]
fn needleman_wunsch_score_gap_ga_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", needleman_wunsch_score("GAP", "GA"))"#),
        "0"
    );
}

#[test]
fn smith_waterman_score_gaat_gatt_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", smith_waterman_score("GAAT", "GATT"))"#),
        "5"
    );
}

#[test]
fn base64_roundtrip_hi_cy() {
    assert_eq!(eval_string(r#"base64_encode("hi")"#), "aGk=");
    assert_eq!(eval_string(r#"base64_decode("aGk=")"#), "hi");
}

#[test]
fn uri_escape_unescape_space_cy() {
    assert_eq!(eval_string(r#"uri_escape("a b")"#), "a%20b");
    assert_eq!(eval_string(r#"uri_unescape("a%20b")"#), "a b");
}

#[test]
fn haversine_one_degree_lon_at_equator_cy() {
    let exp = "111.1949266";
    assert_eq!(
        eval_string(r#"sprintf("%.10g", haversine(0, 0, 0, 1))"#),
        exp
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", haversine_distance(0, 0, 0, 1))"#),
        exp
    );
}

#[test]
fn graph_density_three_node_path_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", graph_density([[1], [0, 2], [1]]))"#),
        "0.6666666667"
    );
}

/// **`graph_density`** expects an **adjacency list**; numeric **`(4, 3)`** is **not** **|E|/C(n**—**2)** — **BUG-177**.
#[test]
fn graph_density_spurious_numeric_pair_yields_zero_bug_cy() {
    assert_eq!(eval_string(r#"sprintf("%.10g", graph_density(4, 3))"#), "0");
}

#[test]
fn graph_average_degree_three_node_path_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", graph_average_degree([[1], [0, 2], [1]]))"#),
        "1.333333333"
    );
}

#[test]
fn degree_centrality_triangle_cy() {
    assert_eq!(
        eval_string(r#"stringify(degree_centrality([[0, 1, 1], [1, 0, 0], [1, 0, 0]]))"#),
        "(1.5, 1.5, 1.5)"
    );
}

#[test]
fn pagerank_two_node_cycle_cy() {
    assert_eq!(
        eval_string(r#"stringify(pagerank([[1], [0]], 0.85, 10))"#,),
        "(0.5, 0.5)"
    );
}

#[test]
fn eigenvector_centrality_two_node_cy() {
    assert_eq!(
        eval_string(r#"stringify(eigenvector_centrality([[1], [0]]))"#),
        "(0.707106781186548, 0.707106781186548)"
    );
}

#[test]
fn connected_components_single_edge_cy() {
    assert_eq!(
        eval_string(r#"stringify(connected_components([[1], [0]]))"#),
        "(0, 0)"
    );
}

#[test]
fn lucas_pell_catalan_cy() {
    assert_eq!(eval_string(r#"sprintf("%.10g", lucas(10))"#), "123");
    assert_eq!(eval_string(r#"sprintf("%.10g", pell(5))"#), "29");
    assert_eq!(eval_string(r#"sprintf("%.10g", catalan(4))"#), "14");
}

#[test]
fn subfactorial_multinomial_multiset_count_cy() {
    assert_eq!(eval_string(r#"sprintf("%.10g", subfactorial(4))"#), "9");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", multinomial([2, 1, 1]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", multiset_permutations_count([2, 1, 1]))"#),
        "12"
    );
}

#[test]
fn centroid_unit_square_and_triangle_cy() {
    assert_eq!(
        eval_string(r#"stringify(centroid([[0, 0], [2, 0], [2, 2], [0, 2]]))"#),
        "(1, 1)"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", polygon_centroid([[0, 0], [2, 0], [2, 3]]))"#,),
        "1.333333333"
    );
}

#[test]
fn ml_logsumexp_and_softmax_temperature_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_logsumexp_step([1, 2, 3]))"#),
        "3.407605964"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_softmax_temperature([1, 2, 3], 2))"#),
        "0.1863237232"
    );
}

#[test]
fn beta_gamma_lgamma_erf_erfc_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", beta_fn(2, 3))"#),
        "0.08333333333"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", lgamma(5))"#), "3.17805383");
    assert_eq!(eval_string(r#"sprintf("%.10g", gamma(5))"#), "24");
    assert_eq!(eval_string(r#"sprintf("%.10g", erf(1))"#), "0.8427007929");
    assert_eq!(eval_string(r#"sprintf("%.10g", erfc(0))"#), "1");
}

#[test]
fn betainc_and_inverse_error_functions_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", betainc(0.5, 2, 3))"#),
        "0.6875"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", inverse_erf(erf(0.5)))"#),
        "0.4999999999"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", inverse_erfc(0.5))"#),
        "0.4769362762"
    );
}

#[test]
fn dft_four_real_impulses_cy() {
    assert_eq!(
        eval_string(r#"stringify(dft([0, 1, 0, 0]))"#),
        "((1, 0), (6.12323399573677e-17, -1), (-1, -1.22464679914735e-16), (-1.83697019872103e-16, 1))"
    );
}

#[test]
fn slugify_title_camel_snake_cy() {
    assert_eq!(eval_string(r#"slugify("Hello World!")"#), "hello-world");
    assert_eq!(eval_string(r#"title_case("hello world")"#), "Hello World");
    assert_eq!(eval_string(r#"camel_case("foo_bar")"#), "fooBar");
    assert_eq!(eval_string(r#"snake_case("FooBar")"#), "foo_bar");
}

#[test]
fn xor_strings_first_byte_space_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", ord(substr(xor_strings("abc", "ABC"), 0, 1)))"#,),
        "32"
    );
}

#[test]
fn human_bytes_kilo_binary_cy() {
    assert_eq!(eval_string(r#"human_bytes(1536)"#), "1.50 KB");
}

#[test]
fn frobenius_norm_matrix_rank_smoothstep_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", frobenius_norm([[1, 2], [3, 4]]))"#),
        "5.477225575"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_rank([[1, 2], [2, 4]]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", smoothstep(0, 1, 0.25))"#),
        "0.15625"
    );
}

#[test]
fn spearman_and_kendall_perfect_negative_three_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", spearman([1, 2, 3], [3, 2, 1]))"#),
        "-1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kendall_tau([1, 2, 3], [3, 2, 1]))"#),
        "-1"
    );
}

#[test]
fn cholesky_spd_two_by_two_cy() {
    assert_eq!(
        eval_string(r#"stringify(cholesky([[4, 2], [2, 3]]))"#),
        "((2, 0), (1, 1.4142135623731))"
    );
}

#[test]
fn diag_identity_zeros_matrix_cy() {
    assert_eq!(
        eval_string(r#"stringify(diag([1, 2, 3]))"#),
        "([1, 0, 0], [0, 2, 0], [0, 0, 3])"
    );
    assert_eq!(
        eval_string(r#"stringify(identity_matrix(3))"#),
        "([1, 0, 0], [0, 1, 0], [0, 0, 1])"
    );
    assert_eq!(
        eval_string(r#"stringify(zeros_matrix(2, 3))"#),
        "([0, 0, 0], [0, 0, 0])"
    );
}

#[test]
fn matrix_transpose_two_by_two_cy() {
    assert_eq!(
        eval_string(r#"stringify(matrix_transpose([[1, 2], [3, 4]]))"#),
        "[[1, 3], [2, 4]]"
    );
}

/// **`transpose`** (vs **`matrix_transpose`**) — nested row shape differs (**BUG-178**).
#[test]
fn transpose_list_of_row_refs_not_matrix_transpose_bug_cy() {
    assert_eq!(
        eval_string(r#"stringify(transpose([[1, 2], [3, 4]]))"#),
        "([[1, 2]], [[3, 4]])"
    );
}

#[test]
fn mahalanobis_nth_root_power_two_log2_trailing_zeros_cy() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mahalanobis_1d(5, 0, 4))"#),
        "6.25"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", nth_root(27, 3))"#), "3");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", is_power_of_two(1024))"#),
        "1"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", log2(1024))"#), "10");
    assert_eq!(eval_string(r#"sprintf("%.10g", trailing_zeros(8))"#), "3");
}
