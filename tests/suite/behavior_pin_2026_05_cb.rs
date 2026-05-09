//! Behavior-pinning batch CB (2026-05-09): activation gradients (`sigmoid_grad`, `tanh_grad`, `relu_grad`),
//! nonlinear loss / activations (`squared_hinge`, `softsign`, `prelu`, `threshold_act`), plasma formulae
//! (`plasma_frequency`, `debye_length`, `cyclotron_frequency`, `larmor_radius`), `skew_normal_*`, `cramer_rao_bound`,
//! plus `iota_range` (**BUG-127**): returns `0 .. N−1` and drops extra comma args without warning.

use crate::common::*;

#[test]
fn sigmoid_grad_midpoint_symmetry_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", sigmoid_grad(0))"#),
        "0.250000000000000"
    );
}

#[test]
fn tanh_grad_identity_at_zero_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", tanh_grad(0))"#),
        "1.000000000000000"
    );
}

#[test]
fn relu_grad_step_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", relu_grad(1.5))"#),
        "1.000000000000000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.15f", relu_grad(-2))"#),
        "0.000000000000000"
    );
}

#[test]
fn squared_hinge_inside_margin_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", squared_hinge(1, 0.25))"#),
        "0.562500000000000"
    );
}

#[test]
fn softsign_negative_three_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", softsign(-3.0))"#),
        "-0.750000000000000"
    );
}

#[test]
fn prelu_scales_negative_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", prelu(-4, 0.1))"#),
        "-0.400000000000"
    );
}

#[test]
fn threshold_act_selects_floor_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", threshold_act(0.5, 0.0, -1.0))"#),
        "0.500000000000"
    );
}

#[test]
fn plasma_frequency_electron_scale_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.12e", plasma_frequency(1e18))"#),
        "5.641460230307e+10"
    );
}

#[test]
fn debye_length_room_and_dense_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.12e", debye_length(300, 1e20))"#),
        "1.195270607402e-07"
    );
}

#[test]
fn cyclotron_frequency_unit_magnetic_field_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.12e", cyclotron_frequency(1.0))"#),
        "1.758820011062e+11"
    );
}

#[test]
fn larmor_radius_nonrelativistic_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.15f", larmor_radius(1e6, 1.0))"#),
        "0.000005685630103"
    );
}

#[test]
fn iota_range_zero_until_n_exclusive_cb() {
    assert_eq!(
        eval_string(r#"stringify(iota_range(5))"#),
        "(0, 1, 2, 3, 4)"
    );
}

// BUG-127: extra commas are swallowed — call shape looks like Perl variads but arity is fixed at 1.
#[test]
fn iota_range_trailing_numeric_args_ignored_matches_five_only_cb() {
    assert_eq!(
        eval_string(r#"stringify(iota_range(5, 99, -1))"#),
        "(0, 1, 2, 3, 4)"
    );
}

#[test]
fn skew_normal_pdf_standard_symmetric_peak_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", skew_normal_pdf(0, 0, 1, 0))"#),
        "0.398942280401"
    );
}

#[test]
fn skew_normal_cdf_standard_median_coin_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", skew_normal_cdf(0, 0, 1, 0))"#),
        "0.500000000000"
    );
}

#[test]
fn cramer_rao_bound_two_and_one_point_two_five_cb() {
    assert_eq!(
        eval_string(r#"sprintf("%.12f", cramer_rao_bound(2, 1.25))"#),
        "0.400000000000"
    );
}
