//! Unit tests that `parse()` produces the expected `StmtKind` / `ExprKind` shapes.

use crate::ast::{BinOp, ExprKind, Sigil, StmtKind};
use crate::parse;

fn first_stmt(code: &str) -> StmtKind {
    let p = parse(code).expect("parse");
    assert!(!p.statements.is_empty(), "{code}");
    p.statements[0].kind.clone()
}

fn first_expr_kind(code: &str) -> ExprKind {
    let p = parse(code).expect("parse");
    let sk = &p.statements[0].kind;
    match sk {
        StmtKind::Expression(e) => e.kind.clone(),
        _ => panic!("expected expression stmt"),
    }
}

/// `local $SIG{__WARN__}` — dynamic localization on a hash slot (Exporter.pm).
#[test]
fn shape_local_hash_element() {
    let k = first_stmt("local $SIG{__WARN__} = 1;");
    assert!(matches!(k, StmtKind::LocalExpr { .. }));
}

/// Bareword `FOO` in `if (FOO)` vs quoted `'FOO'` — distinct [`ExprKind`](crate::ast::ExprKind) for sub/constant resolution.
#[test]
fn shape_bareword_vs_quoted_in_if() {
    let p = parse("if (FOO) { }").expect("parse");
    match &p.statements[0].kind {
        StmtKind::If { condition, .. } => {
            assert!(matches!(condition.kind, ExprKind::Bareword(_)));
        }
        _ => panic!("expected if"),
    }
    let p2 = parse("if ('FOO') { }").expect("parse");
    match &p2.statements[0].kind {
        StmtKind::If { condition, .. } => {
            assert!(matches!(condition.kind, ExprKind::String(_)));
        }
        _ => panic!("expected if"),
    }
}

/// `%$href` hash dereference (Exporter `not %$export_cache`).
#[test]
fn shape_percent_scalar_hash_deref() {
    let k = first_expr_kind("%$export_cache");
    assert!(matches!(
        k,
        ExprKind::Deref {
            kind: Sigil::Hash,
            ..
        }
    ));
}

