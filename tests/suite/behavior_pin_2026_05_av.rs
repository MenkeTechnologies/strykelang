//! Behavior-pinning batch AV (2026-05-08): Sweeping unpinned behaviors.
//!
//! Each test pins the *current* observed output; comments call out the
//! Perl-compat or expected behavior so a future fix flips the assertion to
//! the right value rather than deleting the test.

use crate::common::*;

// ── EXPECT-FEATURE: Magic string decrement ───────────────────────────────────

#[test]
fn magic_string_decrement_range_works() {
    // `\"z\":\"a\":-1` is an unpinned feature from docs/expect-feature-idea.md.
    // It's a strykelang extension since Perl only has magic string increment.
    let out = eval_string(r#"join(",", "z":"x":-1)"#);
    assert_eq!(out, "z,y,x");
}

// ── NAMESPACE-FUNCTIONS: Reserved words as function names ────────────────────

#[test]
fn reserved_word_y_as_function_name_errors() {
    // `fn y { }` should produce an error that `y` is a reserved word.
    let err = parse_err_kind("fn y { }");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("`y` is a reserved word") || msg.contains("Syntax") || msg.contains("Expected"),
        "expected error about reserved word, got {:?}",
        err
    );
}

#[test]
fn reserved_word_cmp_as_function_name_errors() {
    // `fn cmp { }` should produce a syntax error since `cmp` is an operator.
    let err = parse_err_kind("fn cmp { }");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Expected sub name") || msg.contains("Syntax") || msg.contains("StrCmp"),
        "expected error about parsing function name, got {:?}",
        err
    );
}

#[test]
fn single_quote_q_as_function_name_errors() {
    // `fn q { }` should produce a syntax error since `q` is a quote operator.
    let err = parse_err_kind("fn q { }");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Expected sub name") || msg.contains("Syntax") || msg.contains("SingleString"),
        "expected error about parsing function name, got {:?}",
        err
    );
}
