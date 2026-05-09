//! Behavior-pinning batch BM (2026-05-08): Matrix LU/QR/pinv, matrix exp/log/sqrt/sin/cos,
//! and quaternion helpers (no overlap with `behavior_pin_2026_05_bl`).

use crate::common::*;

// ── LU / QR / pseudoinverse ──────────────────────────────────────────────────

#[test]
fn matrix_decompositions_bm() {
    let code_lu = r#"
        my @lu = mlu([[4, 3], [6, 3]]);
        sprintf(
            "%.3f:%.1f,%.1f",
            $lu[0]->[1]->[0],
            $lu[1]->[0]->[0],
            $lu[1]->[1]->[1]
        )
    "#;
    assert_eq!(eval_string(code_lu), "0.667:6.0,1.0");

    let code_qr = r#"
        my @qr = mqr([[1, 1], [1, -1]]);
        sprintf(
            "%.3f:%.3f,%s",
            $qr[0]->[0]->[0],
            $qr[1]->[0]->[0],
            stringify($qr[1]->[1]->[1])
        )
    "#;
    assert_eq!(eval_string(code_qr), "0.707:1.414,1.4142135623731");

    assert_eq!(
        eval_string(
            r#"my @p = pinv([[1, 2], [3, 4], [5, 6]]);
            sprintf("%.2f,%.2f,%s", $p[0]->[0], $p[0]->[2], stringify($p[1]->[2]))"#
        ),
        "-1.33,0.67,-0.416666666666666"
    );
}

// ── Matrix exp / log / sqrt / sin / cos ───────────────────────────────────────

#[test]
fn matrix_functions_bm() {
    assert_eq!(
        eval_string(r#"stringify(expm([[1, 0], [0, 0]]))"#),
        "((2.71828182845905, 0), (0, 1))"
    );
    assert_eq!(
        eval_string(r#"stringify(logm([[1.0, 0], [0, 1]]))"#),
        "((0, 0), (0, 0))"
    );
    assert_eq!(
        eval_string(r#"stringify(sqrtm([[9, 0], [0, 16]]))"#),
        "((3, 0), (0, 4))"
    );
    assert_eq!(
        eval_string(r#"stringify(sinm([[0, 0], [0, 0]]))"#),
        "((0, 0), (0, 0))"
    );
    assert_eq!(
        eval_string(r#"stringify(cosm([[0, 0], [0, 0]]))"#),
        "((1, 0), (0, 1))"
    );
}

// ── Quaternions & rotation matrices ────────────────────────────────────────────

#[test]
fn quaternions_and_euler_bm() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", quat_norm([1, 2, 3, 4]))"#),
        "5.477226"
    );
    assert_eq!(
        eval_string(r#"my @c = quat_conj([1, 2, 3, 4]); join(",", @c)"#),
        "1,-2,-3,-4"
    );
    assert_eq!(
        eval_string(
            r#"my @r = quat_mul([1, 0, 0, 0], [0, 1, 0, 0]);
            join(",", map { int($_) } @r)"#
        ),
        "0,1,0,0"
    );

    assert_eq!(
        eval_string(
            r#"my @q = quat_from_axis_angle([0, 1, 0], 1.57079632679);
            join(",", map { sprintf("%.6f", $_) } @q)"#
        ),
        "0.707107,0.000000,0.707107,0.000000"
    );

    let code_m = r#"
        my $m = quat_to_matrix([0.707106781186547, 0, 0.707106781186547, 0]);
        sprintf("%.0f:%.3f", $m->[0]->[0], $m->[0]->[2])
    "#;
    assert_eq!(eval_string(code_m), "0:1.000");

    assert_eq!(
        eval_string(
            r#"my @e = matrix_to_euler_zyx([[0, 0, 1], [0, 1, 0], [-1, 0, 0]]);
            join(",", map { sprintf("%.5f", $_) } @e)"#
        ),
        "-0.00000,1.57080,0.00000"
    );
}
