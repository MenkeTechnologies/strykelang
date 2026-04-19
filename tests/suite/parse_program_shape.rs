//! Parsed `Program` shape checks (explicit tests; no batching).

use forge::ast::{ExprKind, StmtKind};

#[test]
fn empty_source_yields_empty_statement_list() {
    let p = forge::parse("").expect("parse");
    assert!(p.statements.is_empty());
}

#[test]
fn semicolons_only_parse_to_program_without_panicking() {
    let p = forge::parse(";;").expect("parse");
    assert!(
        p.statements.is_empty()
            || p.statements
                .iter()
                .all(|s| matches!(s.kind, StmtKind::Empty))
    );
}

#[test]
fn three_expression_statements_distinct_lines() {
    let p = forge::parse("1;\n2;\n3").expect("parse");
    assert_eq!(p.statements.len(), 3);
    for (i, stmt) in p.statements.iter().enumerate() {
        let StmtKind::Expression(expr) = &stmt.kind else {
            panic!("stmt {i}: expected Expression");
        };
        let ExprKind::Integer(n) = &expr.kind else {
            panic!("stmt {i}: expected integer literal");
        };
        assert_eq!(*n, (i as i64) + 1);
    }
}

#[test]
fn sub_decl_then_call_without_semicolon_is_two_statements() {
    let p = forge::parse("sub foo { return 5 } foo()").expect("parse");
    assert_eq!(
        p.statements.len(),
        2,
        "expected sub stmt then call stmt; got {:?}",
        p.statements
            .iter()
            .map(|s| std::mem::discriminant(&s.kind))
            .collect::<Vec<_>>()
    );
}

#[test]
fn sub_declaration_statement_kind() {
    let p = forge::parse("sub foo { return 1; }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::SubDecl { name, .. } = &p.statements[0].kind else {
        panic!("expected SubDecl");
    };
    assert_eq!(name, "foo");
}

#[test]
fn package_statement_kind() {
    let p = forge::parse("package Bar::Baz").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Package { name } = &p.statements[0].kind else {
        panic!("expected Package");
    };
    assert_eq!(name, "Bar::Baz");
}

#[test]
fn use_strict_statement_kind() {
    let p = forge::parse("use strict").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Use { module, .. } = &p.statements[0].kind else {
        panic!("expected Use");
    };
    assert_eq!(module, "strict");
}

#[test]
fn no_warnings_statement_kind() {
    let p = forge::parse("no warnings").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::No { module, .. } = &p.statements[0].kind else {
        panic!("expected No");
    };
    assert_eq!(module, "warnings");
}

#[test]
fn my_scalar_declaration_statement_kind() {
    let p = forge::parse("my $x = 10").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::My(decls) = &p.statements[0].kind else {
        panic!("expected My");
    };
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "x");
}

#[test]
fn if_statement_has_else_branch_in_ast() {
    let p = forge::parse("if (0) { 1; } else { 2; }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::If {
        else_block, elsifs, ..
    } = &p.statements[0].kind
    else {
        panic!("expected If");
    };
    assert!(elsifs.is_empty());
    assert!(else_block.is_some());
}

#[test]
fn while_loop_statement_kind() {
    let p = forge::parse("while (0) { }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    assert!(matches!(p.statements[0].kind, StmtKind::While { .. }));
}

#[test]
fn foreach_statement_kind() {
    let p = forge::parse("foreach my $k (1, 2) { $k; }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Foreach { var, .. } = &p.statements[0].kind else {
        panic!("expected Foreach");
    };
    assert_eq!(var, "k");
}

#[test]
fn begin_block_statement_kind() {
    let p = forge::parse("BEGIN { 1; }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    assert!(matches!(p.statements[0].kind, StmtKind::Begin(_)));
}

#[test]
fn end_block_statement_kind() {
    let p = forge::parse("END { 1; }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    assert!(matches!(p.statements[0].kind, StmtKind::End(_)));
}

#[test]
fn return_statement_kind() {
    let p = forge::parse("return 42").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Return(Some(expr)) = &p.statements[0].kind else {
        panic!("expected Return with expr");
    };
    assert!(matches!(expr.kind, ExprKind::Integer(42)));
}

#[test]
fn bare_return_statement_kind() {
    let p = forge::parse("return").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Return(None) = &p.statements[0].kind else {
        panic!("expected Return without expr");
    };
}

#[test]
fn last_next_redo_statement_kinds() {
    let p = forge::parse("last; next; redo").expect("parse");
    assert_eq!(p.statements.len(), 3);
    assert!(matches!(p.statements[0].kind, StmtKind::Last(None)));
    assert!(matches!(p.statements[1].kind, StmtKind::Next(None)));
    assert!(matches!(p.statements[2].kind, StmtKind::Redo(None)));
}

#[test]
fn binary_add_expression_in_statement() {
    let p = forge::parse("7 + 8").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Expression(expr) = &p.statements[0].kind else {
        panic!("expected Expression");
    };
    let ExprKind::BinOp { op, .. } = &expr.kind else {
        panic!("expected BinOp");
    };
    use forge::ast::BinOp;
    assert_eq!(*op, BinOp::Add);
}

#[test]
fn regex_literal_expression_kind() {
    let p = forge::parse("m/abc/").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Expression(expr) = &p.statements[0].kind else {
        panic!("expected Expression");
    };
    let ExprKind::Regex(_, flags) = &expr.kind else {
        panic!("expected Regex");
    };
    assert!(flags.is_empty());
}
