//! Regression pin: `fn <name>(...)` declared anywhere — including
//! `package main` — must NOT trigger the legacy "stryke builtin cannot
//! be redefined" parse error.
//!
//! The new rule: bare callable spellings (the 11k+ names in the global
//! `%all` reflection hash) always dispatch to the global builtin,
//! regardless of package context. A `fn sum {}` declaration anywhere
//! registers under `Pkg::sum`; to invoke it, callers must use the
//! fully-qualified spelling (`Foo::sum(...)`, `main::sum(...)`). There
//! is no shadowing of builtins in stryke.

use crate::common::*;

#[test]
fn fn_named_like_builtin_inside_non_main_package_is_allowed() {
    // `sum` is a stryke builtin; declaring `fn sum` inside `package Stats`
    // registers `Stats::sum` and must not raise a parse error.
    let v = eval_int(
        r#"
            package Stats
            fn sum(@xs) {
                my $total = 0
                $total += $_ for @xs
                $total
            }

            package main
            Stats::sum(10, 20, 30)
        "#,
    );
    assert_eq!(v, 60);
}

#[test]
fn builtin_remains_callable_from_main_when_package_decl_uses_same_name() {
    // Calling bare `sum(...)` from main MUST hit the global builtin; the
    // package-local `Stats::sum` does not pollute the bare namespace.
    let v = eval_int(
        r#"
            package Stats
            fn sum(@xs) { 42 }            # always returns 42

            package main
            sum(1, 2, 3)                  # builtin → 6
        "#,
    );
    assert_eq!(v, 6);
}

#[test]
fn fn_named_like_builtin_in_main_is_rejected() {
    // No-shadow rule WITH a twist: at `package main` scope there is no
    // qualified `main::name(...)` escape hatch the user could use to
    // reach a builtin-shadowing UDF, so the parser rejects the decl.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(r#"fn sum(@xs) { 42 }"#);
    assert_eq!(kind, ErrorKind::Syntax);
}

#[test]
fn package_switching_lets_each_package_define_sum() {
    // Two packages can each define `sum`. Both qualified calls must work
    // and remain isolated from the bare builtin.
    let v = eval_int(
        r#"
            package A
            fn sum(@xs) { 1 }
            package B
            fn sum(@xs) { 2 }
            package main
            A::sum() + B::sum() * 10 + sum(7, 8, 9) * 100
        "#,
    );
    // A::sum() = 1, B::sum() = 2, bare sum(7,8,9) = 24 (builtin)
    assert_eq!(v, 1 + 20 + 2400);
}

#[test]
fn package_main_explicit_restores_shadow_check() {
    // Switching back to `package main` mid-file re-enables the rule:
    // bare `fn sum` after the second `package main` is rejected.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(
        r#"
            package Foo
            fn helper { 1 }
            package main
            fn sum(@xs) { 0 }
        "#,
    );
    assert_eq!(kind, ErrorKind::Syntax);
}

#[test]
fn reserved_syntactic_keyword_still_rejected_as_fn_name() {
    // Parsing keywords (`if`, `while`, …) as function names breaks the
    // grammar — that rejection must remain regardless of package.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(
        r#"
            fn if(@xs) { 0 }
        "#,
    );
    assert_eq!(kind, ErrorKind::Syntax);
}

#[test]
fn bare_set_inside_non_main_package_routes_to_builtin() {
    // `set` is in the 11k callable-spelling table; inside `package Tags`
    // a bare `set(@xs)` call must dispatch to the global builtin, not
    // attempt to invoke `Tags::set`.
    let v = eval_int(
        r#"
            package Tags
            fn count_distinct(@xs) { scalar(set(@xs)) }
            package main
            Tags::count_distinct("a", "b", "b", "c", "a")
        "#,
    );
    assert_eq!(v, 3);
}
