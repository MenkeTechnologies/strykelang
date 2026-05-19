//! Regression pin: a `class` / `struct` / `enum` / `trait` declared
//! inside `package Foo` registers under `Foo::Name`, so calls like
//! `Foo::Name->new(...)` resolve the type definition and method
//! dispatch on the returned instance works. Before the fix, the type
//! was stored under the bare name, so `Foo::Name->new(...)` instantiated
//! a generic blessed hashref and `$obj->field` failed with
//! "Can't locate method".

use crate::common::*;

#[test]
fn class_in_package_field_accessor_dispatches() {
    let v = eval_int(
        r#"
            package Geo
            class Point { x: Int; y: Int }
            package main
            my $p = Geo::Point->new(x => 7, y => 9)
            $p->x + $p->y * 10
        "#,
    );
    assert_eq!(v, 97);
}

#[test]
fn class_in_package_method_dispatches() {
    let v = eval_string(
        r#"
            package Geo
            class Point {
                x: Int
                y: Int
                fn show { sprintf("(%d,%d)", $self->x, $self->y) }
            }
            package main
            my $p = Geo::Point->new(x => 3, y => 4)
            $p->show
        "#,
    );
    assert_eq!(v, "(3,4)");
}

#[test]
fn bare_class_in_main_still_resolves_by_unqualified_name() {
    // No `package` switch — `Point->new(...)` continues to work.
    let v = eval_int(
        r#"
            class Point { x: Int; y: Int }
            my $p = Point->new(x => 5, y => 6)
            $p->x + $p->y
        "#,
    );
    assert_eq!(v, 11);
}

#[test]
fn enum_in_package_resolves_qualified_variant() {
    let v = eval_string(
        r#"
            package Geo
            enum Compass { N, E, S, W }
            package main
            "" . Geo::Compass::N
        "#,
    );
    assert_eq!(v, "Geo::Compass::N");
}

#[test]
fn class_extends_qualified_parent_from_other_package() {
    let v = eval_int(
        r#"
            package Geo
            class Shape {
                kind: Str
                fn label { "shape:" . $self->kind }
            }
            package Geo
            class Circle extends Geo::Shape {
                radius: Int
            }
            package main
            my $c = Geo::Circle->new(kind => "round", radius => 3)
            $c->radius
        "#,
    );
    assert_eq!(v, 3);
}

#[test]
fn package_main_class_uses_bare_name_for_compat() {
    // Explicitly setting `package main` keeps bare-name class registration,
    // so `Point->new(...)` (the common case) is unchanged.
    let v = eval_int(
        r#"
            package main
            class Point { x: Int }
            my $p = Point->new(x => 42)
            $p->x
        "#,
    );
    assert_eq!(v, 42);
}
