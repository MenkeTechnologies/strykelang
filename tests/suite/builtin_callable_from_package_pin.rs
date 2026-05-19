//! Regression pin: stryke's 11k+ global builtins are callable bare from
//! every package — including from inside a user-defined sub that lives
//! in a non-main package. There is no builtin shadowing: even when a
//! `fn name {}` user sub of the same name is registered (anywhere),
//! the bare-name call site routes to the global builtin.
//!
//! Reaching the user's same-named sub requires the fully-qualified
//! `Pkg::name(...)` spelling.

use crate::common::*;

#[test]
fn set_builtin_callable_from_non_main_package() {
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

#[test]
fn sum_builtin_callable_inside_package_fn() {
    let v = eval_int(
        r#"
            package Stats
            fn total(@xs) { sum(@xs) }
            package main
            Stats::total(1, 2, 3, 4, 5)
        "#,
    );
    assert_eq!(v, 15);
}

#[test]
fn bare_call_routes_to_builtin_even_when_user_sub_of_same_name_exists() {
    // `fn sum {}` declared in `package Custom` registers as `Custom::sum`
    // (not bare `sum`). A bare `sum(...)` call from inside the same
    // package — or any other context — always hits the builtin.
    let v = eval_int(
        r#"
            package Custom
            fn sum(@xs) { 99 }
            fn run { sum(1, 2, 3) }
            package main
            Custom::run()
        "#,
    );
    assert_eq!(v, 6);
}

#[test]
fn qualified_call_routes_to_user_sub_only() {
    // `Custom::sum(...)` reaches the user-defined sub; the builtin would
    // have returned 6 for these arguments.
    let v = eval_int(
        r#"
            package Custom
            fn sum(@xs) { 99 }
            package main
            Custom::sum(1, 2, 3)
        "#,
    );
    assert_eq!(v, 99);
}

#[test]
fn class_static_field_call_still_takes_precedence_over_builtin() {
    // `Counter::count()` qualified call resolves the static field
    // before any builtin fallback; the bare `count()` builtin (which
    // returns 1 with no args) must not steal this dispatch.
    let v = eval_int(
        r#"
            class Counter { static count: Int = 0 }
            Counter::count()
        "#,
    );
    assert_eq!(v, 0);
}

#[test]
fn enum_variant_constructor_still_takes_precedence_over_builtin() {
    let v = eval_string(
        r#"
            enum Color { Red, Green, Blue }
            "" . Color::Red
        "#,
    );
    assert_eq!(v, "Color::Red");
}
