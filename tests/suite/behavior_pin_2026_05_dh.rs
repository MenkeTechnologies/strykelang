//! Behavior-pinning batch DH (2026-05): **ζ / η / prime counting**, **moving averages**, **Airy / Bessel /
//! Fresnel**, **primes & mod arithmetic**, **Bezout / Wilson / Goldbach**, **Voigt / English-likeness chi²**,
//! **matrix exponential** (2×2 rotation), **polygamma / Hurwitz ζ**, **CRT** (**two array buckets** required —
//! flat **`crt(r1,m1,r2,m2)`** is wrong — **BUG-196**), **bond convexity & modified duration**, **betweenness**,
//! **Anderson–Darling / K–S**, **Pick / shoelace**, **continued-fraction sqrt**, **hashing**, **primality /
//! Tonelli–Shanks / `mod_inv`**, **polygon centroid**, **tetrahedron volume** vs **`simplex_volume_3d([…])`**
//! (**BUG-197** — single-matrix arg does **not** delegate as four 3-vectors), **combinatorics**
//! (**`derangements`** off-by-recurrence vs subfactorial — **BUG-198**), **`hypergeometric_1f1`**, **`powmod`**.

use crate::common::*;

#[test]
fn riemann_zeta_dirichlet_eta_prime_pi_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", riemann_zeta(2))"#),
        "1.644934067"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dirichlet_eta(2))"#),
        "0.8224670334"
    );
    assert_eq!(eval_string(r#"sprintf("%d", prime_pi(100))"#), "25");
}

#[test]
fn moving_average_and_ewma_dh() {
    assert_eq!(
        eval_string(r#"stringify(moving_average(2, 1, 2, 3, 4))"#),
        "(1.5, 2.5, 3.5)"
    );
    assert_eq!(
        eval_string(r#"stringify(ewma([1, 2, 3, 4], 0.5))"#),
        "(1, 1.5, 2.25, 3.125)"
    );
}

#[test]
fn airy_bessel_fresnel_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", airy_ai(1))"#),
        "0.1352924163"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", airy_bi(1))"#),
        "1.207423595"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bessel_j0(1))"#),
        "0.7651976838"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", fresnel_c(1))"#),
        "0.7798934004"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", fresnel_s(1))"#),
        "0.4382591474"
    );
}

#[test]
fn next_prime_prev_prime_dh() {
    assert_eq!(eval_string(r#"sprintf("%d", next_prime(20))"#), "23");
    assert_eq!(eval_string(r#"sprintf("%d", prev_prime(20))"#), "19");
}

#[test]
fn bezout_wilson_goldbach_dh() {
    assert_eq!(
        eval_string(r#"stringify(bezout(240, 46))"#),
        "(2, -9, 47)"
    );
    assert_eq!(eval_string(r#"sprintf("%d", wilson_test(7))"#), "1");
    assert_eq!(eval_string(r#"sprintf("%d", wilson_test(8))"#), "0");
    assert_eq!(
        eval_string(r#"stringify(goldbach_pair(10))"#),
        "(3, 7)"
    );
}

#[test]
fn voigt_profile_and_english_likeness_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", voigt_profile(0, 1, 0.1))"#),
        "3.183098862"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", english_likeness("HELLO"))"#),
        "4.474821574"
    );
}

#[test]
fn matrix_exp_skew_two_by_two_dh() {
    assert_eq!(
        eval_string(
            r#"my $m = matrix_exp([[0, -1], [1, 0]], 1); sprintf("%.15g", $m->[0][0])"#
        ),
        "0.54030230586814"
    );
    assert_eq!(
        eval_string(
            r#"my $m = matrix_exp([[0, -1], [1, 0]], 1); sprintf("%.15g", $m->[0][1])"#
        ),
        "-0.841470984807897"
    );
}

#[test]
fn polygamma_hurwitz_zeta_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", polygamma(2, 3.5))"#),
        "-0.1082030576"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", hurwitz_zeta(2, 1.5))"#),
        "0.9348022005"
    );
}

#[test]
fn chinese_remainder_buckets_vs_flat_scalars_bug196_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%d", crt([2, 3], [5, 7]))"#),
        "17"
    );
    assert_eq!(
        eval_string(r#"sprintf("%d", crt(2, 5, 3, 7))"#),
        "2"
    );
}

#[test]
fn bond_convexity_modified_duration_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", convexity_bond(100, 0.08, 5, 2, 0.06))"#),
        "6.392000829"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", modified_duration_bond(100, 0.08, 5, 2, 0.06))"#
        ),
        "2.252095211"
    );
}

#[test]
fn betweenness_path_graph_dh() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", betweenness_centrality([[0, 1, 0], [1, 0, 1], [0, 1, 0]]))"#
        ),
        "0"
    );
}

#[test]
fn ks_anderson_samples_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ks_test([1, 2, 3], [1, 2, 4]))"#),
        "0.3333333333"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", anderson_darling([0.1, 0.2, 0.3, 0.4]))"#),
        "0.1592009364"
    );
}

#[test]
fn picks_theorem_shoelace_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", picks_theorem(5, 3, 0))"#),
        "5.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", shoelace_area([[0, 0], [4, 0], [0, 3]]))"#),
        "6"
    );
}

#[test]
fn continued_fraction_sqrt7_dh() {
    assert_eq!(
        eval_string(r#"stringify(continued_fraction_sqrt(7, 5))"#),
        "(2, 1, 1, 1, 4)"
    );
}

#[test]
fn derangements_stirling_bernoulli_harmonic_bug198_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%d", derangements(4))"#),
        "36"
    );
    assert_eq!(eval_string(r#"sprintf("%d", stirling2(5, 3))"#), "25");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bernoulli(4))"#),
        "-0.03333333333"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", harmonic(10))"#),
        "2.928968254"
    );
}

#[test]
fn powmod_hypergeometric_1f1_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%d", powmod(2, 10, 1000))"#),
        "24"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", hypergeometric_1f1(1, 2, 0.5))"#),
        "1.297442541"
    );
}

#[test]
fn crc32_murmur3_popcount_dh() {
    assert_eq!(
        eval_string(r#"sprintf("%u", crc32("hello"))"#),
        "907060870"
    );
    assert_eq!(
        eval_string(r#"sprintf("%u", murmur3("hello", 42))"#),
        "0"
    );
    assert_eq!(eval_string(r#"sprintf("%d", popcount(255))"#), "8");
}

#[test]
fn miller_rabin_tonelli_modinv_dh() {
    assert_eq!(eval_string(r#"sprintf("%d", miller_rabin(15))"#), "0");
    assert_eq!(eval_string(r#"sprintf("%d", miller_rabin(17))"#), "1");
    assert_eq!(eval_string(r#"sprintf("%d", tonelli_shanks(5, 11))"#), "4");
    assert_eq!(eval_string(r#"sprintf("%d", mod_inv(3, 11))"#), "4");
}

#[test]
fn polygon_centroid_triangle_dh() {
    assert_eq!(
        eval_string(r#"stringify(polygon_centroid([[0, 0], [6, 0], [0, 9]]))"#),
        "(2, 3)"
    );
}

#[test]
fn tetrahedron_volume_unit_simplex_dh() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.15g", tetrahedron_volume([0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1]))"#
        ),
        "0.166666666666667"
    );
}

#[test]
fn simplex_volume_3d_matrix_arg_yields_zero_bug197_dh() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", simplex_volume_3d([[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1]]))"#
        ),
        "0"
    );
}
