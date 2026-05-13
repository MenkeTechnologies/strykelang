//! Behavior-pinning batch CW (2026-05-09): **classification losses & probabilities** (**`categorical_cross_entropy`** /
//! **`cce`**, **`softmax`**, **`log_softmax`**, **`log_loss`**), **sorting & top‑k** (**`sorted_desc`**, **`sorted_nums`**,
//! **`argsort`**, **`topk_indices`**, **`vec_topk`**, **`nth_largest`**, **`nth_smallest`**, **`ml_topk_argmax`**),
//! **set / similarity helpers** (**`jaccard_index`**, **`jaccard_similarity`** — multiset **string-set** collapse — **BUG-172**),
//! **scaling & info** (**`minmax_scale`**, **`zscore_norm`**, scalar **`zscore(X, POP)`**, **`kl_divergence`**, **`js_divergence`**,
//! **`mutual_information`**, **`cross_entropy_arr`**, **`robust_scale`**), **metrics** (**`balanced_accuracy`**, **`specificity`**,
//! **`confusion_counts`**, **`cohen_kappa`**, **`brier_score`**, **`roc_auc`**), **ML activations**, **geometry &
//! misc** (**`angle_between`**, **`tversky`**, **`iou_2d_axis_aligned`**, **`matrix_multiply`**, **`gini`**, **`cartesian_product`**,
//! **`cummax`/`cummin`/`diff`**, **`one_hot`**, **`mode`** — single **`ARRAYREF`** atom — **BUG-173**), **`geohash_encode`**.

use crate::common::*;

#[test]
fn categorical_cross_entropy_matches_cce_alias_cw() {
    let exp = "0.5364793041";
    assert_eq!(
        eval_string(r#"sprintf("%.10g", categorical_cross_entropy([0, 1, 0], [0.7, 0.2, 0.1]))"#),
        exp
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cce([0, 1, 0], [0.7, 0.2, 0.1]))"#),
        exp
    );
}

#[test]
fn softmax_two_values_cw() {
    assert_eq!(
        eval_string(r#"stringify(softmax([0, 1]))"#),
        "[0.268941421369995, 0.731058578630005]"
    );
}

#[test]
fn log_softmax_two_values_cw() {
    assert_eq!(
        eval_string(r#"stringify(log_softmax([0, 1]))"#),
        "[-1.31326168751822, -0.313261687518223]"
    );
}

#[test]
fn log_loss_averaged_two_points_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", log_loss([1, 0], [0.8, 0.2]))"#),
        "0.2231435513"
    );
}

#[test]
fn sorted_desc_numeric_list_cw() {
    assert_eq!(
        eval_string(r#"stringify(sorted_desc(5, 1, 9, 2))"#),
        "(9, 5, 2, 1)"
    );
}

#[test]
fn sorted_nums_numeric_not_lexicographic_cw() {
    assert_eq!(
        eval_string(r#"stringify(sorted_nums(10, 2, 1))"#),
        "(1, 2, 10)"
    );
}

#[test]
fn topk_indices_tuple_stringify_cw() {
    assert_eq!(
        eval_string(r#"stringify(topk_indices([5, 1, 9, 2], 2))"#),
        "(2, 0)"
    );
}

#[test]
fn vec_topk_square_bracket_stringify_cw() {
    assert_eq!(
        eval_string(r#"stringify(vec_topk([5, 1, 9, 2], 2))"#),
        "[2, 0]"
    );
}

#[test]
fn ml_topk_argmax_index_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_topk_argmax([5, 1, 9, 2], 2))"#),
        "2"
    );
}

#[test]
fn nth_largest_smallest_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", nth_largest([5, 1, 9, 2], 1))"#),
        "9"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", nth_smallest([30, 10, 20], 2))"#),
        "20"
    );
}

#[test]
fn argsort_stable_order_cw() {
    assert_eq!(
        eval_string(r#"stringify(argsort([30, 10, 20]))"#),
        "(1, 2, 0)"
    );
}

/// Scalar **TP, FP, FN, α, β**; not multiset Jaccard over two vectors.
#[test]
fn tversky_scalar_counts_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", tversky(1, 1, 1, 0.5, 0.5))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", tversky(2, 1, 1, 1, 1))"#),
        "0.5"
    );
}

