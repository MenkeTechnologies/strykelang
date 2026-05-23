//! Pin regex modifier flags `/i` (case-insensitive), `/m` (multiline
//! anchors), `/s` (dotall), `/x` (extended whitespace/comments).
//! Probed against the running interpreter on 2026-05-23.

use crate::common::*;

// ── /i — case-insensitive ─────────────────────────────────────────

#[test]
fn i_modifier_matches_upper_with_lower_pattern() {
    let code = r#"
        "HELLO" =~ /hello/i ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn i_modifier_no_match_without_flag() {
    let code = r#"
        "HELLO" =~ /hello/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn i_modifier_applies_to_dot_too() {
    let code = r#"
        "ABC" =~ /a.c/i ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn i_modifier_inline_via_flag_group() {
    // (?i)pat — inline case-insensitive switch.
    let code = r#"
        "HelloWorld" =~ /^(?i)hello/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── /m — multiline anchors ────────────────────────────────────────

#[test]
fn m_modifier_caret_matches_after_internal_newline() {
    let code = r#"
        "abc\ndef" =~ /^def/m ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn m_modifier_required_for_caret_after_newline() {
    // Without /m, ^ only matches start-of-string.
    let code = r#"
        "abc\ndef" =~ /^def/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn m_modifier_dollar_matches_before_internal_newline() {
    let code = r#"
        "abc\ndef" =~ /abc$/m ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── /s — dotall ───────────────────────────────────────────────────

#[test]
fn s_modifier_dot_matches_newline() {
    // c.d should match c\nd with /s; the . spans the newline.
    let code = r#"
        "abc\ndef" =~ /c.d/s ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn s_modifier_required_for_dot_to_span_newline() {
    let code = r#"
        "abc\ndef" =~ /c.d/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn s_modifier_does_not_change_anchor_behavior() {
    // /s changes . only; ^ still matches start-of-string only.
    let code = r#"
        "abc\ndef" =~ /^def/s ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

// ── /x — extended (free whitespace + comments) ────────────────────

#[test]
fn x_modifier_ignores_whitespace_in_pattern() {
    let code = r#"
        "abc123" =~ / a b c \d+ /x ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_modifier_required_to_ignore_whitespace() {
    // Without /x, the pattern literally needs space chars.
    let code = r#"
        "abc123" =~ / a b c \d+ / ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 0);
}

// ── combined modifiers ────────────────────────────────────────────

#[test]
fn im_combined_case_insensitive_multiline() {
    let code = r#"
        "first\nABC" =~ /^abc/im ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_combined_case_insensitive_dotall() {
    let code = r#"
        "ABC\nDEF" =~ /c.d/is ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ms_combined_multiline_and_dotall_independent() {
    // /m governs ^/$, /s governs `.` — they don't conflict.
    let code = r#"
        my $hits = 0;
        $hits++ if "x\nY" =~ /^Y/m;
        $hits++ if "abc\ndef" =~ /c.d/s;
        $hits == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
