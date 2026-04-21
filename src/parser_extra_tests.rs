//! Extra tests for `Parser` to ensure correct AST generation and precedence.

use crate::ast::{BinOp, ExprKind, StmtKind};
use crate::parse;

fn first_expr_kind(code: &str) -> ExprKind {
    let p = parse(code).expect("parse");
    let sk = &p.statements[0].kind;
    match sk {
        StmtKind::Expression(e) => e.kind.clone(),
        _ => panic!("expected expression stmt"),
    }
}

#[test]
fn test_precedence_arithmetic() {
    // 2 + 3 * 4 should be 2 + (3 * 4)
    let k = first_expr_kind("2 + 3 * 4;");
    if let ExprKind::BinOp { op, left, right } = k {
        assert_eq!(op, BinOp::Add);
        assert!(matches!(left.kind, ExprKind::Integer(2)));
        if let ExprKind::BinOp { op, .. } = right.kind {
            assert_eq!(op, BinOp::Mul);
        } else {
            panic!("expected Mul on right");
        }
    } else {
        panic!("expected Add at top level");
    }
}

#[test]
fn test_precedence_logical() {
    // 1 || 0 && 0 should be 1 || (0 && 0) if && is higher than ||
    let k = first_expr_kind("1 || 0 && 0;");
    if let ExprKind::BinOp { op, right, .. } = k {
        assert_eq!(op, BinOp::LogOr);
        if let ExprKind::BinOp { op, .. } = right.kind {
            assert_eq!(op, BinOp::LogAnd);
        }
    }
}

#[test]
fn test_nested_structures_parsing() {
    // my $x = [ { a => 1 } ];
    let p = parse("my $x = [ { a => 1 } ];").expect("parse");
    let StmtKind::My(decls) = &p.statements[0].kind else {
        panic!("expected my")
    };
    let init = decls[0].initializer.as_ref().unwrap();
    assert!(matches!(init.kind, ExprKind::ArrayRef(_)));
}

#[test]
fn test_pipe_forward_basic_desugaring() {
    // 42 |> p  should desugar to p(42)
    let k = first_expr_kind("42 |> p;");
    if let ExprKind::Say { args, .. } = k {
        assert_eq!(args.len(), 1);
        assert!(matches!(args[0].kind, ExprKind::Integer(42)));
    } else {
        panic!(
            "expected Say (desugared from PipeForward) at top level, got {:?}",
            k
        );
    }
}

#[test]
fn test_ternary_precedence() {
    // $a ? $b : $c || $d  should be $a ? $b : ($c || $d)
    let k = first_expr_kind("$a ? $b : $c || $d;");
    if let ExprKind::Ternary { else_expr, .. } = k {
        assert!(matches!(
            else_expr.kind,
            ExprKind::BinOp {
                op: BinOp::LogOr,
                ..
            }
        ));
    } else {
        panic!("expected Ternary");
    }
}

#[test]
fn test_sub_call_no_parens() {
    let k = first_expr_kind("p 42;");
    if let ExprKind::Say { args, .. } = k {
        assert_eq!(args.len(), 1);
    } else {
        panic!("expected Say");
    }
}
