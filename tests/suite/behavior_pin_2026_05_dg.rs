//! Behavior-pinning batch DG (2026-05): **Black–Scholes** (implementation order **`S, K, T, r, σ`** vs IDE docs
//! **`S, K, r, T, σ`** — **BUG-193**), **string metrics**, **physics / fluids**, **special functions**, **planar / geodesic
//! geometry**, **Greeks & duration**, **distribution CDFs**, **Romberg** (full **`romberg`** vs combine-only **`romberg_quad`**
//! — **BUG-195**), **`fixed_quad`**, **EM clustering**, **`kmeans`**, **MCC / confusion**, **log-sum-exp**, **correlation
//! matrix from covariance**, **norms**, **additive number theory**, **combinatorial counts**, **Gram–Schmidt**, **vector
//! distances**, **ODE one-step**, **`hamming_distance`** on **`ARRAY` refs (**`ARRAY(0x…)`** collapse)** — **BUG-194**.

use crate::common::*;

#[test]
fn black_scholes_call_put_spot_strike_time_rate_vol_bug193_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", black_scholes_call(100, 100, 1, 0.05, 0.2))"#),
        "10.45058357"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", black_scholes_put(100, 100, 1, 0.05, 0.2))"#),
        "5.573526022"
    );
}

#[test]
fn bscall_doc_order_swaps_time_and_rate_bug193_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bscall(100, 100, 0.05, 1, 0.2))"#),
        "5.165791121"
    );
}

#[test]
fn hamming_distance_strings_vs_arrayrefs_bug194_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%d", hamming_distance("101", "110"))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%d", hamming_distance([1, 0, 1], [1, 1, 0]))"#),
        "0"
    );
}

#[test]
fn jaro_jaro_winkler_levenshtein_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaro_similarity("martha", "marhta"))"#),
        "0.9444444444"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaro_winkler("martha", "marhta"))"#),
        "0.9611111111"
    );
    assert_eq!(
        eval_string(r#"sprintf("%d", levenshtein("kitten", "sitting"))"#),
        "3"
    );
}

#[test]
fn kinetic_ideal_gas_snell_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kinetic_energy(10, 5))"#),
        "125"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ideal_gas_pressure(1, 300, 0.082))"#),
        "0.002272619782"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", snells_law(1, 1.5, 30))"#),
        "19.47122063"
    );
}

#[test]
fn hypergeom_2f1_elliptic_e_k_dilog_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", hypergeometric_2f1(0.5, 1, 1.5, 0.5))"#),
        "1.24645048"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", elliptic_e(0.5))"#),
        "1.350643881"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", elliptic_k(0.5))"#),
        "1.854074677"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dilog(0.5))"#),
        "0.5822405265"
    );
}

#[test]
fn convex_hull_polygon_triangle_dg() {
    assert_eq!(
        eval_string(
            r#"stringify(convex_hull_2d([[0, 0], [1, 0], [0, 1], [0.5, 0.5]]))"#
        ),
        "((0, 0), (1, 0), (0, 1))"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", polygon_area([[0, 0], [1, 0], [0, 1]]))"#),
        "0.5"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", triangle_area_heron(3, 4, 5))"#), "6");
}

#[test]
fn vincenty_great_circle_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", vincenty_distance(0, 0, 0, 1))"#),
        "111319.4908"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", great_circle_law_of_cos(0, 0, 0, 90))"#),
        "10007543.4"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", great_circle_bearing(0, 0, 0, 90))"#),
        "90"
    );
}

#[test]
fn bs_delta_gamma_macaulay_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bs_delta(100, 100, 1, 0.05, 0.2))"#),
        "0.636830586"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bs_gamma(100, 100, 1, 0.05, 0.2))"#),
        "0.01876201735"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", macaulay_duration(0.08, 5, 0.06))"#),
        "1"
    );
}

#[test]
fn reynolds_prandtl_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", reynolds_number(1.2, 0.01, 1000, 1e-3))"#),
        "12000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", prandtl_number(0.025, 1005, 0.03))"#),
        "837.5"
    );
}

#[test]
fn romberg_integrate_vs_quad_combine_bug195_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", romberg(sub { $_[0] * $_[0] }, 0, 1, 5))"#),
        "0.3333333333"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", romberg_quad(1, 0.5, 1))"#),
        "1.166666667"
    );
}

