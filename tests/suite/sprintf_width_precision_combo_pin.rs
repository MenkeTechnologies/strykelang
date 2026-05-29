//! sprintf width + precision + flag combinations.
//!
//! Pins behavior at the *intersection* of three printf format dimensions:
//! flag (`-`, `+`, `0`, ` `), width, and precision. Each cell is verified
//! against system `perl` output. Augments `sprintf_format_pin.rs` and
//! `sprintf_edge_pin.rs` which cover dimensions one at a time.

use crate::common::*;

// ── Sign flag + width + precision ─────────────────────────────────

/// `%+8.2f` — explicit-sign positive, width 8, precision 2.
#[test]
fn plus_flag_width_precision_positive_float() {
    let code = r#"
        sprintf("%+8.2f", 3.14) eq "   +3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%+8.2f` with negative — `-` consumes the sign slot.
#[test]
fn plus_flag_width_precision_negative_float() {
    let code = r#"
        sprintf("%+8.2f", -3.14) eq "   -3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `% 8.2f` — space flag reserves a leading space for positive.
#[test]
fn space_flag_width_precision_positive() {
    let code = r#"
        sprintf("% 8.2f", 3.14) eq "    3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `% 8.2f` negative — `-` overrides the space.
#[test]
fn space_flag_width_precision_negative() {
    let code = r#"
        sprintf("% 8.2f", -3.14) eq "   -3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Zero pad + width + precision ─────────────────────────────────

/// `%07.2f` — zero-pad to width 7 with 2 decimal places.
#[test]
fn zero_pad_width_precision_float() {
    let code = r#"
        sprintf("%07.2f", 3.14) eq "0003.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%010.3f` with negative — sign in slot 0, zeros after.
#[test]
fn zero_pad_width_precision_negative_float() {
    let code = r#"
        sprintf("%010.3f", -1.5) eq "-00001.500" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%+010.3f` with negative — sign overrides `+`, zeros fill rest.
#[test]
fn plus_zero_pad_width_precision_negative() {
    let code = r#"
        sprintf("%+010.3f", -1.5) eq "-00001.500" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%+010.3f` positive — `+` consumes the sign slot, zeros after.
#[test]
fn plus_zero_pad_width_precision_positive() {
    let code = r#"
        sprintf("%+010.3f", 1.5) eq "+00001.500" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Left-align flag + width + precision ───────────────────────────

/// `%-+10.3f` — left-align with explicit sign.
#[test]
fn left_align_plus_width_precision_positive() {
    let code = r#"
        sprintf("%-+10.3f|", 1.5) eq "+1.500    |" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%- 10.3f` — left-align with space flag.
#[test]
fn left_align_space_width_precision_positive() {
    let code = r#"
        sprintf("%- 10.3f|", 1.5) eq " 1.500    |" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Width + precision on strings ──────────────────────────────────

/// `%10.6s` — width 10, max 6 chars (precision truncates), right-aligned.
#[test]
fn width_precision_string_truncates_and_pads() {
    let code = r#"
        sprintf("%10.6s", "helloworld") eq "    hellow" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%-10.6s` — same but left-aligned.
#[test]
fn left_align_width_precision_string() {
    let code = r#"
        sprintf("%-10.6s|", "helloworld") eq "hellow    |" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%-.3s` — precision-only truncation with `-` (no-op flag for strings).
#[test]
fn left_align_precision_only_string_truncates() {
    let code = r#"
        sprintf("%-.3s|", "hello") eq "hel|" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%5.10s` — width less than precision; precision truncates if input
/// longer, otherwise input fits and width applies.
#[test]
fn width_lt_precision_string_short_input() {
    let code = r#"
        sprintf("%5.10s", "ab") eq "   ab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%-5.10s` — left-aligned variant of above.
#[test]
fn left_align_width_lt_precision_short() {
    let code = r#"
        sprintf("%-5.10s|", "ab") eq "ab   |" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Width + precision on integer ──────────────────────────────────

/// `%3.0d` — width 3, precision 0 (still emits non-zero value).
#[test]
fn width_precision_zero_nonzero_int() {
    let code = r#"
        sprintf("%3.0d", 5) eq "  5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%.0d` of non-zero — precision 0 doesn't suppress non-zero digit.
#[test]
fn precision_zero_nonzero_int() {
    let code = r#"
        sprintf("%.0d", 5) eq "5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Rounding-mode pins (banker's rounding) ────────────────────────

/// `%6.0f` of 3.5 — banker's rounding to nearest even: 4.
#[test]
fn width_precision_zero_float_bankers_round_up() {
    let code = r#"
        sprintf("%6.0f", 3.5) eq "     4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%.0f` of 2.5 — banker's rounding: 2 (even).
#[test]
fn precision_zero_float_bankers_round_down_to_even() {
    let code = r#"
        sprintf("%.0f", 2.5) eq "2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── e-format with width + precision ───────────────────────────────

/// `%.0e` of 1234.5 — scientific with 0 precision.
#[test]
fn precision_zero_scientific() {
    let code = r#"
        sprintf("%.0e", 1234.5) eq "1e+03" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Dynamic width via `*` ──────────────────────────────────────────

/// `%*d` — dynamic width from a separate argument.
#[test]
fn dynamic_width_star_int() {
    let code = r#"
        sprintf("%*d", 5, 42) eq "   42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `%-*s` — dynamic width with left-align flag.
#[test]
fn dynamic_width_star_left_align_string() {
    let code = r#"
        sprintf("%-*s", 6, "hi") eq "hi    " ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
