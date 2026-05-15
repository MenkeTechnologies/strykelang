//! sprintf format-directive pins. Stryke supports the full Perl
//! sprintf surface plus extensions; lock the core directives so a
//! future printf refactor can't silently change formatting that
//! every demo's column alignment depends on.

use crate::common::*;

// ── Integer directives ───────────────────────────────────────────────

#[test]
fn sprintf_decimal_integer() {
    let code = r#"
        sprintf("%d", 42) eq "42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_signed_negative_integer() {
    let code = r#"
        sprintf("%d", -42) eq "-42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_hex_lowercase() {
    let code = r#"
        sprintf("%x", 255) eq "ff" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_hex_uppercase() {
    let code = r#"
        sprintf("%X", 255) eq "FF" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_octal() {
    let code = r#"
        sprintf("%o", 8) eq "10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_binary() {
    let code = r#"
        sprintf("%b", 10) eq "1010" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Width and zero-pad ───────────────────────────────────────────────

#[test]
fn sprintf_zero_padded_width() {
    let code = r#"
        sprintf("%05d", 42) eq "00042" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_right_aligned_width() {
    let code = r#"
        sprintf("%5d", 42) eq "   42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_left_aligned_width() {
    let code = r#"
        sprintf("%-5d", 42) eq "42   " ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_zero_pad_hex_width() {
    let code = r#"
        sprintf("%08x", 255) eq "000000ff" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Float directives ─────────────────────────────────────────────────

#[test]
fn sprintf_float_default_six_digit_precision() {
    let code = r#"
        # Perl default is 6 digits after the decimal.
        sprintf("%f", 3.14) eq "3.140000" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_float_two_decimal_precision() {
    let code = r#"
        sprintf("%.2f", 3.14159) eq "3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_float_width_and_precision() {
    let code = r#"
        sprintf("%8.2f", 3.14159) eq "    3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_float_zero_pad_width_precision() {
    let code = r#"
        sprintf("%08.2f", 3.14) eq "00003.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_scientific_lower() {
    let code = r#"
        sprintf("%e", 1234567.0) eq "1.234567e+06" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_scientific_upper() {
    let code = r#"
        sprintf("%E", 1234567.0) eq "1.234567E+06" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_g_shortest_form() {
    let code = r#"
        # %g picks %e or %f depending on magnitude; for 3.14 it should
        # produce "3.14" (g strips trailing zeros).
        sprintf("%g", 3.14) eq "3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String directive ────────────────────────────────────────────────

#[test]
fn sprintf_string_basic() {
    let code = r#"
        sprintf("%s", "hello") eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_string_right_aligned_width() {
    let code = r#"
        sprintf("%10s", "hi") eq "        hi" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_string_left_aligned_width() {
    let code = r#"
        sprintf("%-10s", "hi") eq "hi        " ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_string_max_length_precision() {
    let code = r#"
        # Precision on %s truncates.
        sprintf("%.3s", "hello") eq "hel" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Character directive ──────────────────────────────────────────────

#[test]
fn sprintf_char_directive() {
    let code = r#"
        sprintf("%c", 65) eq "A" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Percent literal ──────────────────────────────────────────────────

#[test]
fn sprintf_percent_literal() {
    let code = r#"
        sprintf("100%%") eq "100%" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-arg interleaved ────────────────────────────────────────────

#[test]
fn sprintf_multi_arg_interleaved() {
    let code = r#"
        sprintf("name=%s age=%d", "alice", 30) eq "name=alice age=30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_table_row_format() {
    let code = r#"
        sprintf("%-10s %5d %8.2f", "alice", 42, 3.14)
            eq "alice         42     3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Vector format `%vN` ──────────────────────────────────────────────

#[test]
fn sprintf_v_directive_for_version_string() {
    let code = r#"
        # %vd treats string char codepoints as decimal nums joined by ".".
        # "1.4.7" → 1.4.7 (since "1" is char 49 — actually %vd uses codepoints)
        # Easier: use chr(1).chr(4).chr(7) → 1.4.7
        sprintf("%vd", chr(1) . chr(4) . chr(7)) eq "1.4.7" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Negative-width / left-align with leading minus ──────────────────

#[test]
fn sprintf_explicit_sign_plus() {
    let code = r#"
        # %+d forces leading +/- sign.
        sprintf("%+d", 42) eq "+42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sprintf_explicit_sign_plus_negative() {
    let code = r#"
        sprintf("%+d", -42) eq "-42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Large numbers + grouping (Perl doesn't group; just preserve) ─────

#[test]
fn sprintf_large_integer_preserved() {
    let code = r#"
        sprintf("%d", 1234567890) eq "1234567890" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── %s on numeric arg coerces ───────────────────────────────────────

#[test]
fn sprintf_s_on_int_coerces_to_string() {
    let code = r#"
        sprintf("%s", 42) eq "42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Many-arg one-shot used by every table-print idiom ───────────────

#[test]
fn sprintf_seven_arg_complex_row() {
    let code = r#"
        my $s = sprintf("%-8s %5d %5d %8.2f %8.2f  %s  %s",
            "alice", 42, 100, 3.14, 99.999, "tag1", "tag2");
        # Just check key segments are present.
        (index($s, "alice") == 0
            && index($s, "3.14") >= 0
            && index($s, "tag1  tag2") >= 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
