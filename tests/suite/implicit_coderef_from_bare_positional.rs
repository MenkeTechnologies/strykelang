//! Stryke implicit zero-arg coderef sugar.
//!
//! `my $f = _ * 2;` at module top level is auto-wrapped to
//! `my $f = fn { _ * 2 };` so the bare-positional alias `_` (and `_1`, `_2`,
//! …) reads as an unbound parameter slot when there is no enclosing block to
//! supply a topic. Inside any block (`fn`, `map`, `grep`, `for`, …) bare `_`
//! is the bound topic and `my $i = _` keeps its capture meaning.

use stryke::ast::{ExprKind, Sigil, StmtKind};
use stryke::{parse, run};

#[test]
fn top_level_my_assigns_implicit_coderef_for_bare_underscore_rhs() {
    let v = run(r#"my $f = _ * 2; $f->(5);"#).expect("run");
    assert_eq!(v.to_string(), "10");
}

#[test]
fn top_level_my_implicit_coderef_supports_multiple_positionals() {
    let v = run(r#"my $g = _ + _1; $g->(3, 4);"#).expect("run");
    assert_eq!(v.to_string(), "7");
}

#[test]
fn top_level_my_implicit_coderef_only_first_positional() {
    let v = run(r#"my $h = _1; $h->(10, 20);"#).expect("run");
    assert_eq!(v.to_string(), "20");
}

#[test]
fn top_level_my_wraps_rhs_as_coderef_in_ast() {
    let p = parse(r#"my $f = _ * 2;"#).expect("parse");
    let StmtKind::My(decls) = &p.statements[0].kind else {
        panic!("expected my");
    };
    assert_eq!(decls.len(), 1);
    assert!(matches!(decls[0].sigil, Sigil::Scalar));
    let init = decls[0].initializer.as_ref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::CodeRef { .. }),
        "expected CodeRef wrap, got {:?}",
        init.kind
    );
}

#[test]
fn inside_block_bare_underscore_is_topic_capture_not_coderef() {
    // `my $i = _` inside a `map` block must read the topic, not produce a
    // coderef. Result list is `[2,4,6]`, not three coderefs.
    let v = run(r#"my @doubled = map { my $i = _; $i * 2 } 1, 2, 3;
                   join(",", @doubled);"#)
    .expect("run");
    assert_eq!(v.to_string(), "2,4,6");
}

#[test]
fn inside_fn_body_bare_underscore_is_first_arg_not_coderef() {
    let v = run(r#"fn dbl { my $x = _; $x * 2 }
                   dbl(21);"#)
    .expect("run");
    assert_eq!(v.to_string(), "42");
}

#[test]
fn inside_fn_body_my_is_not_wrapped_in_ast() {
    // The `my $x = _` inside the fn body must NOT become a CodeRef wrap; it
    // is a plain ScalarVar read of the topic.
    let p = parse(r#"fn dbl { my $x = _; $x * 2 }"#).expect("parse");
    // Walk to the inner my statement and confirm initializer is ScalarVar.
    let StmtKind::SubDecl { body, .. } = &p.statements[0].kind else {
        panic!("expected sub decl");
    };
    let inner_stmt = &body[0];
    let StmtKind::My(inner) = &inner_stmt.kind else {
        panic!("expected inner my, got {:?}", inner_stmt.kind);
    };
    let init = inner[0].initializer.as_ref().expect("inner initializer");
    assert!(
        matches!(init.kind, ExprKind::ScalarVar(_)),
        "expected inner my $x = _ to stay a ScalarVar read, got {:?}",
        init.kind
    );
}

#[test]
fn top_level_my_with_dollar_underscore_rhs_is_not_wrapped() {
    // Sigiled `$_` is a regular topic read, never an implicit coderef. Even
    // at top level this must remain a plain assignment (here against an
    // explicitly initialised topic via `local $_`).
    let v = run(r#"$_ = 7; my $x = $_ + 1; $x;"#).expect("run");
    assert_eq!(v.to_string(), "8");
}

#[test]
fn top_level_my_with_match_rhs_is_not_wrapped() {
    // `match { _ => ... }` arm-pattern `_` is `Pattern::Any`, not a topic
    // reference. The first RHS token is `match`, so the wrap heuristic must
    // leave the value as the match result, not a coderef.
    let v = run(
        r#"my $r = match (5) { _ if $_ > 10 => "big", _ => "small" };
                   $r;"#,
    )
    .expect("run");
    assert_eq!(v.to_string(), "small");
}

#[test]
fn top_level_my_with_literal_rhs_is_not_wrapped() {
    // No bare positional → no wrap.
    let v = run(r#"my $x = 1 + 2; $x;"#).expect("run");
    assert_eq!(v.to_string(), "3");
}
