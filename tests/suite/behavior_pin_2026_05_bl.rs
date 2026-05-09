//! Behavior-pinning batch BL (2026-05-08): Color space (LAB, temperature),
//! color adjust/distance, and linear-algebra builtins not covered in prior batches.

use crate::common::*;

// ── Color: temperature & LAB ────────────────────────────────────────────────

#[test]
fn color_kelvin_and_lab_bl() {
    assert_eq!(
        eval_string(
            r#"my @k = kelvin_to_rgb(6500);
            join(",", @k)"#
        ),
        "255,254,250"
    );

    assert_eq!(
        eval_string(
            r#"my @l = rgb_to_lab(255, 0, 0);
            sprintf("%.2f,%.2f,%.2f", @l)"#
        ),
        "53.24,80.09,67.20"
    );

    assert_eq!(
        eval_string(
            r#"my @l = rgb_to_lab(10, 20, 30);
            my @b = lab_to_rgb(@l);
            sprintf("%.0f,%.0f,%.0f", @b)"#
        ),
        "10,20,30"
    );
}

// ── Color: lighten / darken / distance ────────────────────────────────────

#[test]
fn color_adjust_and_distance_bl() {
    assert_eq!(
        eval_string(
            r#"my @c = color_lighten(100, 100, 100, 0.2);
            join(",", @c)"#
        ),
        "151,151,151"
    );
    assert_eq!(
        eval_string(
            r#"my @c = color_darken(200, 200, 200, 0.25);
            join(",", @c)"#
        ),
        "136,136,136"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.6f", color_distance(0, 0, 0, 255, 255, 255))"#),
        "764.833966"
    );
}

// ── Linear algebra: solve, trace, norms, multiply ───────────────────────────

#[test]
fn matrix_solve_trace_norm_mul_bl() {
    assert_eq!(
        eval_string(r#"my @x = solve([[2, 1], [-1, 1]], [5, 2]); join(",", @x)"#),
        "1,3"
    );
    assert_eq!(eval_int("matrix_trace([[1, 2], [3, 4]])"), 5);
    assert_eq!(eval_int(r#"mnorm([[3, 4]])"#), 5);
    assert_eq!(
        eval_int(
            r#"my $m = mat_mul([[1, 2], [3, 4]], [[10, 20], [30, 40]]);
            $m->[1]->[1]"#
        ),
        220
    );
}

#[test]
fn matrix_rank_det_eig_bl() {
    assert_eq!(eval_int("mrank([[1, 2], [2, 4]])"), 1);
    assert_eq!(eval_int(r#"det([[1, 2], [3, 4]])"#), -2);
    assert_eq!(
        eval_string(
            r#"my @e = eig([[2, 1], [1, 2]]);
            join(",", @e)"#
        ),
        "3,1"
    );
}

#[test]
fn matrix_cond_cholesky_bl() {
    assert_eq!(eval_int("matrix_cond([[1, 0], [0, 1]])"), 1);
    assert_eq!(
        eval_string(r#"to_string_val(matrix_cond([[1, 2], [2, 4]]))"#),
        "Inf"
    );
    assert_eq!(
        eval_string(
            r#"my @c = matrix_cholesky([[4, 2], [2, 3]]);
            sprintf("%.1f %.1f %.1f %.3f", $c[0]->[0], $c[0]->[1], $c[1]->[0], $c[1]->[1])"#
        ),
        "2.0 0.0 1.0 1.414"
    );
}

// ── Vector ops ──────────────────────────────────────────────────────────────

#[test]
fn vector_dot_cross_bl() {
    assert_eq!(eval_int("dot_product([1, 2, 3], [4, 5, 6])"), 32);
    assert_eq!(
        eval_string(r#"my @c = cross_product([1, 0, 0], [0, 1, 0]); join(",", @c)"#),
        "0,0,1"
    );
}
