//! Tests for the forge native `class` OOP system.

use crate::common::*;
use forge::error::ErrorKind;

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

// ── Abstract classes ─────────────────────────────────────────────────

#[test]
fn abstract_class_cannot_instantiate() {
    assert_eq!(
        eval_err_kind(r#"abstract class Shape { name: Str }; Shape(name => "x")"#),
        ErrorKind::Runtime,
    );
}

#[test]
fn abstract_class_subclass_can_instantiate() {
    assert_eq!(
        eval_string(
            r#"abstract class Shape { name: Str }
            class Circle extends Shape { radius: Int }
            my $c = Circle(name => "c", radius => 5);
            $c->name . ":" . $c->radius"#
        ),
        "c:5"
    );
}

#[test]
fn abstract_class_with_methods() {
    assert_eq!(
        eval_string(
            r#"abstract class Shape {
                name: Str
                fn describe { "Shape: " . $self->name }
            }
            class Circle extends Shape { }
            my $c = Circle(name => "ring");
            $c->describe()"#
        ),
        "Shape: ring"
    );
}

// ── Protected visibility ─────────────────────────────────────────────

#[test]
fn protected_field_accessible_from_own_method() {
    assert_eq!(
        eval_int(
            r#"class Secret {
                prot hidden: Int = 42
                fn reveal { $self->hidden }
            }
            my $s = Secret();
            $s->reveal()"#
        ),
        42
    );
}

#[test]
fn protected_field_accessible_from_subclass_method() {
    assert_eq!(
        eval_int(
            r#"class Base {
                prot secret: Int = 99
            }
            class Child extends Base {
                fn get_secret { $self->secret }
            }
            my $c = Child();
            $c->get_secret()"#
        ),
        99
    );
}

#[test]
fn protected_field_blocked_from_outside() {
    assert_eq!(
        eval_err_kind(
            r#"class Guarded { prot value: Int = 10 }
            my $g = Guarded();
            $g->value"#
        ),
        ErrorKind::Runtime,
    );
}

#[test]
fn protected_method_accessible_from_subclass() {
    assert_eq!(
        eval_string(
            r#"class Base {
                prot fn helper { "helped" }
            }
            class Child extends Base {
                fn do_work { $self->helper() }
            }
            my $c = Child();
            $c->do_work()"#
        ),
        "helped"
    );
}

#[test]
fn protected_method_blocked_from_outside() {
    assert_eq!(
        eval_err_kind(
            r#"class Guarded {
                prot fn internal { "secret" }
            }
            my $g = Guarded();
            $g->internal()"#
        ),
        ErrorKind::Runtime,
    );
}

#[test]
fn private_field_blocked_from_outside() {
    assert_eq!(
        eval_err_kind(
            r#"class Vault { priv code: Int = 1234 }
            my $v = Vault();
            $v->code"#
        ),
        ErrorKind::Runtime,
    );
}

#[test]
fn private_field_accessible_from_own_method() {
    assert_eq!(
        eval_int(
            r#"class Vault {
                priv code: Int = 1234
                fn get_code { $self->code }
            }
            my $v = Vault();
            $v->get_code()"#
        ),
        1234
    );
}

// ── Static fields ────────────────────────────────────────────────────

#[test]
fn static_field_default_value() {
    assert_eq!(
        eval_int(
            r#"class Counter {
                static count: Int = 0
            }
            Counter::count()"#
        ),
        0
    );
}

#[test]
fn static_field_setter() {
    assert_eq!(
        eval_int(
            r#"class Counter {
                static count: Int = 0
            }
            Counter::count(5);
            Counter::count()"#
        ),
        5
    );
}

#[test]
fn static_field_shared_across_instances() {
    assert_eq!(
        eval_int(
            r#"class Tracker {
                static total: Int = 0
                name: Str
                fn BUILD { Tracker::total(Tracker::total() + 1) }
            }
            my $a = Tracker(name => "a");
            my $b = Tracker(name => "b");
            Tracker::total()"#
        ),
        2
    );
}

// ── BUILD constructor hook ───────────────────────────────────────────

#[test]
fn build_hook_runs_on_construction() {
    assert_eq!(
        eval_string(
            r#"class Greeter {
                name: Str
                greeting: Str = ""
                fn BUILD { $self->greeting("Hello, " . $self->name) }
            }
            my $g = Greeter(name => "World");
            $g->greeting"#
        ),
        "Hello, World"
    );
}

#[test]
fn build_hook_parent_runs_first() {
    assert_eq!(
        eval_string(
            r#"class Base {
                log: Str = ""
                fn BUILD { $self->log("base") }
            }
            class Child extends Base {
                fn BUILD { $self->log($self->log . "+child") }
            }
            my $c = Child();
            $c->log"#
        ),
        "base+child"
    );
}

// ── DESTROY destructor ──────────────────────────────────────────────

#[test]
fn destroy_runs_child_first() {
    assert_eq!(
        eval_string(
            r#"class Base {
                static log: Str = ""
                fn DESTROY { Base::log(Base::log() . "base,") }
            }
            class Child extends Base {
                fn DESTROY { Base::log(Base::log() . "child,") }
            }
            my $c = Child();
            $c->destroy();
            Base::log()"#
        ),
        "child,base,"
    );
}

// ── Trait contract enforcement ───────────────────────────────────────

#[test]
fn trait_missing_required_method_error() {
    assert_eq!(
        eval_err_kind(
            r#"trait Drawable { fn draw }
            class Box impl Drawable {
                name: Str
            }"#
        ),
        ErrorKind::Runtime,
    );
}

#[test]
fn trait_all_required_methods_satisfied() {
    assert_eq!(
        eval_string(
            r#"trait Drawable { fn draw }
            class Box impl Drawable {
                name: Str
                fn draw { "drawing " . $self->name }
            }
            my $b = Box(name => "square");
            $b->draw()"#
        ),
        "drawing square"
    );
}

#[test]
fn trait_default_method_not_required() {
    assert_eq!(
        eval_string(
            r#"trait Loggable {
                fn log_prefix { "LOG" }
                fn log_msg
            }
            class Event impl Loggable {
                msg: Str
                fn log_msg { $self->msg }
            }
            my $e = Event(msg => "hello");
            $e->log_msg()"#
        ),
        "hello"
    );
}

#[test]
fn class_does_trait_check() {
    assert_eq!(
        eval_int(
            r#"trait Printable { fn to_str }
            class Item impl Printable {
                name: Str
                fn to_str { $self->name }
            }
            my $i = Item(name => "x");
            $i->does("Printable")"#
        ),
        1
    );
}

#[test]
fn class_does_unrelated_trait_false() {
    assert_eq!(
        eval_string(
            r#"trait Printable { fn to_str }
            class Item { name: Str }
            my $i = Item(name => "x");
            $i->does("Printable")"#
        ),
        ""
    );
}
