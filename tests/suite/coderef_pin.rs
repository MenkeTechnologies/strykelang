//! Coderef pins. sub{}, \&fn, $cb->() invocation, coderef-as-value
//! storage and dispatch.

use crate::common::*;

// ── Basic anonymous sub ─────────────────────────────────────────────

#[test]
fn anonymous_sub_returns_coderef() {
    let code = r#"
        my $f = sub { 42 };
        ref($f) =~ /CODE/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn anonymous_sub_invoked_via_arrow_call() {
    let code = r#"
        my $f = sub { 42 };
        $f->() == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn anonymous_sub_with_args() {
    let code = r#"
        my $f = sub { $_[0] + $_[1] };
        $f->(10, 20) == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── \&fn (taking ref to named fn) ───────────────────────────────────

#[test]
fn ampersand_ref_to_named_fn() {
    let code = r#"
        fn Demo::Cb::work($x) { $x * 2 }
        my $ref = \&Demo::Cb::work;
        ref($ref) =~ /CODE/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ampersand_ref_invocation() {
    let code = r#"
        fn Demo::Cb::triple($x) { $x * 3 }
        my $ref = \&Demo::Cb::triple;
        $ref->(7) == 21 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Coderefs as values ─────────────────────────────────────────────

#[test]
fn coderef_in_array() {
    let code = r#"
        my @fns = (
            sub { $_[0] + 1 },
            sub { $_[0] * 2 },
            sub { $_[0] ** 2 },
        );
        my $r = $fns[1]->(5);
        $r == 10 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coderef_in_hash() {
    let code = r#"
        my %dispatch = (
            inc => sub { $_[0] + 1 },
            dec => sub { $_[0] - 1 },
        );
        ($dispatch{inc}->(10) == 11 && $dispatch{dec}->(10) == 9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Coderef as callback ─────────────────────────────────────────────

#[test]
fn coderef_passed_to_user_fn() {
    let code = r#"
        fn Demo::Cb::apply_twice($f, $x) {
            $f->($f->($x))
        }
        my $inc = sub { $_[0] + 1 };
        Demo::Cb::apply_twice($inc, 5) == 7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Coderef return type from user fn ───────────────────────────────

#[test]
fn user_fn_returns_coderef() {
    let code = r#"
        fn Demo::Cb::make_adder($n) {
            sub { $_[0] + $n }
        }
        my $add5 = Demo::Cb::make_adder(5);
        $add5->(10) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Recursive coderef via mysync ────────────────────────────────────

#[test]
fn recursive_coderef_via_mysync() {
    let code = r#"
        mysync $fact;
        $fact = sub {
            my $n = $_[0];
            $n <= 1 ? 1 : $n * $fact->($n - 1)
        };
        $fact->(5) == 120 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Currying via nested closures ───────────────────────────────────

#[test]
fn curried_addition_via_nested_closures() {
    let code = r#"
        fn Demo::Cb::curry_add() {
            sub {
                my $a = $_[0];
                sub { $a + $_[0] }
            }
        }
        my $c = Demo::Cb::curry_add();
        my $add10 = $c->(10);
        $add10->(7) == 17 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Pipeline: each stage is a coderef ──────────────────────────────

#[test]
fn pipeline_of_coderefs() {
    let code = r#"
        my @stages = (
            sub { $_[0] + 1 },
            sub { $_[0] * 2 },
            sub { $_[0] - 3 },
        );
        my $v = 5;
        for my $stage (@stages) {
            $v = $stage->($v);
        }
        # 5 → 6 → 12 → 9.
        $v == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Coderef map / sort / grep callbacks ────────────────────────────

#[test]
fn map_with_explicit_coderef_callback() {
    let code = r#"
        my $sq = sub { $_[0] * $_[0] };
        my @r = map { $sq->($_) } (1, 2, 3, 4);
        join(",", @r) eq "1,4,9,16" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Coderef stored in OOP class instance ───────────────────────────

#[test]
fn coderef_stored_in_class_field() {
    let code = r#"
        class CodeBox {
            handler: Any
            fn invoke($x) { $self->handler->($x) }
        }
        my $box = CodeBox(handler => sub { $_[0] + 100 });
        $box->invoke(42) == 142 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Coderefs are first-class: can be returned, passed, stored ──────

#[test]
fn coderef_returned_passed_stored_chain() {
    let code = r#"
        fn Demo::Cb::pick($which) {
            return sub { $_[0] + 1 } if $which eq "inc";
            return sub { $_[0] - 1 } if $which eq "dec";
            return sub { $_[0] };
        }
        my @ops = (
            Demo::Cb::pick("inc"),
            Demo::Cb::pick("dec"),
            Demo::Cb::pick("noop"),
        );
        ($ops[0]->(10) == 11
            && $ops[1]->(10) == 9
            && $ops[2]->(10) == 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multiple arg shapes ────────────────────────────────────────────

#[test]
fn variadic_coderef_via_at_underscore() {
    let code = r#"
        my $sum = sub {
            my $s = 0;
            $s += $_ for @_;
            $s
        };
        $sum->(1, 2, 3, 4, 5) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coderef_with_zero_args() {
    let code = r#"
        my $f = sub { 42 };
        $f->() == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Coderef equality / ref-identity ────────────────────────────────

#[test]
fn coderef_numeric_equality_quirk_both_numify_to_zero() {
    // Two distinct coderefs stringify to "CODE(__ANON__)" and numify
    // to 0, so `==` comparison says they're equal even though they
    // are different references. Pin the observed surface.
    // (PARITY-041 already documents the ref-as-number-zero issue.)
    let code = r#"
        my $f1 = sub { 1 };
        my $f2 = sub { 1 };
        ($f1 == $f2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coderef_aliased_compares_equal() {
    let code = r#"
        my $f1 = sub { 1 };
        my $f2 = $f1;   # same ref
        ($f1 == $f2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep/sort with sub instead of block ────────────────────────────

#[test]
fn grep_with_coderef_callback_form() {
    let code = r#"
        my $is_even = sub { $_[0] % 2 == 0 };
        my @r = grep { $is_even->($_) } (1, 2, 3, 4, 5, 6);
        join(",", @r) eq "2,4,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── coderef inside class method body ──────────────────────────────

#[test]
fn coderef_returned_from_class_method() {
    let code = r#"
        class Codey {
            base: Int = 100
            fn make_adder { sub { $_[0] + $self->base } }
        }
        my $c = Codey();
        my $f = $c->make_adder;
        $f->(7) == 107 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty / pass-through coderef ──────────────────────────────────

#[test]
fn identity_coderef() {
    let code = r#"
        my $id = sub { $_[0] };
        $id->(42) == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
