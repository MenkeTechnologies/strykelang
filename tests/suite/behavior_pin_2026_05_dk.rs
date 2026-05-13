//! Behavior-pinning batch DK (2026-05): **continuous PDFs** (`beta_pdf`, `gamma_pdf`, `chi2_pdf`, `t_pdf`,
//! `f_pdf`, `laplace_pdf`, `cauchy_pdf`, `pareto_pdf`, `weibull_pdf`, `lognormal_pdf`, `normal_cdf` / `normal_pdf`,
//! `poisson_pmf`), **graph / search micro-ops** from **`math_wolfram_networkx_graph_algorithms`** (`dijkstra_relax`, **`bellman_ford_relax`**,
//! **`floyd_warshall_step`**, **`astar_search`**, **`bidirectional_dijkstra`**, **`prim_step`**, **`kruskal_step`**,
//! **`johnson_reweight`**, **`yen_k_shortest`**, **`ida_star`**, **`bfs_count`**, **`tarjan_scc_step`**, **`topo_kahn_step`**,
//! **`dfs_postorder_done`**), **distribution / hash crumbs** (`db_jump_hash_bucket`), **procedural / SDF**
//! (`gfx_voronoi_distance`, `gfx_signed_distance_*`, `gfx_curl_noise_step`, `gfx_gradient_noise_step`), **polynomials**
//! (`chebyshev_u`, `hermite_h`, **`assoc_legendre_p`**, **`spherical_bessel_j`**, **`chebyshev_t`**), **value noise**,
//! **`db_consistent_hash_index`**, **Mandelbrot / Hanoi**.

use crate::common::*;

// ── Distributions ─────────────────────────────────────────────────────────

#[test]
fn beta_pdf_symmetric_beta_two_interior_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", beta_pdf(0.5, 2, 2))"#),
        "1.5"
    );
}

#[test]
fn gamma_pdf_shape_two_at_one_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gamma_pdf(1, 2, 1))"#),
        "0.3678794412"
    );
}

#[test]
fn chi2_pdf_four_df_at_one_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chi2_pdf(1, 4))"#),
        "0.1516326649"
    );
}

#[test]
fn t_pdf_zero_five_dof_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", t_pdf(0, 5))"#),
        "0.3796066898"
    );
}

#[test]
fn f_pdf_five_ten_df_at_one_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", f_pdf(1, 5, 10))"#),
        "0.4954797835"
    );
}

#[test]
fn laplace_pdf_peak_unit_scale_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", laplace_pdf(0, 0, 1))"#),
        "0.5"
    );
}

#[test]
fn cauchy_pdf_peak_unit_scale_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cauchy_pdf(0, 0, 1))"#),
        "0.3183098862"
    );
}

#[test]
fn pareto_pdf_two_xmin_one_shape_two_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pareto_pdf(2, 1, 2))"#),
        "0.25"
    );
}

#[test]
fn weibull_pdf_unit_with_shape_two_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", weibull_pdf(1, 2, 1))"#),
        "0.7357588823"
    );
}

#[test]
fn lognormal_pdf_mode_one_standard_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lognormal_pdf(1, 0, 1))"#),
        "0.3989422804"
    );
}

#[test]
fn normal_cdf_zero_half_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", normal_cdf(0))"#),
        "0.5000000005"
    );
}

#[test]
fn normal_pdf_zero_standard_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", normal_pdf(0, 0, 1))"#),
        "0.3989422804"
    );
}

#[test]
fn poisson_pmf_zero_rate_one_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", poisson_pmf(0, 1))"#),
        "0.3678794412"
    );
}

// ── Shortest-path / traversal micro-steps ─────────────────────────────────

#[test]
fn dijkstra_relax_adds_nonnegative_edge_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dijkstra_relax(2, 1, 99))"#),
        "3"
    );
}

#[test]
fn bellman_ford_relax_allows_negative_edge_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bellman_ford_relax(2, -1, 5))"#),
        "1"
    );
}

/// **BUG-203**: `dijkstra_relax` clamps **`w` ≥ 0**; honoring **`-5`** would give **`3 + (-5) = -2`** (contrast
/// **`bellman_ford_relax(3, -5, 10) -> -2`**).
#[test]
fn dijkstra_relax_clamps_negative_weight_bug203_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dijkstra_relax(3, -5, 10))"#),
        "3"
    );
}

#[test]
fn bellman_ford_relax_negative_weight_to_dist_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bellman_ford_relax(3, -5, 10))"#),
        "-2"
    );
}

#[test]
fn floyd_warshall_relaxation_min_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", floyd_warshall_step(10, 3, 4))"#),
        "7"
    );
}

