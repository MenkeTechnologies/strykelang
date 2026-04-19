//! Tests for the perlrs native `class` OOP system.

use crate::common::*;

#[test]
fn class_basic_instantiation() {
    assert_eq!(
        eval_string(r#"class Dog { name: Str }; my $d = Dog(name => "Rex"); $d->name"#),
        "Rex"
    );
}

#[test]
fn class_field_default_value() {
    assert_eq!(
        eval_int(r#"class Counter { value: Int = 0 }; my $c = Counter(); $c->value"#),
        0
    );
}

#[test]
fn class_positional_construction() {
    assert_eq!(
        eval_string(r#"class Point { x: Int; y: Int }; my $p = Point(3, 4); $p->x . "," . $p->y"#),
        "3,4"
    );
}

#[test]
fn class_field_setter() {
    assert_eq!(
        eval_int(r#"class Box { value: Int = 0 }; my $b = Box(); $b->value(42); $b->value"#),
        42
    );
}

#[test]
fn class_instance_method() {
    assert_eq!(
        eval_string(
            r#"class Greeter {
                name: Str
                fn greet { "Hello, " . $self->name }
            }
            my $g = Greeter(name => "World");
            $g->greet()"#
        ),
        "Hello, World"
    );
}

#[test]
fn class_method_with_params() {
    assert_eq!(
        eval_int(
            r#"class Calculator {
                value: Int = 0
                fn add($n) { $self->value + $n }
            }
            my $c = Calculator(value => 10);
            $c->add(5)"#
        ),
        15
    );
}

#[test]
fn class_static_method() {
    assert_eq!(
        eval_int(
            r#"class Math {
                fn Self.add($a, $b) { $a + $b }
            }
            Math::add(3, 4)"#
        ),
        7
    );
}

#[test]
fn class_inheritance_fields() {
    assert_eq!(
        eval_string(
            r#"class Animal { name: Str }
            class Dog extends Animal { breed: Str = "Mixed" }
            my $d = Dog(name => "Rex", breed => "Lab");
            $d->name . ":" . $d->breed"#
        ),
        "Rex:Lab"
    );
}

#[test]
fn class_inheritance_method() {
    assert_eq!(
        eval_string(
            r#"class Animal {
                name: Str
                fn speak { "Animal: " . $self->name }
            }
            class Dog extends Animal { }
            my $d = Dog(name => "Rex");
            $d->speak()"#
        ),
        "Animal: Rex"
    );
}

#[test]
fn class_method_override() {
    assert_eq!(
        eval_string(
            r#"class Animal {
                name: Str
                fn speak { "Animal" }
            }
            class Dog extends Animal {
                fn speak { "Woof from " . $self->name }
            }
            my $d = Dog(name => "Rex");
            $d->speak()"#
        ),
        "Woof from Rex"
    );
}

#[test]
fn class_isa_self() {
    assert_eq!(
        eval_int(
            r#"class Dog { name: Str }
            my $d = Dog(name => "Rex");
            $d->isa("Dog")"#
        ),
        1
    );
}

#[test]
fn class_isa_parent() {
    assert_eq!(
        eval_int(
            r#"class Animal { }
            class Dog extends Animal { }
            my $d = Dog();
            $d->isa("Animal")"#
        ),
        1
    );
}

#[test]
fn class_isa_unrelated_false() {
    assert_eq!(
        eval_string(
            r#"class Dog { }
            class Cat { }
            my $d = Dog();
            $d->isa("Cat")"#
        ),
        ""
    );
}

#[test]
fn class_fields_method() {
    assert_eq!(
        eval_string(
            r#"class Point { x: Int; y: Int }
            my $p = Point(1, 2);
            join(",", $p->fields())"#
        ),
        "x,y"
    );
}

#[test]
fn class_fields_includes_inherited() {
    assert_eq!(
        eval_string(
            r#"class Animal { name: Str }
            class Dog extends Animal { breed: Str }
            my $d = Dog(name => "Rex", breed => "Lab");
            join(",", $d->fields())"#
        ),
        "name,breed"
    );
}

#[test]
fn class_with_functional_update() {
    assert_eq!(
        eval_string(
            r#"class Point { x: Int; y: Int }
            my $p = Point(1, 2);
            my $q = $p->with(x => 10);
            $q->x . "," . $q->y"#
        ),
        "10,2"
    );
}

#[test]
fn class_with_inherited_fields() {
    assert_eq!(
        eval_string(
            r#"class Animal { name: Str }
            class Dog extends Animal { breed: Str }
            my $d = Dog(name => "Rex", breed => "Lab");
            my $e = $d->with(name => "Max");
            $e->name . ":" . $e->breed"#
        ),
        "Max:Lab"
    );
}

#[test]
fn class_clone() {
    assert_eq!(
        eval_string(
            r#"class Point { x: Int; y: Int }
            my $p = Point(1, 2);
            my $q = $p->clone();
            $p->x(10);
            $q->x . "," . $p->x"#
        ),
        "1,10"
    );
}

#[test]
fn class_to_hash() {
    assert_eq!(
        eval_string(
            r#"class Point { x: Int; y: Int }
            my $p = Point(3, 4);
            my $h = $p->to_hash();
            $h->{x} . "," . $h->{y}"#
        ),
        "3,4"
    );
}

#[test]
fn class_stringify() {
    let s = eval_string(r#"class Point { x: Int; y: Int }; my $p = Point(3, 4); "$p""#);
    assert!(s.contains("Point"));
    assert!(s.contains("x =>"));
    assert!(s.contains("y =>"));
}

#[test]
fn trait_basic_definition() {
    assert_eq!(
        eval_string(
            r#"trait Printable { fn to_str }
            class Item impl Printable {
                name: Str
                fn to_str { $self->name }
            }
            my $i = Item(name => "test");
            $i->to_str()"#
        ),
        "test"
    );
}

#[test]
fn class_multiple_inheritance() {
    assert_eq!(
        eval_string(
            r#"class A { a: Int = 1 }
            class B { b: Int = 2 }
            class C extends A, B { c: Int = 3 }
            my $obj = C();
            $obj->a . "," . $obj->b . "," . $obj->c"#
        ),
        "1,2,3"
    );
}
