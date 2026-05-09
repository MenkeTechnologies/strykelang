//! Behavior-pinning batch DF (2026-05): **Gamma / polygamma** (`lgamma`, `tgamma`, `digamma`, `trigamma`), **basic
//! discrete & continuous PMFs** (`dnbinom`, `dgeom`, `dunif`), **sigmoid ↔ logit** (`sigmoid`, `sigmoid_inverse`),
//! **χ² & F** (`pchisq`, `pf`, `dchisq`), **`multinomial` coefficient**, **matrix decompositions** (`matrix_rank`,
//! `matrix_pinv`, `qr_decompose`, `matrix_eigenvalues`), **numerical differentiation** (`numerical_jacobian`,
//! `numerical_hessian`), **Weibull / lognormal**, **Kaplan–Meier**, **classifier scores** (`brier_score`, `roc_auc`),
//! **transport / geo / f-divergence samples** (`wasserstein_1d`, `haversine`, `hellinger_distance`,
//! `bhattacharyya_coefficient`), **losses** (`huber_loss`, `hinge_loss`), **clustering** (`silhouette_score`,
//! `davies_bouldin_index`, `dbscan`), **range interpolation** (`lerp`, `inv_lerp`, `smoothstep`, `remap` — **`lerp`**
//! is **`(A, B, T)`** not GLSL **`(T, A, B)`** — **BUG-192**), **association** (`concordance_correlation`,
//! `kendall_tau`), **inverse hyperbolic & `sinc`**, **Lambert W** (adjacent to **`1`**; exact **`1`** remains
//! **NaN** per **BUG-128**), **quadrature & ODE & root-finding** (`boole_rule`, `gauss_legendre_5`, `midpoint_rule`,
//! `rk4`, `euler_ode`, `brent_root`).

use crate::common::*;

#[test]
fn gamma_polygamma_family_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lgamma(5))"#),
        "3.17805383"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", tgamma(5))"#), "24");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", digamma(5))"#),
        "1.506117668"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", trigamma(5))"#),
        "0.2203274357"
    );
}

#[test]
fn dnbinom_dgeom_dunif_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dnbinom(3, 2, 0.5))"#),
        "0.125"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dgeom(2, 0.5))"#),
        "0.125"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dunif(0.5, 0, 1))"#),
        "1"
    );
}

#[test]
fn sigmoid_logit_inverse_df() {
    assert_eq!(eval_string(r#"sprintf("%.10g", sigmoid(0))"#), "0.5");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", sigmoid_inverse(0.25))"#),
        "-1.098612289"
    );
}

#[test]
fn pchisq_pf_dchisq_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pchisq(3.84, 1))"#),
        "0.9499564788"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pf(4.0, 2, 10))"#),
        "0.9470777142"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dchisq(1, 3))"#),
        "0.2419707245"
    );
}

#[test]
fn multinomial_coefficient_df() {
    assert_eq!(eval_string(r#"sprintf("%d", multinomial(5, 2, 3))"#), "10");
}

#[test]
fn matrix_rank_singular_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_rank([[1, 2], [2, 4]]))"#),
        "1"
    );
}

#[test]
fn matrix_pinv_qr_eigen_df() {
    assert_eq!(
        eval_string(
            r#"my $p = matrix_pinv([[1, 2], [3, 4]]); sprintf("%.15g", $p->[0][0])"#
        ),
        "-2"
    );
    assert_eq!(
        eval_string(
            r#"my $qr = qr_decompose([[1, 2], [3, 4]]); sprintf("%.15g", $qr->[0][0][0])"#
        ),
        "0.316227766016838"
    );
    assert_eq!(
        eval_string(r#"stringify(matrix_eigenvalues([[2, 1], [1, 2]]))"#),
        "(3, 1)"
    );
}

#[test]
fn numerical_jacobian_square_map_df() {
    assert_eq!(
        eval_string(
            r#"my $j = numerical_jacobian(sub { my $a = $_[0]; my @y = @$a; ($y[0], $y[1] * 2) }, [1, 2]); sprintf("%.10g", $j->[0][0])"#
        ),
        "1"
    );
    assert_eq!(
        eval_string(
            r#"my $j = numerical_jacobian(sub { my $a = $_[0]; my @y = @$a; ($y[0], $y[1] * 2) }, [1, 2]); sprintf("%.10g", $j->[1][1])"#
        ),
        "2"
    );
}

#[test]
fn numerical_hessian_mixed_quadratic_df() {
    assert_eq!(
        eval_string(
            r#"my $h = numerical_hessian(sub { my $a = $_[0]; my @y = @$a; $y[0]**2 * $y[1] }, [1, 2]); sprintf("%.10g", $h->[0][0])"#
        ),
        "3.999689469"
    );
    assert_eq!(
        eval_string(
            r#"my $h = numerical_hessian(sub { my $a = $_[0]; my @y = @$a; $y[0]**2 * $y[1] }, [1, 2]); sprintf("%.10g", $h->[0][1])"#
        ),
        "1.999983512"
    );
    assert_eq!(
        eval_string(
            r#"my $h = numerical_hessian(sub { my $a = $_[0]; my @y = @$a; $y[0]**2 * $y[1] }, [1, 2]); sprintf("%.10g", $h->[1][1])"#
        ),
        "-5.551115123e-05"
    );
}

#[test]
fn weibull_lognormal_pweibull_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", weibull_pdf(1, 1.5, 1))"#),
        "0.5518191618"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pweibull(1, 1.5, 1))"#),
        "0.6321205588"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lognormal_pdf(1, 0, 1))"#),
        "0.3989422804"
    );
}

