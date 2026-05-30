//! Regression pin: an identifier whose spelling matches one of stryke's
//! reserved infix-operator keywords (`eq`, `ne`, `lt`, `gt`, `le`, `ge`,
//! `cmp`, `and`, `or`, `not`, `x`) must lex as `Token::Ident` — not as
//! the operator token — in two positions:
//!
//!   * Leaf of a package-qualified path: `Mat::eq`, `Foo::Bar::and`, …
//!   * Method name after `->`:           `$obj->eq(99)`, `$ref->cmp()`, …
//!
//! Pre-fix: the lexer unconditionally routed bare identifiers through
//! `keyword_or_ident`. `Mat::eq` lexed as `Ident("Mat"), PackageSep,
//! StrEq`, and `parse_func_call_stmt`'s path-completion loop (which
//! only accepts `Ident` after `PackageSep`) rejected it. The method-call
//! form was worse: `$m->eq(99)` lexed as `ScalarVar("m"), Arrow, StrEq,
//! LParen, …` which silently fell through to `$m eq 99` evaluation
//! and returned `0` instead of dispatching to `Obj::eq`.
//!
//! Post-fix: when the just-read identifier starts immediately after `::`
//! OR immediately after `->`, the keyword conversion is skipped and
//! `Ident(leaf)` is emitted.

use crate::common::*;

#[test]
fn mat_eq_decl_and_call() {
    // The original report: `Mat::eq` collides with the `eq` operator.
    let v = eval_int(
        r#"
            fn Mat::eq($u, $v) { $u == $v ? 1 : 0 }
            Mat::eq(7, 7) + Mat::eq(7, 8) * 10
        "#,
    );
    assert_eq!(v, 1);
}

#[test]
fn every_op_keyword_works_as_package_leaf() {
    // All Perl-style word operators must be usable as the leaf segment
    // of a qualified function name. Each branch returns a distinct value
    // so a regression in any single leaf is locatable.
    let v = eval_int(
        r#"
            fn Op::eq  { 1 }
            fn Op::ne  { 2 }
            fn Op::lt  { 3 }
            fn Op::gt  { 4 }
            fn Op::le  { 5 }
            fn Op::ge  { 6 }
            fn Op::cmp { 7 }
            fn Op::and { 8 }
            fn Op::or  { 9 }
            fn Op::not { 10 }
            fn Op::x   { 11 }
            Op::eq() + Op::ne()*10 + Op::lt()*100 + Op::gt()*1000 + Op::le()*10_000 + Op::ge()*100_000 + Op::cmp()*1_000_000 + Op::and()*10_000_000 + Op::or()*100_000_000 + Op::not()*1_000_000_000 + Op::x()*10_000_000_000
        "#,
    );
    // 1 + 20 + 300 + 4000 + 50000 + 600000 + 7000000
    //   + 80000000 + 900000000 + 10000000000 + 110000000000
    assert_eq!(
        v,
        1
            + 20
            + 300
            + 4_000
            + 50_000
            + 600_000
            + 7_000_000
            + 80_000_000
            + 900_000_000
            + 10_000_000_000
            + 110_000_000_000,
    );
}

#[test]
fn three_level_path_with_op_keyword_leaf() {
    // `Foo::Bar::eq` — multi-segment path, op-keyword leaf. The
    // `after_package_sep` guard fires on every segment whose immediately
    // preceding chars are `::`, so the leaf at any depth is safe.
    let v = eval_int(
        r#"
            fn Foo::Bar::eq($a, $b) { $a + $b }
            Foo::Bar::eq(40, 2)
        "#,
    );
    assert_eq!(v, 42);
}

#[test]
fn bare_eq_operator_still_works_after_fix() {
    // The fix must only gate the keyword conversion when preceded by `::`.
    // Bare `eq` outside a path stays the string-equality infix operator.
    let v = eval_int(
        r#"
            "ab" eq "ab" ? 99 : -1
        "#,
    );
    assert_eq!(v, 99);
}

#[test]
fn bare_x_repetition_operator_still_works_after_fix() {
    // `x` is also a `keyword_or_ident` target. The fix must not break the
    // string-repetition operator either.
    let v = eval_string(
        r#"
            "ab" x 3
        "#,
    );
    assert_eq!(v, "ababab");
}

#[test]
fn method_call_with_op_keyword_name() {
    // The arrow gate: `$m->eq(99)` must dispatch to `Obj::eq`, not
    // collapse into `$m eq 99` (which would return 0 and silently mask
    // the call). Pre-fix the test asserted exactly the wrong result.
    let v = eval_int(
        r#"
            package Obj
            fn new($class) { bless +{}, $class }
            fn Obj::eq($self, $x) { 42 }

            package main
            my $m = Obj::new("Obj")
            $m->eq(99)
        "#,
    );
    assert_eq!(v, 42);
}

#[test]
fn every_op_keyword_works_as_method_name() {
    // Every op keyword that pre-fix would have been re-tokenized as an
    // infix operator after `->` must now reach method dispatch. Each
    // method returns a distinct value so a regression in any single
    // leaf is locatable.
    let v = eval_int(
        r#"
            package Obj
            fn new($class) { bless +{}, $class }
            fn Obj::eq  { 1 }
            fn Obj::ne  { 2 }
            fn Obj::lt  { 3 }
            fn Obj::gt  { 4 }
            fn Obj::le  { 5 }
            fn Obj::ge  { 6 }
            fn Obj::cmp { 7 }
            fn Obj::and { 8 }
            fn Obj::or  { 9 }
            fn Obj::not { 10 }
            fn Obj::x   { 11 }

            package main
            my $m = Obj::new("Obj")
            $m->eq() + $m->ne()*10 + $m->lt()*100 + $m->gt()*1000 + $m->le()*10_000 + $m->ge()*100_000 + $m->cmp()*1_000_000 + $m->and()*10_000_000 + $m->or()*100_000_000 + $m->not()*1_000_000_000 + $m->x()*10_000_000_000
        "#,
    );
    assert_eq!(
        v,
        1
            + 20
            + 300
            + 4_000
            + 50_000
            + 600_000
            + 7_000_000
            + 80_000_000
            + 900_000_000
            + 10_000_000_000
            + 110_000_000_000,
    );
}

#[test]
fn op_keyword_leaf_call_inside_an_expression() {
    // Mix the leaf-as-callee with an actual infix `eq` in the same
    // expression — the lexer must distinguish them by `::` adjacency.
    let v = eval_int(
        r#"
            fn Cmp::eq($a, $b) { $a eq $b ? 1 : 0 }
            Cmp::eq("hi", "hi") + Cmp::eq("hi", "ho") * 10
        "#,
    );
    assert_eq!(v, 1);
}
