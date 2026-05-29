//! Pin: indexed array-element assignment must leave the VM stack
//! balanced when executed as a non-tail expression statement inside
//! a sub body.
//!
//! Before the fix, `compile_assign` for `ExprKind::ArrayElement`
//! always emitted `Op::SetArrayElem` (which consumes both index and
//! value), regardless of the `keep` flag passed by the generic
//! `ExprKind::Assign` path. The wrapping expression-statement
//! compiler then emitted an `Op::Pop`, expecting one residual value
//! from the assignment; with nothing left to pop, `Op::Pop` ate the
//! caller's stack slot below the sub's `stack_base`.
//!
//! Symptom: `Pkg::sink(Pkg::leak(), Pkg::leak())` — where each
//! `Pkg::leak()` body did `my @res = (0,0); $res[0] = 1; "OUT"` —
//! made the sink see `($u = "", $v = "OUT")` instead of two `"OUT"`s.
//! Each nested call ate one outer stack slot, dropping the prior
//! call's pushed result and shifting `UNDEF` into the first arg.
//!
//! The fix: when `compile_assign` is called with `keep = true` on an
//! `ArrayElement` target, emit `Op::SetArrayElemKeep` instead, so the
//! value remains on the stack for the wrapping statement's `Op::Pop`.

use crate::common::*;

#[test]
fn nested_call_with_local_array_elem_assign_returns_correct_value() {
    let code = r#"
        fn Pkg::leak() {
            my @res = (0, 0);
            $res[0] = 1;
            "OUT"
        }
        fn Pkg::sink($u, $v) {
            "$u|$v"
        }
        Pkg::sink(Pkg::leak(), Pkg::leak())
    "#;
    assert_eq!(eval_string(code), "OUT|OUT");
}

#[test]
fn nested_call_three_args_no_arg_drop() {
    let code = r#"
        fn Pkg::leak() {
            my @res = (0, 0, 0);
            $res[1] = 7;
            "X"
        }
        fn Pkg::join3($a1, $a2, $a3) {
            "$a1|$a2|$a3"
        }
        Pkg::join3(Pkg::leak(), Pkg::leak(), Pkg::leak())
    "#;
    assert_eq!(eval_string(code), "X|X|X");
}

#[test]
fn nested_call_four_args_no_arg_drop() {
    let code = r#"
        fn Pkg::leak() {
            my @res = (0, 0, 0);
            $res[2] = 9;
            "Y"
        }
        fn Pkg::join4($a, $b, $c, $d) {
            "$a|$b|$c|$d"
        }
        Pkg::join4(Pkg::leak(), Pkg::leak(), Pkg::leak(), Pkg::leak())
    "#;
    assert_eq!(eval_string(code), "Y|Y|Y|Y");
}

#[test]
fn multi_elem_assigns_in_body_then_tail() {
    // Multiple element assigns in the body must each preserve stack balance.
    let code = r#"
        fn Pkg::work() {
            my @res = (0) x 5;
            $res[0] = 10;
            $res[1] = 20;
            $res[2] = 30;
            $res[3] = 40;
            $res[4] = 50;
            "DONE"
        }
        fn Pkg::pair($a, $b) {
            "$a/$b"
        }
        Pkg::pair(Pkg::work(), Pkg::work())
    "#;
    assert_eq!(eval_string(code), "DONE/DONE");
}

#[test]
fn array_elem_assign_returns_assigned_value_when_used_as_expr() {
    // `my $r = ($arr[0] = 7)` must yield 7; the assignment expression
    // itself returns the assigned value when its result is consumed.
    let code = r#"
        my @a = (0, 0);
        my $r = ($a[0] = 7);
        $r
    "#;
    assert_eq!(eval_int(code), 7);
}

#[test]
fn karatsuba_grade_school_natural_form() {
    // Regression-pin the exact pattern that drove the karatsuba demo's
    // `grade_school` workaround: a function that builds a `my @res`
    // partial-product accumulator via indexed assigns, then returns a
    // digit string built from the accumulator, called nested in another
    // function's arg list.
    let code = r#"
        fn Bn::gs($x, $y) {
            my $nx = len($x);
            my $ny = len($y);
            my @res = (0) x ($nx + $ny);
            for my $i (0:$nx - 1) {
                my $a = int(substr($x, $nx - 1 - $i, 1));
                for my $j (0:$ny - 1) {
                    my $b = int(substr($y, $ny - 1 - $j, 1));
                    $res[$i + $j] += $a * $b;
                }
            }
            my $carry = 0;
            for my $k (0:len(@res) - 1) {
                my $v = $res[$k] + $carry;
                $res[$k] = $v % 10;
                $carry = int($v / 10);
            }
            my $out = "";
            for my $k (0:len(@res) - 1) {
                $out = $res[$k] . $out;
            }
            # Strip leading zeros (but keep at least one digit).
            my $i = 0;
            while ($i < len($out) - 1 && substr($out, $i, 1) eq "0") { $i++ }
            substr($out, $i)
        }
        fn Bn::concat($a, $b) {
            "$a+$b"
        }
        # 7 * 11 = 77, 7 * 13 = 91 — the pattern that used to break.
        Bn::concat(Bn::gs("7", "11"), Bn::gs("7", "13"))
    "#;
    assert_eq!(eval_string(code), "77+91");
}
