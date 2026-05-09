//! Behavior-pinning batch CG (2026-05-09): map projections + geohash (**`geohash_neighbor`** no-op deltas — BUG-136),
//! static filter kernels (**`sharpen_kernel`**, **`box_blur_kernel`** radius semantics — BUG-137, Haar/Wavelet tuples), string codepage
//! helpers (**`charcodes_to_string`** array bucket — BUG-126),
//! AES S-box stubs, **`simon_round`**, small graph predicates (**DFS/BFS**/bridges/Euler/Hamilton), calendar helpers.

use crate::common::*;

#[test]
fn geohash_encode_sf_precision_six_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", geohash_encode(37.7749, -122.4194, 6))"#),
        "9q8yyk"
    );
}

#[test]
fn geohash_decode_centroid_tuple_cg() {
    assert_eq!(
        eval_string(r#"stringify(geohash_decode("9q8yyk"))"#),
        "(37.7737426757812, -122.415161132812)"
    );
}

#[test]
fn geohash_bbox_four_corner_tuple_cg() {
    assert_eq!(
        eval_string(r#"stringify(geohash_bbox("9q8"))"#),
        "(36.5625, -123.75, 37.96875, -122.34375)"
    );
}

#[test]
fn geohash_neighbor_cardinals_are_identity_at_precision_six_cg() {
    assert_eq!(
        eval_string(
            r#"(geohash_neighbor("9q8yyk", "n") eq "9q8yyk" && geohash_neighbor("9q8yyk", "e") eq "9q8yyk" && geohash_neighbor("9q8yyk", "ne") eq "9q8yyk") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn geohash_neighbor_unknown_direction_leaves_hash_unchanged_cg() {
    assert_eq!(
        eval_string(r#"(geohash_neighbor("9q8yyk", "oops") eq "9q8yyk") ? "1" : "0""#),
        "1"
    );
}

#[test]
fn string_to_charcodes_ascii_hi_cg() {
    assert_eq!(
        eval_string(r#"stringify(string_to_charcodes("Hi"))"#),
        "(72, 105)"
    );
}

#[test]
fn charcodes_to_string_array_round_trip_hi_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", charcodes_to_string([72, 105]))"#),
        "Hi"
    );
}

#[test]
fn charcodes_to_string_variadic_second_codepoint_dropped_tail_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", charcodes_to_string(72, 105))"#),
        "H"
    );
}

#[test]
fn string_xor_repeating_key_single_space_flip_case_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", string_xor("abc", " "))"#),
        "ABC"
    );
}

#[test]
fn substring_count_non_overlapping_pair_in_triple_a_cg() {
    assert_eq!(eval_string(r#"substring_count("aaa", "aa")"#), "1");
}

#[test]
fn string_kebab_to_snake_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", string_kebab_to_snake("ke-bab"))"#),
        "ke_bab"
    );
}

#[test]
fn string_snake_to_camel_example_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", string_snake_to_camel("snake_case"))"#),
        "snakeCase"
    );
}

#[test]
fn string_truncate_ellipsis_preserves_budget_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", string_truncate_ellipsis("abcdefgh", 4))"#),
        "abc…"
    );
}

#[test]
fn string_expand_tabs_four_column_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", string_expand_tabs("a\tb", 4))"#),
        "a   b"
    );
}

