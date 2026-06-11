//! Pin the `??` / `??=` aliases for `//` / `//=` (C#/Swift
//! null-coalescing spelling) and the `null` alias for `undef`
//! (JS/SQL spelling), added 2026-06-11. Both are stryke extensions:
//! `--compat` leaves `??` as two ternary tokens (syntax error) and
//! `null` as an ordinary sub name.

use crate::common::*;

// ── ?? : defined-or alias ─────────────────────────────────────────────

#[test]
fn double_question_returns_rhs_when_lhs_undef() {
    assert_eq!(eval_string(r#"my $x; $x ?? "fallback""#), "fallback");
}

#[test]
fn double_question_keeps_defined_falsy_lhs() {
    // Defined-or semantics, not `||`: 0 and "" are defined, so they win.
    assert_eq!(eval_int(r#"my $x = 0; $x ?? 9"#), 0);
    assert_eq!(eval_string(r#"my $s = ""; $s ?? "dflt""#), "");
}

#[test]
fn double_question_assign_only_when_undef() {
    assert_eq!(eval_int(r#"my $x; $x ??= 5; $x"#), 5);
    assert_eq!(eval_int(r#"my $x = 3; $x ??= 5; $x"#), 3);
}

#[test]
fn double_question_on_missing_hash_key() {
    assert_eq!(
        eval_string(r#"my $h = {}; $h->{missing} ?? "dflt""#),
        "dflt"
    );
}

#[test]
fn double_question_does_not_break_nested_ternary() {
    // Adjacent `?`s never occur in valid ternaries; spaced ones still work.
    assert_eq!(eval_string(r#"my $c = 1; 1 ? $c ? "a" : "b" : "c""#), "a");
}

#[test]
fn double_question_is_rejected_in_compat_mode() {
    use stryke::error::ErrorKind;
    // `parse_err_kind` takes the read lock — parse directly under the
    // write lock held by `with_global_flags`.
    let kind = with_global_flags(|| {
        stryke::set_compat_mode(true);
        let k = stryke::parse(r#"my $x; my $y = $x ?? "f";"#)
            .unwrap_err()
            .kind;
        stryke::set_compat_mode(false);
        k
    });
    assert!(matches!(kind, ErrorKind::Syntax), "got {:?}", kind);
}

// ── null : undef alias ────────────────────────────────────────────────

#[test]
fn null_value_is_undef() {
    assert_eq!(eval_int(r#"defined(null) ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#"my $x = null; defined($x) ? 1 : 0"#), 0);
}

#[test]
fn null_as_list_assignment_sink() {
    assert_eq!(
        eval_string(r#"my ($a, null, $c) = (1, 2, 3); "$a $c""#),
        "1 3"
    );
}

#[test]
fn null_function_form_undefines_a_variable() {
    assert_eq!(eval_int(r#"my $y = 7; null $y; defined($y) ? 1 : 0"#), 0);
}

#[test]
fn null_coalesces_through_double_question() {
    assert_eq!(eval_string(r#"null ?? "coalesced""#), "coalesced");
}
