//! Behavior-pinning batch AP (2026-05-06): word-op precedence + interpolation.
//!
//! Pins fixes from the bug-hunt:
//!   - `EXPR or $err = $@` parses as `EXPR or ($err = $@)`, not the broken
//!     `(EXPR or $err) = $@` that produced "Assign to complex lvalue".
//!     Fix landed by hoisting `parse_or_word`/`parse_and_word`/`parse_not_word`
//!     above `parse_assign_expr` in the precedence chain.
//!   - `"$Foo::x"` interpolates the package-qualified scalar (BUG-107 fix
//!     also pinned in `behavior_pin_2026_05_q.rs`; this is the secondary
//!     coverage at the AP layer).

use crate::common::*;

#[test]
fn or_word_lower_than_assignment() {
    // The Perl idiom: `eval { ... } or $err = $@`.
    // Perl precedence: `or` is the LOWEST-binding operator, lower than `=`,
    // so the assignment is the RHS of `or`. Stryke used to parse this as
    // `(eval { ... } or $err) = $@`, surfacing as "Assign to complex lvalue".
    let code = r#"
        my $err;
        eval { die "boom" } or $err = $@;
        # `boom` may include trailing context; just check it's non-empty.
        defined($err) && length($err) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn or_word_with_short_circuit_assignment_simple() {
    // Even simpler: `0 or $x = "fallback"` — `or` should yield the assignment
    // result (the fallback) when LHS is false.
    let code = r#"
        my $x;
        0 or $x = "fallback";
        $x
    "#;
    assert_eq!(eval_string(code), "fallback");
}

#[test]
fn and_word_lower_than_assignment() {
    // Companion: `and` also sits below `=`.
    // `1 and $x = "set"` should assign "set".
    let code = r#"
        my $x;
        1 and $x = "set";
        $x
    "#;
    assert_eq!(eval_string(code), "set");
}

#[test]
fn not_word_lower_than_assignment() {
    // `not` at LOWEST tier of word ops sits above `=` precedence (binds
    // tighter than `or`/`and`). `not 0 or $x = "y"` should still parse the
    // assignment as the `or`-RHS.
    let code = r#"
        my $x;
        not 0 or $x = "y";
        $x
    "#;
    // `not 0` is true; `or` short-circuits — assignment should NOT run.
    assert_eq!(eval_string(code), "");
}

#[test]
fn package_qualified_scalar_interpolates_in_double_quoted() {
    // `parse_interpolated_string` now greedy-matches `::` continuations
    // (parser.rs after the bare-name read). Without this, `"$Foo::x"`
    // captured only `Foo` and left `::x` as literal.
    let code = r#"
        package Foo;
        our $bar = "hello";
        package main;
        "[$Foo::bar]"
    "#;
    assert_eq!(eval_string(code), "[hello]");
}

// NOTE: a `package A::B::C` declaration currently fails to parse with
// "Expected package name, got DoubleString" — separate bug in the package-
// statement parser, not in string interpolation. Multi-segment `::` chains
// inside string interpolation work in principle (the loop in
// `parse_interpolated_string` greedy-matches `::word` repeatedly), but we
// can't write a positive test for it until the package-decl bug is fixed.

#[test]
fn interpolation_single_colon_does_not_capture_namespace() {
    // The continuation only fires on `::`, not a single `:`. So `"$x:end"`
    // must stop the variable at `$x` and emit `:end` as literal text.
    let code = r#"
        my $x = "A";
        ">$x:end<"
    "#;
    assert_eq!(eval_string(code), ">A:end<");
}