#[test]
fn mollweide_projection_wgs84_example_cg() {
    assert_eq!(
        eval_string(r#"stringify(mollweide_project(122.4, 37.7))"#),
        "(-590238.031622705, -8909311.05640884)"
    );
}

#[test]
fn robinson_projection_b_example_cg() {
    assert_eq!(
        eval_string(r#"stringify(robinson_project(140, -35))"#),
        "(-2711485.64770041, 18143683.6834007)"
    );
}

#[test]
fn sinusoidal_projection_degrees_example_cg() {
    assert_eq!(
        eval_string(r#"stringify(sinusoidal_project(140, -20))"#),
        "(1705513.54666043, 15584728.7110583)"
    );
}

#[test]
fn equirectangular_projection_default_std_parallel_cg() {
    assert_eq!(
        eval_string(r#"stringify(equirectangular_project(12, -8))"#),
        "(-890555.926346189, 1335833.88951928)"
    );
}

#[test]
fn lambert_azimuthal_projection_defaults_cg() {
    assert_eq!(
        eval_string(r#"stringify(lambert_azimuthal_project(100, -30))"#),
        "(849645.313469769, 9637156.03880898)"
    );
}

#[test]
fn albers_conic_projection_sample_cg() {
    assert_eq!(
        eval_string(r#"stringify(albers_conic_project(40, -100, 0.5, 0.9, 0.6, -96, 6_371_000))"#),
        "(-441303.748618592, 4044595.7371313)"
    );
}

#[test]
fn haar_two_by_two_ll_and_hh_subbands_cg() {
    assert_eq!(
        eval_string(r#"stringify(haar_2d_step([[1, 2], [3, 4]]))"#),
        "(((2.5)), ((0)))"
    );
}

#[test]
fn db4_scaling_coeffs_tuple_cg() {
    assert_eq!(
        eval_string(r#"stringify(db4_coeffs())"#),
        "(0.482962911806533, 0.836516302399807, 0.224143869380014, -0.129409521213259)"
    );
}

#[test]
fn db6_scaling_coeffs_tuple_cg() {
    assert_eq!(
        eval_string(r#"stringify(db6_coeffs())"#),
        "(0.47046721, 1.14111692, 0.65036501, -0.19093442, -0.12083221, 0.0498175)"
    );
}

#[test]
fn sym4_scaling_coeffs_tuple_cg() {
    assert_eq!(
        eval_string(r#"stringify(sym4_coeffs())"#),
        "(-0.07576571, -0.02963553, 0.49761867, 0.80373875, 0.2978578, -0.09921954, -0.01260396, 0.0322231)"
    );
}

#[test]
fn coif1_scaling_coeffs_tuple_cg() {
    assert_eq!(
        eval_string(r#"stringify(coif1_coeffs())"#),
        "(-0.01565572, -0.07273262, 0.38486485, 0.85257202, 0.33789767, -0.07273262)"
    );
}

#[test]
fn sharpen_kernel_three_by_three_cg() {
    assert_eq!(
        eval_string(r#"stringify(sharpen_kernel())"#),
        "((0, -1, 0), (-1, 5, -1), (0, -1, 0))"
    );
}

#[test]
fn edge_detect_kernel_three_by_three_cg() {
    assert_eq!(
        eval_string(r#"stringify(edge_detect_kernel())"#),
        "((-1, -1, -1), (-1, 8, -1), (-1, -1, -1))"
    );
}

#[test]
fn sobel_diagonal_kernel_three_by_three_cg() {
    assert_eq!(
        eval_string(r#"stringify(sobel_diagonal_kernel())"#),
        "((0, 1, 2), (-1, 0, 1), (-2, -1, 0))"
    );
}

#[test]
fn box_blur_kernel_radius_three_is_seven_squared_weights_cg() {
    assert_eq!(
        eval_string(r#"stringify(box_blur_kernel(3))"#),
        "((0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061), (0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061), (0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061), (0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061), (0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061), (0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061), (0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061, 0.0204081632653061))"
    );
}

#[test]
fn motion_blur_kernel_length_seven_horizontal_cg() {
    assert_eq!(
        eval_string(r#"stringify(motion_blur_kernel(7, 0))"#),
        "((0.142857142857143, 0, 0, 0, 0, 0, 0), (0, 0.142857142857143, 0, 0, 0, 0, 0), (0, 0, 0.142857142857143, 0, 0, 0, 0), (0, 0, 0, 0.142857142857143, 0, 0, 0), (0, 0, 0, 0, 0.142857142857143, 0, 0), (0, 0, 0, 0, 0, 0.142857142857143, 0), (0, 0, 0, 0, 0, 0, 0.142857142857143))"
    );
}

#[test]
fn unsharp_mask_kernel_half_amount_cg() {
    assert_eq!(
        eval_string(r#"stringify(unsharp_mask_kernel(0.5))"#),
        "((-0.0555555555555556, -0.0555555555555556, -0.0555555555555556), (-0.0555555555555556, 1.44444444444444, -0.0555555555555556), (-0.0555555555555556, -0.0555555555555556, -0.0555555555555556))"
    );
}

#[test]
fn emboss_kernel_three_by_three_cg() {
    assert_eq!(
        eval_string(r#"stringify(emboss_kernel())"#),
        "((-2, -1, 0), (-1, 1, 1), (0, 1, 2))"
    );
}

#[test]
fn eulerian_path_exists_on_triangle_cg() {
    assert_eq!(
        eval_string(r#"eulerian_path_q([[1, 2], [0, 2], [0, 1]])"#),
        "1"
    );
}

#[test]
fn eulerian_path_line_three_vertices_cg() {
    assert_eq!(eval_string(r#"eulerian_path_q([[1, 2], [0], [0]])"#), "1");
}

#[test]
fn hamiltonian_exists_complete_three_cg() {
    assert_eq!(
        eval_string(r#"hamiltonian_brute([[1, 2], [0, 2], [0, 1]])"#),
        "1"
    );
}

#[test]
fn graph_density_complete_three_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", graph_density([[1, 2], [0, 2], [0, 1]]))"#),
        "1.00000000000000"
    );
}

#[test]
fn graph_is_tree_path_three_cg() {
    assert_eq!(eval_string(r#"graph_is_tree([[1], [0, 2], [1]])"#), "1");
}

#[test]
fn graph_average_degree_triangle_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", graph_average_degree([[1, 2], [0, 2], [0, 1]]))"#),
        "2.00000000000000"
    );
}

#[test]
fn is_connected_two_node_edge_cg() {
    assert_eq!(eval_string(r#"is_connected([[1], [0]])"#), "1");
}

#[test]
fn graph_complement_single_edge_becomes_isolates_cg() {
    assert_eq!(
        eval_string(r#"stringify(graph_complement([[1], [0]]))"#),
        "((), ())"
    );
}

#[test]
fn connected_components_path_is_one_label_cg() {
    assert_eq!(
        eval_string(r#"stringify(connected_components([[1], [0, 2], [1]]))"#),
        "(0, 0, 0)"
    );
}

#[test]
fn dfs_preorder_star_from_center_cg() {
    assert_eq!(
        eval_string(r#"stringify(dfs_preorder([[1, 2], [0], [0]]))"#),
        "(0, 1, 2)"
    );
}

#[test]
fn bfs_distances_star_from_center_cg() {
    assert_eq!(
        eval_string(r#"stringify(bfs_distances([[1, 2], [0], [0]], 0))"#),
        "(0, 1, 1)"
    );
}

#[test]
fn in_degree_directed_three_cycle_cg() {
    assert_eq!(
        eval_string(r#"stringify(in_degree_directed([[1], [2], [0]]))"#),
        "(1, 1, 1)"
    );
}

#[test]
fn out_degree_directed_fork_cg() {
    assert_eq!(
        eval_string(r#"stringify(out_degree_directed([[1, 2], [0], [0]]))"#),
        "(2, 1, 1)"
    );
}

#[test]
fn bridges_path_middle_edge_count_two_cg() {
    assert_eq!(
        eval_string(r#"stringify(bridges_edges([[1], [0, 2], [1]]))"#),
        "((1, 2), (0, 1))"
    );
}

#[test]
fn articulation_points_triangle_empty_cg() {
    assert_eq!(
        eval_string(r#"stringify(articulation_points([[1, 2], [0, 2], [0, 1]]))"#),
        "()"
    );
}

#[test]
fn graph_max_degree_triangle_cg() {
    assert_eq!(
        eval_string(r#"graph_max_degree([[1, 2], [0, 2], [0, 1]])"#),
        "2"
    );
}

#[test]
fn graph_min_degree_triangle_cg() {
    assert_eq!(
        eval_string(r#"graph_min_degree([[1, 2], [0, 2], [0, 1]])"#),
        "2"
    );
}

#[test]
fn aes_sbox_byte_zero_cg() {
    assert_eq!(eval_string(r#"aes_sbox_byte(0)"#), "99");
}

#[test]
fn aes_inverse_sbox_byte_zero_cg() {
    assert_eq!(eval_string(r#"aes_inv_sbox_byte(0)"#), "82");
}

#[test]
fn aes_sbox_byte_sixteen_cg() {
    assert_eq!(eval_string(r#"aes_sbox_byte(16)"#), "202");
}

#[test]
fn quarter_of_year_july_third_cg() {
    assert_eq!(eval_string(r#"quarter_of_year(7)"#), "3");
}

#[test]
fn business_days_between_workweek_strip_cg() {
    assert_eq!(
        eval_string(r#"business_days_between(2026, 1, 5, 2026, 1, 9)"#),
        "4"
    );
}

#[test]
fn unix_epoch_to_iso_y2k_utc_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", unix_epoch_to_iso(946684800))"#),
        "2000-01-01T00:00:00Z"
    );
}

#[test]
fn zeller_monday_jan_first_nineteen_hundred_cg() {
    assert_eq!(eval_string(r#"zeller_day_of_week(1900, 1, 1)"#), "0");
}

#[test]
fn simon_cipher_round_known_vector_cg() {
    assert_eq!(
        eval_string(r#"stringify(simon_round(305419896, 2596069104, 286331153))"#),
        "(3879517697, 305419896)"
    );
}

#[test]
fn graph_eccentricity_path_middle_two_leaves_far_cg() {
    assert_eq!(
        eval_string(r#"stringify(graph_eccentricity_all([[1], [0, 2], [1]]))"#),
        "(2, 1, 2)"
    );
}

#[test]
fn graph_eccentricity_path_ordered_endpoints_far_one_cg() {
    assert_eq!(
        eval_string(r#"stringify(graph_eccentricity_all([[2], [1], [0]]))"#),
        "(1, 0, 1)"
    );
}

#[test]
fn eulerian_single_undirected_edge_has_trail_cg() {
    assert_eq!(eval_string(r#"eulerian_path_q([[1], [0]])"#), "1");
}

#[test]
fn bridges_edges_triangle_has_no_bridge_cg() {
    assert_eq!(
        eval_string(r#"stringify(bridges_edges([[1, 2], [0, 2], [0, 1]]))"#),
        "()"
    );
}

#[test]
fn articulation_interior_vertex_path_three_cg() {
    assert_eq!(
        eval_string(r#"articulation_points([[1], [0, 2], [1]])"#),
        "1"
    );
}

#[test]
fn graph_complement_complete_three_is_antiedge_free_cg() {
    assert_eq!(
        eval_string(r#"stringify(graph_complement([[1, 2], [0, 2], [0, 1]]))"#),
        "((), (), ())"
    );
}

#[test]
fn graph_min_degree_star_center_two_cg() {
    assert_eq!(eval_string(r#"graph_min_degree([[1, 2], [0], [0]])"#), "1");
}

#[test]
fn graph_average_degree_leaf_edge_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", graph_average_degree([[1], [0]]))"#),
        "1.00000000000000"
    );
}

#[test]
fn bfs_distances_middle_vertex_as_source_cg() {
    assert_eq!(
        eval_string(r#"stringify(bfs_distances([[1, 2], [0], [0]], 1))"#),
        "(1, 0, 2)"
    );
}

#[test]
fn palindromic_q_ab_returns_falseish_cg() {
    assert_eq!(eval_string(r#"palindromic_q("ab")"#), "0");
}

#[test]
fn string_snake_to_kebab_double_underscore_middle_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%s", string_snake_to_kebab("snake_case_here"))"#),
        "snake-case-here"
    );
}

#[test]
fn days_in_year_ord_twenty_twenty_five_cg() {
    assert_eq!(eval_string(r#"days_in_year(2025)"#), "365");
}

#[test]
fn knapsack_lp_relaxation_sums_positive_entries_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", knapsack_lp_relaxation([12, 18, 27]))"#),
        "57.00000000000000"
    );
}

#[test]
fn hamiltonian_path_exists_three_path_cg() {
    assert_eq!(eval_string(r#"hamiltonian_brute([[1], [0, 2], [1]])"#), "1");
}

#[test]
fn is_connected_disjoint_pair_of_isolates_cg() {
    assert_eq!(eval_string(r#"is_connected([[], []])"#), "0");
}

#[test]
fn graph_coloring_brooks_bound_delta_three_cg() {
    assert_eq!(
        eval_string(r#"sprintf("%.14f", graph_coloring_brooks_bound(3))"#),
        "3.00000000000000"
    );
}
