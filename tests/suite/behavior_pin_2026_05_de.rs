//! Behavior-pinning batch DE (2026-05): **distributions** (`bb_pmf`, `dhyper`, `qbinom`, **`rbinom`**
//! arity — **BUG-190**), **window vectors** (`blackman`, `hann`, `hamming`), **information / divergence**
//! (`entropy`, `kl_divergence`, `js_divergence`, **`mutual_information` / `mi`** extra operand — **BUG-126**),
//! **model selection** (`aic`, `bic`), **distances** (`total_variation_distance`), **combinatorics**
//! (`permutation_parity`), **assignment** (`hungarian_assignment`), **graphs** (`modularity_q`, `pagerank`,
//! `degree_centrality`, `closeness_centrality`, `eigenvector_centrality`), **inequality** (`gini_coefficient`,
//! `theil_index`), **k-means seeding** (`kmeans_pp_init`), **quadrature** (`simpson`), **correlation**
//! (`correlation`, `spearman`), **moments** (`skewness`, `kurtosis`, `bowley_skewness`), **means**
//! (`harmonic_mean`, `geometric_mean`), **`numerical_gradient` / `ngrad`** list-assignment footgun — **BUG-191**,
//! **`covariance`**, **`binom_test`**, **softmax** / **one_hot**, **Poisson** / **Exponential**, **`linear_regression`**,
//! **matrix** summaries (**`matrix_det`**, **`matrix_trace`**, **`frobenius_norm`**), **`dot_product`**, **`cosine_similarity`**.

use crate::common::*;

#[test]
fn beta_binomial_and_hypergeometric_pins_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bb_pmf(2, 5, 2, 3))"#),
        "0.2380952381"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", beta_binomial_pmf(2, 5, 2, 3))"#),
        "0.2380952381"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dhyper(1, 2, 2, 3))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dhyper(2, 5, 5, 8))"#),
        "0"
    );
}

#[test]
fn qbinom_median_de() {
    assert_eq!(
        eval_string(r#"sprintf("%d", qbinom(0.5, 10, 0.5))"#),
        "5"
    );
}

#[test]
fn rbinom_two_arg_interprets_prob_as_size_bug190_de() {
    assert_eq!(
        eval_string(r#"stringify(rbinom(4, 0.5))"#),
        "(0, 0, 0, 0)"
    );
}

#[test]
fn blackman_hann_hamming_window_samples_de() {
    assert_eq!(
        eval_string(r#"my $b = blackman(4); sprintf("%.10g", $b->[1])"#),
        "0.63"
    );
    assert_eq!(
        eval_string(r#"my $h = hann(4); sprintf("%.10g", $h->[1])"#),
        "0.75"
    );
    assert_eq!(
        eval_string(r#"my $w = hamming(5); sprintf("%.10g", $w->[2])"#),
        "1"
    );
}

#[test]
fn entropy_kl_js_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", entropy([0.5, 0.5]))"#),
        "0.6931471806"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kl_divergence([0.5, 0.5], [0.6, 0.4]))"#),
        "0.02041099726"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", js_divergence([0.5, 0.5], [0.6, 0.4]))"#),
        "0.005059389929"
    );
}

#[test]
fn mutual_information_flat_list_joint_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mutual_information([0, 1, 0, 1]))"#),
        "-1.386294361"
    );
}

#[test]
fn mutual_information_two_by_two_matrix_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mutual_information([[0.25, 0], [0, 0.25]]))"#),
        "0.6931471806"
    );
}

#[test]
fn mutual_information_second_operand_silent_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mutual_information([0, 1, 0, 1], [9, 9, 9, 9]))"#),
        "-1.386294361"
    );
}