#[test]
fn jaccard_index_unique_elements_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaccard_index([1, 2, 3], [2, 3, 4]))"#),
        "0.5"
    );
}

#[test]
fn jaccard_similarity_unique_elements_matches_index_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaccard_similarity([1, 2, 3], [2, 3, 4]))"#),
        "0.5"
    );
}

#[test]
fn dice_coefficient_two_sets_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dice_coefficient([1, 2, 3], [2, 3, 4]))"#),
        "0.6666666667"
    );
}

/// **`jaccard_similarity`** flattens each argument to strings and builds **`HashSet`**s — multiset/binary masks that only differ
/// by digit **multiplicity** collapse to the same set (**BUG-172**).
#[test]
fn jaccard_similarity_binary_masks_collapse_to_unit_bug_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaccard_similarity([1, 0, 1], [0, 1, 1]))"#),
        "1"
    );
}

#[test]
fn angle_between_bearing_degrees_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", angle_between(1, 0, 0, 1))"#),
        "135"
    );
}

#[test]
fn ml_focal_loss_step_gamma_two_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_focal_loss_step(0.9, 1, 2))"#),
        "0.001053605157"
    );
}

#[test]
fn ml_dice_loss_overlap_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_dice_loss_step([1, 0, 1], [1, 1, 0]))"#),
        "0"
    );
}

#[test]
fn iou_two_boxes_overlap_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", iou_2d_axis_aligned([0, 0, 2, 2], [1, 1, 3, 3]))"#),
        "0.1428571429"
    );
}

#[test]
fn geohash_encode_sf_precision_six_cw() {
    assert_eq!(
        eval_string(r#"geohash_encode(37.7749, -122.4194, 6)"#),
        "9q8yyk"
    );
}

#[test]
fn minmax_zscore_norm_lists_cw() {
    assert_eq!(
        eval_string(r#"stringify(minmax_scale([1, 2, 3, 4]))"#),
        "(0, 0.333333333333333, 0.666666666666667, 1)"
    );
    assert_eq!(
        eval_string(r#"stringify(zscore_norm([1, 2, 3, 4]))"#),
        "(-1.34164078649987, -0.447213595499958, 0.447213595499958, 1.34164078649987)"
    );
}

#[test]
fn zscore_scalar_against_population_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", zscore(3, [1, 2, 3, 4]))"#),
        "0.4472135955"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", zscore(10, [1, 2, 3, 4, 100]))"#),
        "-0.3075912095"
    );
}

#[test]
fn kl_js_divergence_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kl_divergence([0.2, 0.8], [0.2, 0.8]))"#),
        "0"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kl_divergence([0.5, 0.5], [0.2, 0.8]))"#),
        "0.2231435513"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", js_divergence([0.1, 0.9], [0.2, 0.8]))"#),
        "0.009966389341"
    );
}

#[test]
fn mutual_information_joint_matrix_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mutual_information([[0.25, 0.25], [0.25, 0.25]]))"#),
        "0"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mutual_information([[0.5, 0], [0, 0.5]]))"#),
        "0.6931471806"
    );
}

#[test]
fn cross_entropy_arr_non_standard_formula_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cross_entropy_arr([0, 1, 0], [0.7, 0.2, 0.1]))"#,),
        "1.609437912"
    );
}

#[test]
fn ml_kl_divergence_loss_identical_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", ml_kl_divergence_loss([0.2, 0.8], [0.2, 0.8]))"#),
        "0"
    );
}

