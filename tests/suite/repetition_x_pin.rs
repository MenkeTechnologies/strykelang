//! `x` (repetition) operator pins. Strings and list contexts.

use crate::common::*;

// ── string repetition ────────────────────────────────────────────

#[test]
fn string_x_basic() {
    let code = r#"("ab" x 4) eq "abababab" ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_x_one_is_identity() {
    let code = r#"("hello" x 1) eq "hello" ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_x_zero_is_empty() {
    let code = r#"("xyz" x 0) eq "" ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_x_negative_treated_as_zero() {
    // Perl coerces negative counts to 0.
    let code = r#"("xyz" x -5) eq "" ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_x_on_empty_string_stays_empty() {
    let code = r#"("" x 100) eq "" ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_x_single_char_builds_padding() {
    let code = r#"
        my $s = " " x 8;
        length($s) == 8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_x_with_multi_char_unit() {
    let code = r#"("abc" x 3) eq "abcabcabc" ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_x_large_count() {
    let code = r#"
        my $s = "x" x 10000;
        length($s) == 10000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_x_fractional_count_truncated() {
    // Perl: count is int-truncated.
    let code = r#"("ab" x 3.7) eq "ababab" ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_x_concat_with_other_strings() {
    let code = r#"
        my $line = "+" . ("-" x 5) . "+";
        $line eq "+-----+" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── list repetition ──────────────────────────────────────────────

#[test]
fn list_x_basic() {
    let code = r#"
        my @r = (1, 2, 3) x 3;
        join(",", @r) eq "1,2,3,1,2,3,1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn list_x_one_is_identity() {
    let code = r#"
        my @r = (1, 2, 3) x 1;
        join(",", @r) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn list_x_zero_yields_empty() {
    let code = r#"
        my @r = (1, 2, 3) x 0;
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn list_x_negative_yields_empty() {
    let code = r#"
        my @r = (1, 2, 3) x -3;
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn list_x_single_element_list() {
    let code = r#"
        my @r = (42) x 5;
        join(",", @r) eq "42,42,42,42,42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn list_x_mixed_types() {
    let code = r#"
        my @r = ("a", 1, "b") x 2;
        join("|", @r) eq "a|1|b|a|1|b" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn list_x_used_for_init() {
    // Common idiom: pre-initialise array of zeros.
    let code = r#"
        my @zeros = (0) x 10;
        (len(@zeros) == 10 && sum(@zeros) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn list_x_used_for_initial_ones() {
    let code = r#"
        my @ones = (1) x 5;
        sum(@ones) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── derived patterns ─────────────────────────────────────────────

#[test]
fn padding_with_x_for_table_alignment() {
    let code = r#"
        my $name = "alice";
        my $pad  = " " x (10 - length($name));
        ($name . $pad) eq "alice     " ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn horizontal_rule_via_x() {
    let code = r#"
        my $hr = "-" x 20;
        length($hr) == 20 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bordered_table_via_x_repetition() {
    let code = r#"
        my $border = "+" . ("-" x 5) . "+" . ("-" x 5) . "+";
        $border eq "+-----+-----+" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn box_drawing_via_unicode_x() {
    let code = r#"
        my $line = "\x{2500}" x 5;   # ─
        # 5 chars × 3 bytes UTF-8 = 15 bytes.
        length($line) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grid initialization ─────────────────────────────────────────

#[test]
fn grid_via_nested_x_repeat() {
    let code = r#"
        # 3x3 grid of zeros via list-of-arrayref pattern.
        my @grid;
        for my $i (1:3) {
            push @grid, [(0) x 3];
        }
        # All entries 0; sum across all rows = 0.
        my $total = 0;
        for my $row (@grid) {
            $total += sum(@$row);
        }
        $total == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── lvalue interaction ──────────────────────────────────────────

#[test]
fn list_x_in_array_assignment_flattens() {
    let code = r#"
        my @a = (("a") x 3, ("b") x 2);
        # a a a b b
        join(",", @a) eq "a,a,a,b,b" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── arithmetic with count ───────────────────────────────────────

#[test]
fn string_x_count_from_var() {
    let code = r#"
        my $n = 7;
        my $s = "*" x $n;
        length($s) == 7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn list_x_count_from_expression() {
    let code = r#"
        my @r = ("a") x (3 + 2);
        len(@r) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── inside print/join ─────────────────────────────────────────────

#[test]
fn x_inside_join_args() {
    let code = r#"
        my $s = join(",", ("x") x 3);
        $s eq "x,x,x" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_inside_concat_chain() {
    let code = r#"
        my $s = "[" . ("-" x 4) . "]";
        $s eq "[----]" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── edge: count fractional ──────────────────────────────────────

#[test]
fn list_x_fractional_truncates() {
    let code = r#"
        my @r = (1, 2) x 2.9;
        # 2.9 -> 2; result is (1, 2, 1, 2).
        join(",", @r) eq "1,2,1,2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