/// Symbolic array: `@{ "Pkg::EXPORT" }` / `\@{ ... }` (Exporter.pm).
#[test]
fn shape_symbolic_array_braces() {
    let k = first_expr_kind(r##"@{ "Foo::EXPORT" }"##);
    assert!(matches!(k, ExprKind::Deref { kind: Sigil::Array, .. }));
    let r = first_expr_kind(r##"\@{ "Foo::EXPORT" }"##);
    let ExprKind::ScalarRef(inner) = r else {
        panic!("expected scalar ref");
    };
    assert!(matches!(
        inner.kind,
        ExprKind::Deref {
            kind: Sigil::Array,
            ..
        }
    ));
}

/// Dynamic coderef and typeglob slot syntax (Exporter.pm loads).
#[test]
fn shape_dynamic_subref_and_typeglob_expr() {
    let k = first_expr_kind(r##"\&{"Foo::bar"}"##);
    assert!(matches!(k, ExprKind::DynamicSubCodeRef(_)));
    let t = first_expr_kind(r##"*{"Foo::bar"}"##);
    assert!(matches!(t, ExprKind::TypeglobExpr(_)));
}

/// `$coderef(...)` — indirect call (core `B.pm` / `walksymtable`).
#[test]
fn shape_indirect_scalar_call() {
    let k = first_expr_kind("$cr($x);");
    let ExprKind::IndirectCall {
        target,
        args,
        ampersand,
        pass_caller_arglist,
    } = k
    else {
        panic!("expected IndirectCall");
    };
    assert!(!ampersand);
    assert!(!pass_caller_arglist);
    assert!(matches!(target.kind, ExprKind::ScalarVar(ref s) if s == "cr"));
    assert_eq!(args.len(), 1);
}

/// `&$coderef(...)` — ampersand form (e.g. `&$recurse($sym)` in core `B.pm`).
#[test]
fn shape_ampersand_indirect_call() {
    let k = first_expr_kind(r##"&$recurse($sym);"##);
    let ExprKind::IndirectCall {
        target,
        args,
        ampersand,
        pass_caller_arglist,
    } = k
    else {
        panic!("expected IndirectCall");
    };
    assert!(ampersand);
    assert!(!pass_caller_arglist);
    assert!(matches!(target.kind, ExprKind::ScalarVar(ref s) if s == "recurse"));
    assert_eq!(args.len(), 1);
}

/// `&$cr` with no `(...)` — passes caller's `@_` at runtime ([`ExprKind::IndirectCall::pass_caller_arglist`]).
#[test]
fn shape_goto_postfix_if() {
    let p = parse("goto &$boots if defined &$boots;").expect("parse");
    let StmtKind::If { body, .. } = &p.statements[0].kind else {
        panic!("expected if-wrapped goto (XSLoader.pm style)");
    };
    assert_eq!(body.len(), 1);
    assert!(matches!(body[0].kind, StmtKind::Goto { .. }));
}

/// Mixed-case `boot:` — system `XSLoader.pm` uses this before `my $xs = …` (not only `FOO:`).
#[test]
fn shape_statement_label_mixed_case_boot() {
    let p = parse("boot: my $xs = 0;").expect("parse");
    assert_eq!(p.statements[0].label.as_deref(), Some("boot"));
    assert!(matches!(p.statements[0].kind, StmtKind::My(_)));
}

#[test]
fn shape_ampersand_scalar_no_paren_passes_caller_arglist() {
    let k = first_expr_kind(r##"&$cb;"##);
    let ExprKind::IndirectCall {
        target,
        args,
        ampersand,
        pass_caller_arglist,
    } = k
    else {
        panic!("expected IndirectCall");
    };
    assert!(ampersand);
    assert!(pass_caller_arglist);
    assert!(args.is_empty());
    assert!(matches!(target.kind, ExprKind::ScalarVar(ref s) if s == "cb"));
}

/// Parenthesized `sort $coderef (LIST)` in a ternary then-branch (JSON::PP-style) must keep parens balanced.
#[test]
fn shape_sort_coderef_paren_list_inside_ternary_then() {
    let k = first_expr_kind("1 ? (sort $k (1)) : 0");
    let ExprKind::Ternary { then_expr, .. } = k else {
        panic!("expected ternary");
    };
    let ExprKind::SortExpr { cmp, .. } = then_expr.kind else {
        panic!("expected sort in then branch");
    };
    assert!(cmp.is_some(), "sort with coderef comparator must keep cmp");
}

#[test]
fn shape_if_block() {
    assert!(matches!(first_stmt("if (1) { 2; }"), StmtKind::If { .. }));
}

#[test]
fn shape_unless_block() {
    assert!(matches!(
        first_stmt("unless (0) { 1; }"),
        StmtKind::Unless { .. }
    ));
}

#[test]
fn shape_while_loop() {
    assert!(matches!(
        first_stmt("while (0) { 1; }"),
        StmtKind::While { .. }
    ));
}

#[test]
fn shape_until_loop() {
    assert!(matches!(
        first_stmt("until (1) { 1; }"),
        StmtKind::Until { .. }
    ));
}

#[test]
fn shape_for_c_style() {
    assert!(matches!(
        first_stmt("for (my $i = 0; $i < 1; $i++) { 1; }"),
        StmtKind::For { .. }
    ));
}

#[test]
fn shape_foreach() {
    assert!(matches!(
        first_stmt("foreach my $x (1, 2) { $x; }"),
        StmtKind::Foreach { .. }
    ));
}

#[test]
fn shape_sub_decl() {
    assert!(matches!(
        first_stmt("sub foo { 1; }"),
        StmtKind::SubDecl { .. }
    ));
}

/// `sub Pkg::name { }` (core `B.pm`, `Exporter.pm`, …).
#[test]
fn shape_sub_decl_qualified_name() {
    match first_stmt("sub B::GV::SAFENAME { 1; }") {
        StmtKind::SubDecl { name, .. } => assert_eq!(name, "B::GV::SAFENAME"),
        _ => panic!("expected SubDecl"),
    }
}

#[test]
fn shape_package() {
    assert!(matches!(
        first_stmt("package Foo::Bar;"),
        StmtKind::Package { .. }
    ));
}

#[test]
fn shape_use_no() {
    assert!(matches!(first_stmt("use strict;"), StmtKind::Use { .. }));
    assert!(matches!(
        first_stmt("use 5.008;"),
        StmtKind::UsePerlVersion { .. }
    ));
    assert!(matches!(first_stmt("use 5;"), StmtKind::UsePerlVersion { .. }));
    assert!(matches!(
        first_stmt("use overload ();"),
        StmtKind::UseOverload { pairs } if pairs.is_empty()
    ));
    assert!(matches!(first_stmt("no warnings;"), StmtKind::No { .. }));
}

#[test]
fn shape_my_our_local() {
    assert!(matches!(first_stmt("my $x;"), StmtKind::My(_)));
    assert!(matches!(first_stmt("our $y;"), StmtKind::Our(_)));
    assert!(matches!(first_stmt("local $z;"), StmtKind::Local(_)));
}

#[test]
fn shape_return_last_next_redo() {
    assert!(matches!(first_stmt("return 1;"), StmtKind::Return(_)));
    assert!(matches!(first_stmt("last;"), StmtKind::Last(_)));
    assert!(matches!(first_stmt("next;"), StmtKind::Next(_)));
    assert!(matches!(first_stmt("redo;"), StmtKind::Redo(_)));
}

#[test]
fn shape_begin_end_blocks() {
    assert!(matches!(first_stmt("BEGIN { 1; }"), StmtKind::Begin(_)));
    assert!(matches!(first_stmt("END { 1; }"), StmtKind::End(_)));
}

#[test]
fn shape_leading_semicolon_is_empty_statement() {
    let p = parse(";;").expect("parse");
    assert_eq!(p.statements.len(), 2);
    assert!(matches!(p.statements[0].kind, StmtKind::Empty));
    assert!(matches!(p.statements[1].kind, StmtKind::Empty));
}

#[test]
fn expr_binop_add() {
    assert!(matches!(
        first_expr_kind("1 + 2;"),
        ExprKind::BinOp { op: BinOp::Add, .. }
    ));
}

#[test]
fn expr_binop_pow() {
    assert!(matches!(
        first_expr_kind("2 ** 3;"),
        ExprKind::BinOp { op: BinOp::Pow, .. }
    ));
}

#[test]
fn expr_ternary() {
    assert!(matches!(
        first_expr_kind("1 ? 2 : 3;"),
        ExprKind::Ternary { .. }
    ));
}

#[test]
fn expr_repeat() {
    assert!(matches!(
        first_expr_kind(r#""a" x 3;"#),
        ExprKind::Repeat { .. }
    ));
}

#[test]
fn expr_range() {
    assert!(matches!(first_expr_kind("1..10;"), ExprKind::Range { .. }));
}

#[test]
fn expr_scalar_var() {
    assert!(matches!(
        first_expr_kind("$foo;"),
        ExprKind::ScalarVar(ref s) if s == "foo"
    ));
}

#[test]
fn expr_array_var() {
    assert!(matches!(
        first_expr_kind("@arr;"),
        ExprKind::ArrayVar(ref s) if s == "arr"
    ));
}

#[test]
fn expr_hash_var() {
    assert!(matches!(
        first_expr_kind("%h;"),
        ExprKind::HashVar(ref s) if s == "h"
    ));
}

#[test]
fn expr_array_element() {
    assert!(matches!(
        first_expr_kind("$a[0];"),
        ExprKind::ArrayElement { .. }
    ));
}

#[test]
fn expr_hash_element() {
    assert!(matches!(
        first_expr_kind("$h{key};"),
        ExprKind::HashElement { .. }
    ));
}

#[test]
fn expr_length_builtin() {
    assert!(matches!(
        first_expr_kind("length('ab');"),
        ExprKind::Length(_)
    ));
}

#[test]
fn expr_print_say() {
    assert!(matches!(
        first_expr_kind("print 1;"),
        ExprKind::Print { .. }
    ));
    assert!(matches!(first_expr_kind("say 1;"), ExprKind::Say { .. }));
}

#[test]
fn expr_undef_literal() {
    assert!(matches!(first_expr_kind("undef;"), ExprKind::Undef));
}

#[test]
fn expr_integer_float_string() {
    assert!(matches!(first_expr_kind("42;"), ExprKind::Integer(42)));
    assert!(matches!(first_expr_kind("1.5;"), ExprKind::Float(f) if (f - 1.5).abs() < 1e-9));
    assert!(matches!(
        first_expr_kind("'hi';"),
        ExprKind::String(ref s) if s == "hi"
    ));
}

#[test]
fn expr_regex_literal_token_form() {
    // Statement-level `m//` is often parsed as a regex literal expression.
    assert!(matches!(
        first_expr_kind("m/pattern/;"),
        ExprKind::Regex(_, _) | ExprKind::Match { .. }
    ));
}

#[test]
fn expr_substitution_form() {
    assert!(matches!(
        first_expr_kind("s/a/b/;"),
        ExprKind::Substitution { .. }
    ));
}

#[test]
fn expr_transliterate_form() {
    assert!(matches!(
        first_expr_kind("tr/a/b/;"),
        ExprKind::Transliterate { .. }
    ));
}

#[test]
fn expr_map_grep_sort() {
    assert!(matches!(
        first_expr_kind("map { $_ } (1);"),
        ExprKind::MapExpr { .. }
    ));
    assert!(matches!(
        first_expr_kind("grep { $_ } (1);"),
        ExprKind::GrepExpr { .. }
    ));
    assert!(matches!(
        first_expr_kind("grep -e \"x\", (1);"),
        ExprKind::GrepExprComma { .. }
    ));
    assert!(matches!(
        first_expr_kind("sort (1, 2);"),
        ExprKind::SortExpr { .. }
    ));
}

#[test]
fn expr_list_paren_parses() {
    parse("(1, 2, 3);").expect("list expr");
}

#[test]
fn stmt_block_bare() {
    assert!(matches!(first_stmt("{ 1; }"), StmtKind::Block(_)));
}

#[test]
fn shape_eval_block_stmt() {
    assert!(matches!(
        first_stmt("eval { 1; };"),
        StmtKind::Expression(_)
    ));
}

#[test]
fn shape_require_do_string() {
    assert!(matches!(
        first_stmt("require strict;"),
        StmtKind::Expression(_)
    ));
    assert!(matches!(
        first_stmt("do 'lib.pl';"),
        StmtKind::Expression(_)
    ));
}

#[test]
fn expr_postfix_increment_second_statement() {
    let p = parse("my $i = 0; $i++;").expect("parse");
    assert!(p.statements.len() >= 2);
    match &p.statements[1].kind {
        StmtKind::Expression(e) => {
            assert!(matches!(e.kind, ExprKind::PostfixOp { .. }));
        }
        _ => panic!("expected expression statement for postfix"),
    }
}
