//! Behavior-pinning batch DI (2026-05): **drag / SNR**, **RGB ↔ HSV**, **3D Euclidean / triangle / plane distance**,
//! **hashes & codec** (`sha256`, `rot13`), **bitcounts**, **graph summaries** (diameter, radius, density, edges, max degree),
//! **tree check** (adjacency-**list** `[[1],[0]]` vs mistaken 0/1 **matrix** `[[0,1],[1,0]]` — **BUG-199**),
//! **Floyd–Warshall**, **Dijkstra** (hash graph), **`bellman_ford`**, **topological sort**, **BFS path**, **sorted nums / LIS**,
//! **Kronecker product / Jacobi / Levi-Civita (three-index)** — **NLP / phonetics**, **`snowball_stem_english`** codepoints vs
//! bare strings — **BUG-200**, **`english_chi2`**, **`classification_metrics`**, **`categorical_cross_entropy`**, **`iqr`**,
//! **`variance`**, **`boykov_kolmogorov_step`**.

use crate::common::*;

#[test]
fn drag_force_aero_di() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", drag_force(0.47, 1.225, 0.01, 30))"#),
        "2.590875"
    );
}

#[test]
fn rgb_hsv_roundtrip_red_di() {
    assert_eq!(
        eval_string(r#"stringify(rgb_to_hsv(255, 0, 0))"#),
        "(0, 1, 1)"
    );
    assert_eq!(
        eval_string(r#"stringify(hsv_to_rgb(0, 1, 1))"#),
        "(255, 0, 0)"
    );
}

#[test]
fn euclidean_nd_triangle_3d_plane_dist_di() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", euclidean_distance_nd([0, 0, 0], [1, 2, 2]))"#),
        "3"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", triangle_3d_area([0, 0, 0], [1, 0, 0], [0, 1, 0]))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"stringify(triangle_3d_normal([0, 0, 0], [1, 0, 0], [0, 1, 0]))"#),
        "(0, 0, 1)"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dist_point_plane_3d(0, 0, 0, 0, 0, 1, 1))"#),
        "1"
    );
}

