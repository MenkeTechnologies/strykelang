//! Operator-precedence pins. Common pitfalls: `**` is right-assoc and
//! high precedence, ternary is right-assoc and low, `=` is even lower,
//! `||` < `&&` < bit-ops < comparison < arithmetic, `//` defined-or
//! lives next to `||`.

use crate::common::*;

// ── Arithmetic precedence ──────────────────────────────────────────

#[test]
fn multiplication_before_addition() {
    let code = r#"
        2 + 3 * 4
    "#;
    assert_eq!(eval_int(code), 14);
}

#[test]
fn parentheses_override_precedence() {
    let code = r#"
        (2 + 3) * 4
    "#;
    assert_eq!(eval_int(code), 20);
}

#[test]
fn division_before_subtraction() {
    let code = r#"
        20 - 10 / 2
    "#;
    assert_eq!(eval_int(code), 15);
}

#[test]
fn modulo_same_precedence_as_multiplication() {
    let code = r#"
        10 + 7 % 3
    "#;
    assert_eq!(eval_int(code), 11);
}

#[test]
fn exponent_higher_than_multiplication() {
    let code = r#"
        2 * 3 ** 2
    "#;
    assert_eq!(eval_int(code), 18);
}

#[test]
fn exponent_right_associative() {
    let code = r#"
        # 2 ** 3 ** 2 = 2 ** (3 ** 2) = 2 ** 9 = 512.
        2 ** 3 ** 2
    "#;
    assert_eq!(eval_int(code), 512);
}

// ── Unary minus ────────────────────────────────────────────────────

#[test]
fn unary_minus_binds_tight() {
    let code = r#"
        -3 + 5
    "#;
    assert_eq!(eval_int(code), 2);
}

#[test]
fn unary_minus_on_parenthesized_expr() {
    let code = r#"
        -(3 + 5)
    "#;
    assert_eq!(eval_int(code), -8);
}

// ── Comparison < arithmetic ────────────────────────────────────────

#[test]
fn comparison_returns_truthy_after_arithmetic() {
    let code = r#"
        (10 + 5 > 12) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chained_comparisons_left_associative() {
    let code = r#"
        # Perl: 1 < 2 < 3 evaluates as (1 < 2) < 3 → true < 3 → 1 < 3 → true.
        (1 < 2 < 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Logical operators ─────────────────────────────────────────────

#[test]
fn and_higher_than_or() {
    let code = r#"
        # 1 || 0 && 0 parses as 1 || (0 && 0) = 1 || 0 = 1.
        (1 || 0 && 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn and_short_circuit_returns_first_falsy() {
    let code = r#"
        # 0 && die → 0 (no die).
        my $r = 0 && die "should not fire\n";
        $r == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn or_short_circuit_returns_first_truthy() {
    let code = r#"
        my $r = 5 || die "should not fire\n";
        $r == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Defined-or `//` ────────────────────────────────────────────────

#[test]
fn defined_or_returns_lhs_if_defined() {
    let code = r#"
        my $a = 0;
        my $r = $a // 99;
        $r == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_returns_rhs_if_lhs_undef() {
    let code = r#"
        my $a;
        my $r = $a // 99;
        $r == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_differs_from_or_for_zero() {
    let code = r#"
        my $a = 0;
        my $or  = $a || 99;
        my $dor = $a // 99;
        # `||` returns 99 (0 falsy); `//` returns 0 (0 defined).
        ($or == 99 && $dor == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Ternary ────────────────────────────────────────────────────────

#[test]
fn ternary_right_associative() {
    let code = r#"
        # 1 ? 2 : 3 ? 4 : 5 = 1 ? 2 : (3 ? 4 : 5) = 2.
        1 ? 2 : 3 ? 4 : 5
    "#;
    assert_eq!(eval_int(code), 2);
}

#[test]
fn nested_ternary_picks_correct_branch() {
    let code = r#"
        my $x = 50;
        my $r = $x < 25 ? "low"
              : $x < 75 ? "mid"
              :           "high";
        $r eq "mid" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Assignment ─────────────────────────────────────────────────────

#[test]
fn chained_assignment_right_associative() {
    let code = r#"
        my $a; my $b; my $c;
        $a = $b = $c = 42;
        ($a == 42 && $b == 42 && $c == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn assignment_returns_assigned_value() {
    let code = r#"
        my $x;
        my $r = ($x = 10);
        ($x == 10 && $r == 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Augmented-assignment operators ─────────────────────────────────

#[test]
fn plus_eq_in_place() {
    let code = r#"
        my $x = 10;
        $x += 5;
        $x == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn star_eq_in_place() {
    let code = r#"
        my $x = 4;
        $x *= 3;
        $x == 12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_eq_appends_string() {
    let code = r#"
        my $s = "hello";
        $s .= " world";
        $s eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Bitwise ────────────────────────────────────────────────────────

#[test]
fn bitwise_and_lower_than_arithmetic() {
    let code = r#"
        # 0xF0 & 0xFF + 1 → 0xF0 & 0x100 = 0. (in Perl + binds tighter than &)
        0xF0 & 0xFF + 1
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn bit_or_works() {
    let code = r#"
        (0xF0 | 0x0F) == 0xFF ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bit_shift_left() {
    let code = r#"
        (1 << 8) == 256 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Range vs comma ────────────────────────────────────────────────

#[test]
fn range_inside_list_with_other_items() {
    let code = r#"
        my @r = (0, 1:3, 99);
        len(@r) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── eq vs == precedence ──────────────────────────────────────────

#[test]
fn eq_lower_than_concat() {
    let code = r#"
        # "a" . "b" eq "ab" parses as ("a" . "b") eq "ab" = true.
        ("a" . "b" eq "ab") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Unary not ─────────────────────────────────────────────────────

#[test]
fn not_returns_truthy_zero_one() {
    let code = r#"
        my $r = !1;
        my $s = !0;
        ($r == 0 && $s) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String repeat ─────────────────────────────────────────────────

#[test]
fn string_repeat_higher_than_concat() {
    let code = r#"
        # "ab" x 2 . "c" parses as ("ab" x 2) . "c" = "abab" . "c".
        ("ab" x 2 . "c") eq "ababc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Auto-increment ─────────────────────────────────────────────────

#[test]
fn post_increment_returns_old_value() {
    let code = r#"
        my $x = 5;
        my $r = $x++;
        ($r == 5 && $x == 6) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pre_increment_returns_new_value() {
    let code = r#"
        my $x = 5;
        my $r = ++$x;
        ($r == 6 && $x == 6) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