#[test]
fn fixed_quad_weighted_sum_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", fixed_quad([0.25, 0.25, 0.25, 0.25], [1, 1, 1, 1]))"#),
        "1"
    );
}

#[test]
fn ppois_pexp_pgamma_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ppois(3, 2))"#),
        "0.8571234605"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pexp(1, 1))"#),
        "0.6321205588"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pgamma(2, 3, 1))"#),
        "0.3233235838"
    );
}

#[test]
fn laplace_rayleigh_pdf_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", laplace_pdf(0, 0, 1))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", rayleigh_pdf(1, 1))"#),
        "0.6065306597"
    );
}

#[test]
fn gmm_em_1d_gaussian_mix_dg() {
    assert_eq!(
        eval_string(
            r#"my $g = gmm_em_1d([1, 2, 3, 2, 1], 2, 10); sprintf("%.15g", $g->[0][0])"#
        ),
        "0.441638710566797"
    );
    assert_eq!(
        eval_string(
            r#"my $g = gmm_em_1d([1, 2, 3, 2, 1], 2, 10); sprintf("%.15g", $g->[1][0])"#
        ),
        "1.15525790328457"
    );
}

#[test]
fn kmeans_two_clusters_labels_dg() {
    assert_eq!(
        eval_string(r#"stringify(kmeans([[0, 0], [5, 5], [1, 1]], 2, 5))"#),
        "(0, 1, 0)"
    );
}

#[test]
fn mcc_confusion_counts_binary_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mcc([1, 1, 0, 0], [1, 0, 0, 1]))"#),
        "0"
    );
    assert_eq!(
        eval_string(r#"stringify(confusion_counts([1, 1, 0, 0], [1, 0, 0, 1]))"#),
        "(1, 1, 1, 1)"
    );
}

#[test]
fn log_sum_exp_stable_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", log_sum_exp([1, 2, 3]))"#),
        "3.407605964"
    );
}

#[test]
fn horner_quadratic_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", horner([1, 2, 3], 2))"#),
        "17"
    );
}

#[test]
fn cov2cor_frobenius_norm_string_dg() {
    assert_eq!(
        eval_string(r#"stringify(cov2cor([[4, 2], [2, 9]]))"#),
        "((1, 0.333333333333333), (0.333333333333333, 1))"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_norm([[1, 2], [3, 4]], "fro"))"#),
        "5.477225575"
    );
}

#[test]
fn euler_totient_mobius_divisor_sum_dg() {
    assert_eq!(eval_string(r#"sprintf("%d", euler_totient(12))"#), "4");
    assert_eq!(eval_string(r#"sprintf("%d", mobius(30))"#), "-1");
    assert_eq!(eval_string(r#"sprintf("%d", sum_divisors(12))"#), "16");
}

#[test]
fn catalan_bell_partition_fib_lucas_dg() {
    assert_eq!(eval_string(r#"sprintf("%d", catalan(4))"#), "14");
    assert_eq!(eval_string(r#"sprintf("%d", bell_number(4))"#), "15");
    assert_eq!(eval_string(r#"sprintf("%d", partition_number(5))"#), "7");
    assert_eq!(eval_string(r#"sprintf("%d", fibonacci(10))"#), "55");
    assert_eq!(eval_string(r#"sprintf("%d", lucas(10))"#), "123");
}

#[test]
fn gram_schmidt_orthonormal_first_axis_dg() {
    assert_eq!(
        eval_string(
            r#"my $b = gram_schmidt([[1, 1, 0], [1, 0, 1], [0, 1, 1]]); sprintf("%.15g", $b->[0][0])"#
        ),
        "0.707106781186547"
    );
}

#[test]
fn vector_distances_mahalanobis_row_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", manhattan_distance([1, 2, 3], [2, 3, 5]))"#),
        "4"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", minkowski_distance([0, 0], [3, 4], 2))"#),
        "5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chebyshev_distance(0, 0, 3, 5))"#),
        "5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", canberra_distance([1, 2, 3], [2, 3, 4]))"#),
        "0.6761904762"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", mahalanobis([[2, 3]], [0, 0], [[1, 0], [0, 1]]))"#
        ),
        "3.605551275"
    );
}

#[test]
fn ode45_runge_kutta_single_step_dg() {
    assert_eq!(
        eval_string(r#"sprintf("%.15g", ode45_step(1, 0.5, 1, 1, 1, 1))"#),
        "1.5"
    );
}