#[test]
fn psnr_db_amp_di() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", psnr(255, 5))"#),
        "0.1720034352"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", db_to_amp(20))"#), "10");
    assert_eq!(eval_string(r#"sprintf("%.10g", amp_to_db(10))"#), "20");
}

#[test]
fn sha256_and_rot13_di() {
    assert_eq!(
        eval_string(r#"sha256("abc")"#),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(eval_string(r#"rot13("uryyb")"#), "hello");
}

#[test]
fn hamming_weight_popcount_di() {
    assert_eq!(eval_string(r#"sprintf("%d", hamming_weight(15))"#), "4");
    assert_eq!(eval_string(r#"sprintf("%d", popcount(255))"#), "8");
}

#[test]
fn graph_diameter_radius_triangle_di() {
    assert_eq!(
        eval_string(r#"sprintf("%d", graph_diameter([[0, 1, 1], [1, 0, 1], [1, 1, 0]]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%d", graph_radius([[0, 1, 1], [1, 0, 1], [1, 1, 0]]))"#),
        "1"
    );
}

#[test]
fn graph_density_path_adj_list_di() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", graph_density([[1], [0]]))"#),
        "1"
    );
}

#[test]
fn floyd_warshall_three_node_di() {
    assert_eq!(
        eval_string(r#"stringify(floyd_warshall([[0, 3, 1e100], [3, 0, 1], [1e100, 1, 0]]))"#),
        "((0, 3, 4), (3, 0, 1), (4, 1, 0))"
    );
}

#[test]
fn dijkstra_hash_shortest_distances_di() {
    assert_eq!(
        eval_string(
            r#"my $h = { a => [[qq(b), 1]], b => [[qq(c), 2]], c => [] }; stringify(dijkstra($h, qq(a)))"#
        ),
        // Stable key order: pairs sorted lexicographically when building the result hash (BUG-201 fix).
        r#"("a", 0, "b", 1, "c", 3)"#
    );
}

#[test]
fn bellman_ford_two_edges_di() {
    assert_eq!(
        eval_string(r#"stringify(bellman_ford([[0, 1, 2], [1, 2, 1]], 3, 0))"#),
        "(0, 2, 3)"
    );
}

#[test]
fn topological_sort_chain_di() {
    assert_eq!(
        eval_string(r#"stringify(topological_sort([[0, 1], [1, 2]]))"#),
        r#"("0", "1", "2")"#
    );
}

#[test]
fn shortest_path_bfs_string_nodes_di() {
    assert_eq!(
        eval_string(
            r#"stringify(shortest_path_bfs(qq(a), qq(c), [[qq(a), qq(b)], [qq(b), qq(c)]]))"#
        ),
        r#"("a", "b", "c")"#
    );
}

#[test]
fn graph_tree_count_edges_max_degree_bug199_matrix_vs_list_di() {
    assert_eq!(
        eval_string(r#"sprintf("%d", graph_is_tree([[1], [0]]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%d", graph_is_tree([[0, 1], [1, 0]]))"#),
        "0"
    );
    assert_eq!(
        eval_string(r#"sprintf("%d", graph_count_edges([[1], [0]]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%d", graph_max_degree([[1, 2], [0], [0]]))"#),
        "2"
    );
}

#[test]
fn sorted_nums_variance_iqr_di() {
    assert_eq!(
        eval_string(r#"stringify(sorted_nums([3, 1, 4, 1]))"#),
        "(1, 1, 3, 4)"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", variance([1, 2, 3, 4, 5]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", iqr(1, 2, 3, 4, 5, 6, 7, 8, 9, 10))"#),
        "5"
    );
}

#[test]
fn longest_increasing_subsequence_length_di() {
    assert_eq!(
        eval_string(r#"sprintf("%d", longest_increasing([3, 1, 2, 4]))"#),
        "3"
    );
    assert_eq!(eval_string(r#"sprintf("%d", lis([3, 1, 2, 4]))"#), "3");
}

#[test]
fn kronecker_product_jacobi_levi_three_di() {
    assert_eq!(
        eval_string(r#"stringify(kronecker_product([1, 2], [3, 4]))"#),
        "((3), (4), (6), (8))"
    );
    assert_eq!(eval_string(r#"sprintf("%d", jacobi_symbol(15, 7))"#), "1");
    assert_eq!(
        eval_string(r#"sprintf("%d", levi_civita_three(0, 1, 2))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%d", levi_civita_three(0, 0, 1))"#),
        "0"
    );
}

#[test]
fn categorical_cross_entropy_and_classification_metrics_di() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", categorical_cross_entropy([[1, 0], [0, 1]], [[0.7, 0.3], [0.2, 0.8]]))"#
        ),
        "0.2899092476"
    );
    assert_eq!(
        eval_string(r#"stringify(classification_metrics(1, 1, 1, 1))"#),
        "(0.5, 0.5, 0.5, 0.5)"
    );
}

#[test]
fn soundex_metaphone_di() {
    assert_eq!(eval_string(r#"soundex("Robert")"#), "R163");
    assert_eq!(eval_string(r#"metaphone("knight")"#), "NT");
}

#[test]
fn snowball_stem_english_codepoints_not_string_bug200_di() {
    assert_eq!(
        eval_string(r#"stringify(snowball_stem_english("running"))"#),
        "0"
    );
    assert_eq!(
        eval_string(r#"stringify(snowball_stem_english([114, 117, 110, 110, 105, 110, 103]))"#),
        "(114, 117, 110, 110)"
    );
}

#[test]
fn english_chi2_cipher_bias_di() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", english_chi2("abc"))"#),
        "0.0817"
    );
}

#[test]
fn boykov_kolmogorov_graph_cut_step_di() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", boykov_kolmogorov_step(0.5, 0.3))"#),
        "0.8"
    );
}
