//! Binary and unary operator expression shapes in the AST.

use perlrs::ast::{BinOp, ExprKind, StmtKind, UnaryOp};

#[test]
fn binop_subtract() {
    let p = perlrs::parse("9 - 4").expect("parse");
    let StmtKind::Expression(e) = &p.statements[0].kind else {
        panic!("expr stmt");
    };
    let ExprKind::BinOp { op, .. } = &e.kind else {
        panic!("binop");
    };
    assert_eq!(*op, BinOp::Sub);
}

#[test]
fn binop_multiply() {
    let p = perlrs::parse("3 * 4").expect("parse");
    let StmtKind::Expression(e) = &p.statements[0].kind else {
        panic!("expr stmt");
    };
    let ExprKind::BinOp { op, .. } = &e.kind else {
        panic!("binop");
    };
    assert_eq!(*op, BinOp::Mul);
}

#[test]
fn binop_divide() {
    let p = perlrs::parse("8 / 2").expect("parse");
    let StmtKind::Expression(e) = &p.statements[0].kind else {
        panic!("expr stmt");
    };
    let ExprKind::BinOp { op, .. } = &e.kind else {
        panic!("binop");
    };
    assert_eq!(*op, BinOp::Div);
}

#[test]
fn binop_modulo() {
    let p = perlrs::parse("7 % 3").expect("parse");
    let StmtKind::Expression(e) = &p.statements[0].kind else {
        panic!("expr stmt");
    };
    let ExprKind::BinOp { op, .. } = &e.kind else {
        panic!("binop");
    };
    assert_eq!(*op, BinOp::Mod);
}

#[test]
fn binop_power() {
    let p = perlrs::parse("2 ** 8").expect("parse");
    let StmtKind::Expression(e) = &p.statements[0].kind else {
        panic!("expr stmt");
    };
    let ExprKind::BinOp { op, .. } = &e.kind else {
        panic!("binop");
    };
    assert_eq!(*op, BinOp::Pow);
}

#[test]
fn unary_numeric_negate() {
    let p = perlrs::parse("-15").expect("parse");
    let StmtKind::Expression(e) = &p.statements[0].kind else {
        panic!("expr stmt");
    };
    let ExprKind::UnaryOp { op, .. } = &e.kind else {
        panic!("unary");
    };
    assert_eq!(*op, UnaryOp::Negate);
}

#[test]
fn unary_logical_not_bang() {
    let p = perlrs::parse("!0").expect("parse");
    let StmtKind::Expression(e) = &p.statements[0].kind else {
        panic!("expr stmt");
    };
    let ExprKind::UnaryOp { op, .. } = &e.kind else {
        panic!("unary");
    };
    assert_eq!(*op, UnaryOp::LogNot);
}

#[test]
fn unary_bitwise_not_tilde() {
    let p = perlrs::parse("~0").expect("parse");
    let StmtKind::Expression(e) = &p.statements[0].kind else {
        panic!("expr stmt");
    };
    let ExprKind::UnaryOp { op, .. } = &e.kind else {
        panic!("unary");
    };
    assert_eq!(*op, UnaryOp::BitNot);
}
