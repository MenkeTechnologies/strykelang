//! OOP class-declaration pins. Stryke's `class { name: Type ... }` is
//! the modern OOP surface — auto-generated constructor, typed fields,
//! BUILD hook, single-inheritance via `extends`, method dispatch.
//! These pins protect the public-facing shape across parser refactors.

use crate::common::*;

// ── Construction + field access ──────────────────────────────────────

#[test]
fn class_with_typed_fields_constructs_and_reads_back() {
    let code = r#"
        class TestUser {
            name: Str
            age:  Int
        }
        my $u = TestUser(name => "alice", age => 30);
        ($u->name eq "alice" && $u->age == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn class_default_value_used_when_field_omitted() {
    let code = r#"
        class TestRole {
            name: Str
            role: Str = "guest"
        }
        my $u = TestRole(name => "alice");
        $u->role eq "guest" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn class_default_value_overridable_at_construction() {
    let code = r#"
        class TestRole2 {
            name: Str
            role: Str = "guest"
        }
        my $u = TestRole2(name => "alice", role => "admin");
        $u->role eq "admin" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn class_int_field_default_zero() {
    let code = r#"
        class TestCounter {
            n: Int = 0
        }
        my $c = TestCounter();
        $c->n == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Field mutation via setter (`$obj->field(NEW_VAL)`) ───────────────

#[test]
fn class_field_setter_mutates_value() {
    let code = r#"
        class TestBox {
            value: Int = 0
        }
        my $b = TestBox();
        $b->value(42);
        $b->value == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn class_counter_increments_via_method() {
    let code = r#"
        class TestCounter2 {
            n: Int = 0
            fn inc { $self->n($self->n + 1) }
        }
        my $c = TestCounter2();
        $c->inc for 1:5;
        $c->n == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Methods + $self ──────────────────────────────────────────────────

#[test]
fn class_method_can_read_self_fields() {
    let code = r#"
        class TestRect {
            width:  Int
            height: Int
            fn area { $self->width * $self->height }
        }
        my $r = TestRect(width => 4, height => 5);
        $r->area == 20 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn class_method_with_args_in_addition_to_self() {
    let code = r#"
        class TestAdder {
            base: Int = 10
            fn plus($n) { $self->base + $n }
        }
        my $a = TestAdder();
        $a->plus(5) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUILD hook fires at construction time ────────────────────────────

#[test]
fn class_build_hook_runs_on_construction() {
    let code = r#"
        class TestBuilt {
            x: Int
            doubled: Int = 0
            fn BUILD { $self->doubled($self->x * 2) }
        }
        my $b = TestBuilt(x => 7);
        $b->doubled == 14 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn class_build_hook_can_die_to_reject_invalid_args() {
    let code = r#"
        class TestValidated {
            x: Int
            fn BUILD { die "x_negative\n" if $self->x < 0 }
        }
        eval { TestValidated(x => -1) };
        $@ eq "x_negative\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── extends — single inheritance ─────────────────────────────────────

#[test]
fn child_class_inherits_parent_fields() {
    let code = r#"
        class TestAnimal {
            name: Str
        }
        class TestDog extends TestAnimal {
            breed: Str
        }
        my $d = TestDog(name => "rex", breed => "shepherd");
        ($d->name eq "rex" && $d->breed eq "shepherd") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn child_class_inherits_parent_methods() {
    let code = r#"
        class TestParent {
            name: Str
            fn greet { "hello from " . $self->name }
        }
        class TestChild extends TestParent {
            age: Int
        }
        my $c = TestChild(name => "alice", age => 30);
        $c->greet eq "hello from alice" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn child_method_overrides_parent_method() {
    let code = r#"
        class TestBase {
            fn kind { "base" }
        }
        class TestDerived extends TestBase {
            fn kind { "derived" }
        }
        my $d = TestDerived();
        my $b = TestBase();
        ($d->kind eq "derived" && $b->kind eq "base") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-level inheritance ──────────────────────────────────────────

#[test]
fn three_level_inheritance_inherits_through_chain() {
    let code = r#"
        class TestL1 {
            fn level { 1 }
        }
        class TestL2 extends TestL1 { }
        class TestL3 extends TestL2 { }
        my $obj = TestL3();
        $obj->level == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn three_level_method_override_picks_most_derived() {
    let code = r#"
        class TestA { fn kind { "A" } }
        class TestB extends TestA { fn kind { "B" } }
        class TestC extends TestB { fn kind { "C" } }
        my @kinds = (TestA()->kind, TestB()->kind, TestC()->kind);
        join(",", @kinds) eq "A,B,C" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Object identity ──────────────────────────────────────────────────

#[test]
fn two_constructions_have_independent_state() {
    let code = r#"
        class TestBox2 {
            value: Int = 0
        }
        my $a = TestBox2(value => 1);
        my $b = TestBox2(value => 2);
        ($a->value == 1 && $b->value == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn setter_on_one_does_not_affect_other() {
    let code = r#"
        class TestBox3 {
            value: Int = 0
        }
        my $a = TestBox3(value => 1);
        my $b = TestBox3(value => 1);
        $a->value(99);
        ($a->value == 99 && $b->value == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Float field type ─────────────────────────────────────────────────

#[test]
fn class_float_field_holds_decimal() {
    let code = r#"
        class TestPoint {
            x: Float
            y: Float
        }
        my $p = TestPoint(x => 3.5, y => 2.5);
        (abs($p->x - 3.5) < 1e-9 && abs($p->y - 2.5) < 1e-9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Composition: class holding another class ─────────────────────────

#[test]
fn class_can_hold_another_class_instance() {
    let code = r#"
        class TestInner {
            v: Int
        }
        class TestOuter {
            inner: Any
            fn make {
                my $i = TestInner(v => 42);
                $self->inner($i);
            }
        }
        my $o = TestOuter();
        $o->make;
        $o->inner->v == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Method calls in loops / collections ──────────────────────────────

#[test]
fn map_over_class_instances_via_method() {
    let code = r#"
        class TestSq {
            n: Int
            fn squared { $self->n * $self->n }
        }
        my @objs = map { TestSq(n => $_) } (1, 2, 3, 4);
        my @vals = map { $_->squared } @objs;
        join(",", @vals) eq "1,4,9,16" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ref() returns the class name ─────────────────────────────────────

#[test]
fn ref_on_class_instance_returns_class_name() {
    let code = r#"
        class TestRefName {
            x: Int = 0
        }
        my $obj = TestRefName();
        ref($obj) eq "TestRefName" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUILD hook does not run when extending a class without override ─

#[test]
fn child_inherits_parent_build_hook() {
    let code = r#"
        class TestBuiltParent {
            x: Int
            doubled: Int = 0
            fn BUILD { $self->doubled($self->x * 2) }
        }
        class TestBuiltChild extends TestBuiltParent { }
        my $c = TestBuiltChild(x => 7);
        $c->doubled == 14 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