#[test]
fn astar_search_f_equals_g_plus_h_dk() {
    assert_eq!(eval_string(r#"sprintf("%.10g", astar_search(5, 2))"#), "7");
}

#[test]
fn bidirectional_dijkstra_meeting_sum_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bidirectional_dijkstra(2, 3))"#),
        "5"
    );
}

#[test]
fn prim_relaxation_min_key_dk() {
    assert_eq!(eval_string(r#"sprintf("%.10g", prim_step(5, 2))"#), "2");
}

#[test]
fn kruskal_merge_distinct_roots_dk() {
    assert_eq!(eval_string(r#"sprintf("%d", kruskal_step(1, 2))"#), "1");
}

#[test]
fn kruskal_skip_same_component_dk() {
    assert_eq!(eval_string(r#"sprintf("%d", kruskal_step(3, 3))"#), "0");
}

#[test]
fn johnson_reweight_potential_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", johnson_reweight(1, 2, 3))"#),
        "0"
    );
}

#[test]
fn yen_k_shortest_adds_spur_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", yen_k_shortest(10, 3))"#),
        "13"
    );
}

#[test]
fn ida_star_next_threshold_dk() {
    assert_eq!(eval_string(r#"sprintf("%.10g", ida_star(4, 6))"#), "6");
}

#[test]
fn bfs_visit_count_branch_two_depth_three_dk() {
    assert_eq!(eval_string(r#"sprintf("%.10g", bfs_count(2, 3))"#), "15");
}

#[test]
fn tarjan_lowlink_min_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", tarjan_scc_step(5, 3))"#),
        "3"
    );
}

#[test]
fn topo_kahn_zero_indegree_ready_dk() {
    assert_eq!(eval_string(r#"sprintf("%d", topo_kahn_step(0))"#), "1");
}

#[test]
fn dfs_postorder_no_children_done_dk() {
    assert_eq!(eval_string(r#"sprintf("%d", dfs_postorder_done(0))"#), "1");
}

// ── Hashing / noise / SDF ──────────────────────────────────────────────────

#[test]
fn db_jump_hash_bucket_slot_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_jump_hash_bucket(12345, 10))"#),
        "1"
    );
}

#[test]
fn gfx_signed_distance_sphere_abs_minus_r_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gfx_signed_distance_sphere(5, 3))"#),
        "2"
    );
}

#[test]
fn gfx_signed_distance_box_1d_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gfx_signed_distance_box(2, 1))"#),
        "1"
    );
}

#[test]
fn gfx_voronoi_worley_min_feature_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gfx_voronoi_distance([3, 1.5, 4]))"#),
        "1.5"
    );
}

#[test]
fn gfx_curl_noise_component_zero_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gfx_curl_noise_step(0, 1, -1, 2, -2, 1))"#),
        "2"
    );
}

#[test]
fn gfx_gradient_noise_quarter_sample_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gfx_gradient_noise_step(0.25))"#),
        "0.3017578125"
    );
}

// ── Orthogonal polynomials & misc ────────────────────────────────────────

#[test]
fn chebyshev_u_degree_three_half_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chebyshev_u(3, 0.5))"#),
        "-1"
    );
}

#[test]
fn hermite_h_physicists_cubic_at_one_dk() {
    assert_eq!(eval_string(r#"sprintf("%.10g", hermite_h(3, 1))"#), "-4");
}

#[test]
fn mandelbrot_char_quick_escape_outside_dk() {
    assert_eq!(eval_string(r#"mandelbrot_char(2, 2, 50)"#), ".");
}

#[test]
fn tower_of_hanoi_classic_move_count_three_dk() {
    assert_eq!(eval_string(r#"sprintf("%d", tower_of_hanoi(3))"#), "7");
}

#[test]
fn gfx_value_noise_fixed_sample_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gfx_value_noise_step(0.25, 0.75))"#),
        "0.006334233342"
    );
}

#[test]
fn assoc_legendre_p_order_one_degree_two_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", assoc_legendre_p(2, 1, 0.5))"#),
        "-1.299038106"
    );
}

#[test]
fn spherical_bessel_j_order_two_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", spherical_bessel_j(2, 1.5))"#),
        "0.1273492837"
    );
}

#[test]
fn chebyshev_t_degree_four_half_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chebyshev_t(4, 0.5))"#),
        "-0.5"
    );
}

#[test]
fn db_consistent_hash_bucket_dk() {
    assert_eq!(
        eval_string(r#"sprintf("%d", db_consistent_hash_index(99, 8))"#),
        "3"
    );
}
