//! Behavior-pinning batch BK (2026-05-08): Color operations and Matrix manipulation.

use crate::common::*;

// ── Color Operations ────────────────────────────────────────────────────────

#[test]
fn color_ops_bk() {
    // color_invert(r, g, b)
    let inv = eval(r#"color_invert(255, 0, 0)"#).as_array_vec().unwrap();
    assert_eq!(inv[0].to_int(), 0);
    assert_eq!(inv[1].to_int(), 255);
    assert_eq!(inv[2].to_int(), 255);

    // color_grayscale(r, g, b)
    // 0.2126 * 255 + 0 + 0 = 54.213 -> 54
    let gray = eval(r#"color_grayscale(255, 0, 0)"#)
        .as_array_vec()
        .unwrap();
    assert_eq!(gray[0].to_int(), 54);
    assert_eq!(gray[1].to_int(), 54);
    assert_eq!(gray[2].to_int(), 54);

    // color_blend(r1, g1, b1, r2, g2, b2, t)
    let blend = eval(r#"color_blend(255, 0, 0, 0, 0, 255, 0.5)"#)
        .as_array_vec()
        .unwrap();
    assert_eq!(blend[0].to_int(), 128);
    assert_eq!(blend[1].to_int(), 0);
    assert_eq!(blend[2].to_int(), 128);

    // color_complement(r, g, b)
    // Red (0, 1, 0.5) in HSL -> complement is (180, 1, 0.5) -> Cyan (0, 255, 255)
    let comp = eval(r#"color_complement(255, 0, 0)"#)
        .as_array_vec()
        .unwrap();
    assert_eq!(comp[0].to_int(), 0);
    assert_eq!(comp[1].to_int(), 255);
    assert_eq!(comp[2].to_int(), 255);
}

// ── Matrix Manipulation ─────────────────────────────────────────────────────

#[test]
fn matrix_ops_bk() {
    // matrix_hadamard(A, B) -> element-wise product
    let code_had = r#"
        my $m = matrix_hadamard([[1, 2]], [[10, 20]]);
        $m->[0]->[1]
    "#;
    assert_eq!(eval_int(code_had), 40);

    // matrix_flatten(M) -> flat list
    let code_flat = r#"
        my @f = matrix_flatten([[1, 2], [3, 4]]);
        join(",", @f)
    "#;
    assert_eq!(eval_string(code_flat), "1,2,3,4");

    // matrix_sum(M) -> scalar sum
    assert_eq!(eval_int("matrix_sum([[1, 2], [3, 4]])"), 10);

    // matrix_map(sub, M)
    let code_map = r#"
        my $m = matrix_map({ _ * 10 }, [[1, 2]]);
        $m->[0]->[1]
    "#;
    assert_eq!(eval_int(code_map), 20);

    // matrix_from_rows(rows, cols, ...flat)
    let code_from = r#"
        my $m = matrix_from_rows(2, 2, 1, 2, 3, 4);
        $m->[1]->[0]
    "#;
    assert_eq!(eval_int(code_from), 3);
}

// ── Misc Uncategorized ───────────────────────────────────────────────────────

#[test]
fn misc_uncategorized_bk() {
    // byte_size of numbers (converted to string)
    assert_eq!(eval_int("byte_size(12345)"), 5);

    // to_string_val on undef
    assert_eq!(eval_string("to_string_val(undef)"), "");
}
