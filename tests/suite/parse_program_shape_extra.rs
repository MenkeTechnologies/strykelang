//! Additional `Program` / `StmtKind` shape checks (explicit tests; no batching).

use perlrs::ast::{ExprKind, StmtKind};

#[test]
fn unless_statement_kind() {
    let p = perlrs::parse("unless (1) { 0; }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    assert!(matches!(p.statements[0].kind, StmtKind::Unless { .. }));
}

#[test]
fn until_loop_statement_kind() {
    let p = perlrs::parse("until (1) { }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    assert!(matches!(p.statements[0].kind, StmtKind::Until { .. }));
}

#[test]
fn do_while_statement_kind() {
    let p = perlrs::parse("do { 1 } while (0)").expect("parse");
    assert_eq!(p.statements.len(), 1);
    assert!(matches!(p.statements[0].kind, StmtKind::DoWhile { .. }));
}

#[test]
fn c_style_for_statement_kind() {
    let p = perlrs::parse("for (my $i = 0; $i < 2; $i = $i + 1) { 1; }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    assert!(matches!(p.statements[0].kind, StmtKind::For { .. }));
}

#[test]
fn our_declaration_statement_kind() {
    let p = perlrs::parse("our $g = 1").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Our(decls) = &p.statements[0].kind else {
        panic!("expected Our");
    };
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "g");
}

#[test]
fn local_declaration_statement_kind() {
    let p = perlrs::parse("local $l = 2").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Local(decls) = &p.statements[0].kind else {
        panic!("expected Local");
    };
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "l");
}

#[test]
fn my_list_declares_two_scalars() {
    let p = perlrs::parse("my ($a, $b)").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::My(decls) = &p.statements[0].kind else {
        panic!("expected My");
    };
    assert_eq!(decls.len(), 2);
    assert_eq!(decls[0].name, "a");
    assert_eq!(decls[1].name, "b");
}

#[test]
fn if_with_one_elsif_branch() {
    let p = perlrs::parse("if (0) { 1; } elsif (1) { 2; } else { 3; }").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::If { elsifs, .. } = &p.statements[0].kind else {
        panic!("expected If");
    };
    assert_eq!(elsifs.len(), 1);
}

#[test]
fn ternary_expression_shape() {
    let p = perlrs::parse("1 ? 2 : 3").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Expression(expr) = &p.statements[0].kind else {
        panic!("expected Expression");
    };
    assert!(matches!(expr.kind, ExprKind::Ternary { .. }));
}

#[test]
fn range_expression_shape() {
    let p = perlrs::parse("1..5").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Expression(expr) = &p.statements[0].kind else {
        panic!("expected Expression");
    };
    assert!(matches!(expr.kind, ExprKind::Range { .. }));
}

#[test]
fn repeat_operator_expression_shape() {
    let p = perlrs::parse(r#""a" x 3"#).expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Expression(expr) = &p.statements[0].kind else {
        panic!("expected Expression");
    };
    assert!(matches!(expr.kind, ExprKind::Repeat { .. }));
}

#[test]
fn qw_list_expression_shape() {
    let p = perlrs::parse("qw(a b)").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Expression(expr) = &p.statements[0].kind else {
        panic!("expected Expression");
    };
    let ExprKind::QW(words) = &expr.kind else {
        panic!("expected QW");
    };
    assert_eq!(words, &vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn undef_literal_expression_shape() {
    let p = perlrs::parse("undef").expect("parse");
    assert_eq!(p.statements.len(), 1);
    let StmtKind::Expression(expr) = &p.statements[0].kind else {
        panic!("expected Expression");
    };
    assert!(matches!(expr.kind, ExprKind::Undef));
}