#[test]
fn kaplan_meier_curve_df() {
    assert_eq!(
        eval_string(r#"stringify(kaplan_meier([0, 1, 2], [1, 1, 0]))"#),
        "((0, 0.666666666666667), (1, 0.333333333333333))"
    );
}

#[test]
fn brier_roc_auc_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", brier_score([0.1, 0.9], [0, 1]))"#),
        "0.01"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", roc_auc([0, 0, 1, 1], [0.1, 0.4, 0.35, 0.8]))"#),
        "0.5"
    );
}

#[test]
fn wasserstein_haversine_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", wasserstein_1d([0, 2, 4], [1, 3]))"#),
        "1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", haversine(0, 0, 0, 1))"#),
        "111.1949266"
    );
}

#[test]
fn hellinger_bhattacharyya_df() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", hellinger_distance([0.5, 0.5], [0.25, 0.75]))"#
        ),
        "0.1845919113"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", bhattacharyya_coefficient([0.5, 0.5], [0.25, 0.75]))"#
        ),
        "0.9659258263"
    );
}

#[test]
fn huber_hinge_loss_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", huber_loss(0.5, 1))"#),
        "0.125"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", hinge_loss(0.5, 1))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", hinge_loss(-0.5, 1))"#),
        "1.5"
    );
}

#[test]
fn silhouette_davies_dbscan_df() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", silhouette_score([[0, 0], [5, 5], [0.1, 0.1], [5.1, 5.1]], [0, 0, 1, 1]))"#
        ),
        "-0.49"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", davies_bouldin_index([[0, 0], [5, 5], [0.1, 0.1]], [0, 0, 1]))"#
        ),
        "1.041666667"
    );
    assert_eq!(
        eval_string(r#"stringify(dbscan([[0, 0], [0.1, 0.1], [5, 5]], 0.5, 2))"#),
        "(0, 0, -1)"
    );
}

#[test]
fn lerp_inv_lerp_smoothstep_remap_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lerp(10, 20, 0.5))"#),
        "15"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", inv_lerp(10, 20, 15))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", smoothstep(0, 1, 0.5))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", remap(5, 0, 10, 100, 200))"#),
        "150"
    );
}

#[test]
fn lerp_shader_style_args_numify_to_giant_bug192_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lerp(0.5, 10, 20))"#),
        "190.5"
    );
}

#[test]
fn concordance_kendall_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", concordance_correlation([1, 2, 3], [2, 3, 4]))"#),
        "0.5714285714"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kendall_tau([1, 2, 3], [1, 3, 2]))"#),
        "0.3333333333"
    );
}

#[test]
fn inverse_hyperbolic_sinc_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", acosh(2))"#),
        "1.316957897"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", asinh(1))"#),
        "0.881373587"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", atanh(0.5))"#),
        "0.5493061443"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", sinc(3.14159265358979))"#),
        "1.028487619e-15"
    );
}

#[test]
fn lambert_w0_adjacent_to_one_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lambert_w0(1.0000000001))"#),
        "0.5671432904"
    );
}

#[test]
fn quadrature_x_squared_rules_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", boole_rule(sub { $_[0] * $_[0] }, 0, 1, 4))"#),
        "0.3333333333"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gauss_legendre_5(sub { $_[0] * $_[0] }, -1, 1))"#),
        "0.6666666667"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", midpoint_rule(sub { $_[0] * $_[0] }, 0, 1, 4))"#),
        "0.328125"
    );
}

#[test]
fn rk4_euler_exponential_ivp_df() {
    assert_eq!(
        eval_string(
            r#"my @r = rk4(sub { my ($t, $y) = @_; $y }, 0, 1, 1, 4); sprintf("%.15g", $r[4][1])"#
        ),
        "53.8032437548225"
    );
    assert_eq!(
        eval_string(
            r#"my @r = euler_ode(sub { my ($t, $y) = @_; $y }, 0, 1, 1, 4); sprintf("%.15g", $r[4][1])"#
        ),
        "16"
    );
}

#[test]
fn brent_root_sqrt_two_df() {
    assert_eq!(
        eval_string(r#"sprintf("%.15g", brent_root(sub { $_[0] * $_[0] - 2 }, 0, 2))"#),
        "1.41421356237314"
    );
}