#[test]
fn aic_bic_de() {
    assert_eq!(eval_string(r#"sprintf("%.10g", aic(100, 5))"#), "190");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bic(100, 5, 3))"#),
        "154.9437912"
    );
}

#[test]
fn total_variation_permutation_parity_de() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", total_variation_distance([0.5, 0.5], [0.6, 0.4]))"#
        ),
        "0.1"
    );
    assert_eq!(eval_string(r#"sprintf("%d", permutation_parity([1, 0, 2]))"#), "-1");
    assert_eq!(eval_string(r#"sprintf("%d", permutation_parity([2, 0, 1]))"#), "1");
}

#[test]
fn hungarian_assignment_small_matrix_de() {
    assert_eq!(
        eval_string(r#"stringify(hungarian_assignment([[1, 2], [3, 1]]))"#),
        "((0, 1), 2)"
    );
}

#[test]
fn modularity_q_two_clique_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", modularity_q([[0, 1], [1, 0]], [0, 0]))"#),
        "0"
    );
}

#[test]
fn graph_centralities_triangle_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pagerank([[0, 1], [1, 0]], 0.85))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", degree_centrality([[0, 1, 1], [1, 0, 1], [1, 1, 0]]))"#
        ),
        "1.5"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", closeness_centrality([[0, 1, 0], [1, 0, 1], [0, 1, 0]]))"#
        ),
        "1"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", eigenvector_centrality([[0, 1, 1], [1, 0, 1], [1, 1, 0]]))"#
        ),
        "0.5773502692"
    );
}

#[test]
fn gini_theil_kmeans_pp_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gini_coefficient([1, 2, 3, 4]))"#),
        "0.25"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", theil_index([1, 2, 3, 4]))"#),
        "0.1064401353"
    );
    assert_eq!(
        eval_string(r#"stringify(kmeans_pp_init([[0, 0], [5, 5], [1, 1]], 2))"#),
        "((1, 1), (5, 5))"
    );
}

#[test]
fn simpson_even_spacing_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", simpson([0, 1, 4], 1))"#),
        "2.666666667"
    );
}

#[test]
fn correlation_spearman_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", correlation([1, 2, 3], [2, 4, 7]))"#),
        "0.9933992678"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", spearman([1, 2, 3], [3, 2, 1]))"#),
        "-1"
    );
}

#[test]
fn skewness_kurtosis_bowley_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", skewness([1, 2, 3, 4, 5]))"#),
        "0"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kurtosis([1, 2, 3, 4, 5]))"#),
        "-1.3"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bowley_skewness([1, 2, 3, 4, 5, 6]))"#),
        "-0.3333333333"
    );
}

#[test]
fn harmonic_geometric_mean_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", harmonic_mean([1, 2, 4]))"#),
        "1.714285714"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", geometric_mean([1, 2, 4]))"#),
        "2"
    );
}

#[test]
fn covariance_binom_test_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", covariance([1, 2, 3], [2, 4, 6]))"#),
        "2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", binom_test(5, 10, 0.5))"#),
        "1"
    );
}

#[test]
fn numerical_gradient_arrayref_callback_de() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.15g", (numerical_gradient(sub { my $a = $_[0]; my @y = @$a; $y[0]**2 + 2 * $y[1]**2 }, [3, 4]))[0])"#
        ),
        "6.00000000024655"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.15g", (numerical_gradient(sub { my $a = $_[0]; my @y = @$a; $y[0]**2 + 2 * $y[1]**2 }, [3, 4]))[1])"#
        ),
        "16.0000000004601"
    );
}

#[test]
fn numerical_gradient_my_x_at_wrong_grad_bug191_de() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", numerical_gradient(sub { my ($x) = @_; $x * $x }, [3]))"#
        ),
        "0"
    );
}

#[test]
fn softmax_one_hot_poisson_exponential_de() {
    assert_eq!(
        eval_string(r#"stringify(softmax([1, 2, 3]))"#),
        "(0.0900305731703805, 0.244728471054798, 0.665240955774822)"
    );
    assert_eq!(
        eval_string(r#"stringify(one_hot(2, 5))"#),
        "(0, 0, 1, 0, 0)"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", poisson_pmf(3, 2))"#),
        "0.1804470443"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", exponential_pdf(0, 1))"#),
        "1"
    );
}

#[test]
fn linear_regression_perfect_collinearity_de() {
    assert_eq!(
        eval_string(r#"stringify(linear_regression([1, 2, 3], [2, 4, 6]))"#),
        "(2, 0, 1)"
    );
}

#[test]
fn matrix_det_trace_frobenius_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_det([[1, 2], [3, 4]]))"#),
        "-2"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", matrix_trace([[1, 2], [3, 4]]))"#),
        "5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", frobenius_norm([[1, 2], [3, 4]]))"#),
        "5.477225575"
    );
}

#[test]
fn dot_cosine_de() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dot_product([1, 2, 3], [4, 5, 6]))"#),
        "32"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cosine_similarity([1, 2], [2, 4]))"#),
        "1"
    );
}