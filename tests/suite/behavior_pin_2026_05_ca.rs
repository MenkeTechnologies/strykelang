//! Behavior-pinning batch CA (2026-05-09): ML classification metrics (`confusion_counts`, `mcc`,
//! `balanced_accuracy`, `specificity`, `cohen_kappa`), losses (`brier_score`, `hinge_loss`, `log_loss`,
//! `logistic_loss`, `adaboost_alpha`, `f_beta`), Lorenz tooling, APL-ish `reshape_array`/`grade_*`.
//! Extend **BUG-126**: `lorenz_curve_points`, `grade_up`, `grade_down` read only `args.first()`.

use crate::common::*;

#[test]
fn confusion_counts_binary_lists_ca() {
    assert_eq!(
        eval_string(r#"stringify(confusion_counts([1, 0, 1, 1], [1, 1, 0, 1]))"#),
        "(2, 1, 0, 1)"
    );
}

#[test]
fn mcc_from_confusion_matrix_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", mcc([1, 0, 1, 1], [1, 1, 0, 1]))"#),
        "-0.333333333333333"
    );
}

#[test]
fn balanced_accuracy_sensitivity_specificity_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", balanced_accuracy([1, 0, 1, 1], [1, 1, 0, 1]))"#),
        "0.333333333333333"
    );
}

#[test]
fn specificity_tn_fp_ratio_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", specificity([1, 0, 1, 1], [1, 1, 0, 1]))"#),
        "0.000000000000000"
    );
}

#[test]
fn cohen_kappa_perfect_agreement_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", cohen_kappa([1, 1, 0, 0], [1, 1, 0, 0]))"#),
        "1.000000000000000"
    );
}

#[test]
fn brier_score_mean_squared_residual_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", brier_score([0, 1], [0.1, 0.9]))"#),
        "0.010000000000"
    );
}

#[test]
fn hinge_loss_positive_class_negative_margin_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", hinge_loss(1, -0.25))"#),
        "1.250000000000"
    );
}

#[test]
fn f_beta_given_precision_and_recall_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", f_beta(0.8, 0.6, 2))"#),
        "0.631578947368"
    );
}

#[test]
fn adaboost_alpha_half_error_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", adaboost_alpha(0.5))"#),
        "0.000000000000"
    );
}

#[test]
fn log_loss_two_labels_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", log_loss([1, 0], [0.731, 0.259]))"#),
        "0.306548236459"
    );
}

#[test]
fn logistic_loss_logit_origin_ca() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", logistic_loss(1, 0))"#),
        "0.693147180560"
    );
}

#[test]
fn reshape_array_two_by_two_ca() {
    assert_eq!(
        eval_string(r#"stringify(reshape_array(2, 2, [1, 2, 3, 4]))"#),
        "((1, 2), (3, 4))"
    );
}

#[test]
fn lorenz_curve_points_sorted_three_in_array_ca() {
    assert_eq!(
        eval_string(r#"stringify(lorenz_curve_points([1, 2, 3]))"#),
        "((0, 0), (0.333333333333333, 0.166666666666667), (0.666666666666667, 0.5), (1, 1))"
    );
}

// BUG-126 tail-drop: incomes after the first comma are ignored.
#[test]
fn lorenz_curve_points_variadic_truncated_tail_ca() {
    assert_eq!(
        eval_string(r#"stringify(lorenz_curve_points(1, 2, 3))"#),
        "((0, 0), (1, 1))"
    );
}

#[test]
fn grade_up_permutation_three_ca() {
    assert_eq!(
        eval_string(r#"stringify(grade_up([3, 1, 2]))"#),
        "(1, 2, 0)"
    );
}

#[test]
fn grade_up_variadic_first_element_only_ca() {
    assert_eq!(eval_string(r#"stringify(grade_up(3, 1, 2))"#), "0");
}

#[test]
fn grade_down_permutation_three_ca() {
    assert_eq!(
        eval_string(r#"stringify(grade_down([1, -1, 0]))"#),
        "(0, 2, 1)"
    );
}

#[test]
fn grade_down_variadic_first_scalar_only_ca() {
    assert_eq!(eval_string(r#"stringify(grade_down(1, -1, 0))"#), "0");
}
