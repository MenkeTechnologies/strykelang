//! Regression pin: `our @arr` and `our %h` declared inside `package Foo`
//! must be visible from any other package as `@Foo::arr` / `%Foo::h`,
//! and bare reads inside `Foo` must address the same storage. Before the
//! fix, only `our $scalar` was exported across packages — arrays / hashes
//! stayed package-local, so `@Config::servers` from `main` returned an
//! empty list while `@servers` inside `package Config` saw the data.

use crate::common::*;

#[test]
fn our_array_in_package_visible_from_main() {
    let v = eval_int(
        r#"
            package Config
            our @servers = ("a", "b", "c")
            package main
            scalar @Config::servers
        "#,
    );
    assert_eq!(v, 3);
}

#[test]
fn our_hash_in_package_visible_from_main() {
    let v = eval_string(
        r#"
            package Config
            our %env = (region => "us-west-2", stage => "prod")
            package main
            $Config::env{region}
        "#,
    );
    assert_eq!(v, "us-west-2");
}

#[test]
fn bare_array_inside_package_aliases_qualified_form() {
    // Inside `package Config`, `@servers` must read the same storage
    // as `@Config::servers`. A push through either name is visible
    // through the other.
    let v = eval_int(
        r#"
            package Config
            our @servers = ("a", "b")
            push @servers, "c"
            scalar @Config::servers
        "#,
    );
    assert_eq!(v, 3);
}

#[test]
fn main_can_mutate_package_array() {
    // Write to `@Config::servers` from main, read bare inside Config.
    let v = eval_string(
        r#"
            package Config
            our @servers = ("a")
            package main
            push @Config::servers, "z"
            package Config
            join(",", @servers)
        "#,
    );
    assert_eq!(v, "a,z");
}

#[test]
fn main_can_mutate_package_hash_element() {
    let v = eval_string(
        r#"
            package Config
            our %env = (stage => "dev")
            package main
            $Config::env{stage} = "prod"
            package Config
            $env{stage}
        "#,
    );
    assert_eq!(v, "prod");
}

#[test]
fn our_array_does_not_leak_globally_when_in_non_main_package() {
    // Bare `@servers` in `main` must NOT see `Config`'s array; the only
    // route from outside is the qualified `@Config::servers` form.
    let v = eval_int(
        r#"
            package Config
            our @servers = ("a", "b", "c")
            package main
            scalar @servers
        "#,
    );
    assert_eq!(v, 0);
}

#[test]
fn my_array_inside_package_stays_lexical() {
    // `my @x` must still be lexical even inside a non-main package —
    // not exported to `@Config::x`.
    let v = eval_int(
        r#"
            package Config
            my @x = (1, 2, 3, 4)
            scalar @Config::x + scalar @x * 10
        "#,
    );
    // @Config::x == 0 (my is lexical), @x == 4
    assert_eq!(v, 40);
}

#[test]
fn our_array_in_main_works_via_main_prefix() {
    // `our @x` in main and `@main::x` must alias.
    let v = eval_int(
        r#"
            our @nums = (10, 20, 30)
            scalar @main::nums
        "#,
    );
    assert_eq!(v, 3);
}