#[test]
fn relu_sigmoid_softplus_gelu_swish_elu_cw() {
    assert_eq!(eval_string(r#"sprintf("%.10g", relu(-2))"#), "0");
    assert_eq!(eval_string(r#"sprintf("%.10g", relu(3))"#), "3");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", leaky_relu(-2, 0.1))"#),
        "-0.2"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", sigmoid(0))"#), "0.5");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", softplus(1))"#),
        "1.313261688"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", gelu(1))"#), "0.9135418956");
    assert_eq!(eval_string(r#"sprintf("%.10g", swish(1))"#), "0.7310585786");
    assert_eq!(eval_string(r#"sprintf("%.10g", elu(-1))"#), "-0.6321205588");
}

#[test]
fn balanced_accuracy_specificity_confusion_counts_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", balanced_accuracy([1, 0, 1, 1], [1, 1, 0, 1]))"#),
        "0.3333333333"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", specificity([1, 0, 1, 1], [1, 1, 0, 1]))"#),
        "0"
    );
    assert_eq!(
        eval_string(r#"stringify(confusion_counts([1, 0, 1, 1], [1, 1, 0, 1]))"#),
        "(2, 1, 0, 1)"
    );
}

#[test]
fn cohen_kappa_brier_score_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cohen_kappa([1, 0, 1, 1], [1, 1, 0, 1]))"#),
        "-0.3333333333"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", brier_score([0.9, 0.1, 0.8], [1, 0, 1]))"#),
        "0.02"
    );
}

#[test]
fn roc_auc_sklearn_style_example_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", roc_auc([0.1, 0.4, 0.35, 0.8], [0, 0, 1, 1]))"#,),
        "0.75"
    );
}

#[test]
fn matrix_multiply_two_by_two_cw() {
    assert_eq!(
        eval_string(r#"stringify(matrix_multiply([[1, 2], [3, 4]], [[0, 1], [1, 0]]))"#),
        "([2, 1], [4, 3])"
    );
}

#[test]
fn gini_impurity_and_gini_coefficient_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gini_impurity([10, 10]))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", gini([1, 2, 3]))"#),
        "0.2222222222"
    );
}

#[test]
fn cartesian_product_pairs_cw() {
    assert_eq!(
        eval_string(r#"stringify(cartesian_product([1, 2], [10, 20]))"#),
        "([1, 10], [1, 20], [2, 10], [2, 20])"
    );
}

#[test]
fn cummax_cummin_and_diff_cw() {
    assert_eq!(
        eval_string(r#"stringify(cummax([3, 1, 4, 2]))"#),
        "(3, 3, 4, 4)"
    );
    assert_eq!(
        eval_string(r#"stringify(cummin([3, 1, 4, 2]))"#),
        "(3, 1, 1, 1)"
    );
    assert_eq!(
        eval_string(r#"stringify(diff([3, 5, 9, 8]))"#),
        "(2, 4, -1)"
    );
}

#[test]
fn one_hot_middle_class_cw() {
    assert_eq!(eval_string(r#"stringify(one_hot(1, 4))"#), "(0, 1, 0, 0)");
}

#[test]
fn robust_scale_outlier_cw() {
    assert_eq!(
        eval_string(r#"stringify(robust_scale([1, 2, 3, 100]))"#),
        "(-0.0204081632653061, -0.0102040816326531, 0, 0.989795918367347)"
    );
}

#[test]
fn zero_sum_minmax_list_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", zero_sum_minmax([1, 2, 3]))"#),
        "1"
    );
}

#[test]
fn nlp_laplace_smoothing_step_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", nlp_laplace_smoothing(1, 2, 3))"#),
        "0.4"
    );
}

#[test]
fn cosine_similarity_orthogonal_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cosine_similarity([1, 0, 1], [0, 1, 0]))"#),
        "0"
    );
}

/// **`mode([…])`** with a single bracket list echoes **`stringify`** as that list; variadic picks the true modal (**BUG-173**).
#[test]
fn mode_variadic_vs_single_arrayref_bug_cw() {
    assert_eq!(eval_string(r#"stringify(mode(1, 2, 2, 3))"#), "2");
    assert_eq!(
        eval_string(r#"stringify(mode([1, 2, 2, 3]))"#),
        "2"
    );
}

#[test]
fn mode_val_arrayref_finds_modal_cw() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mode_val([1, 2, 2, 3]))"#),
        "2"
    );
}
