//! Detect trivial `map` / `grep` blocks that can run without AST interpretation per element.

use crate::ast::{BinOp, Block, Expr, ExprKind, StmtKind};

fn is_underscore(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::ScalarVar(s) if s == "_")
}

/// `map { $_ * k }` with integer constant `k`.
pub fn detect_map_int_mul(block: &Block) -> Option<i64> {
    if block.len() != 1 {
        return None;
    }
    let e = match &block[0].kind {
        StmtKind::Expression(e) => e,
        _ => return None,
    };
    match &e.kind {
        ExprKind::BinOp {
            left,
            op: BinOp::Mul,
            right,
        } if is_underscore(left) => match &right.kind {
            ExprKind::Integer(n) => Some(*n),
            _ => None,
        },
        ExprKind::BinOp {
            left,
            op: BinOp::Mul,
            right,
        } if is_underscore(right) => match &left.kind {
            ExprKind::Integer(n) => Some(*n),
            _ => None,
        },
        _ => None,
    }
}

/// `grep { $_ % m == r }` with integer constants (also `r == $_ % m`).
pub fn detect_grep_int_mod_eq(block: &Block) -> Option<(i64, i64)> {
    if block.len() != 1 {
        return None;
    }
    let e = match &block[0].kind {
        StmtKind::Expression(e) => e,
        _ => return None,
    };
    let (left, right) = match &e.kind {
        ExprKind::BinOp {
            left,
            op: BinOp::NumEq,
            right,
        } => (left, right),
        _ => return None,
    };
    // `($_ % m) == r`
    if let ExprKind::BinOp {
        left: l,
        op: BinOp::Mod,
        right: rm,
    } = &left.kind
    {
        if is_underscore(l) {
            if let (ExprKind::Integer(mv), ExprKind::Integer(rv)) = (&rm.kind, &right.kind) {
                if *mv != 0 {
                    return Some((*mv, *rv));
                }
            }
        }
    }
    // `r == ($_ % m)`
    if let ExprKind::BinOp {
        left: l,
        op: BinOp::Mod,
        right: rm,
    } = &right.kind
    {
        if is_underscore(l) {
            if let (ExprKind::Integer(rv), ExprKind::Integer(mv)) = (&left.kind, &rm.kind) {
                if *mv != 0 {
                    return Some((*mv, *rv));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_map_mul() {
        let p = crate::parse("map { $_ * 2 } (1);").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::Expression(Expr {
                kind: ExprKind::MapExpr { block, .. },
                ..
            }) => block,
            _ => panic!("expected map"),
        };
        assert_eq!(detect_map_int_mul(block), Some(2));
    }

    #[test]
    fn detects_grep_mod_eq() {
        let p = crate::parse("grep { $_ % 2 == 0 } (1);").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::Expression(Expr {
                kind: ExprKind::GrepExpr { block, .. },
                ..
            }) => block,
            _ => panic!("expected grep"),
        };
        assert_eq!(detect_grep_int_mod_eq(block), Some((2, 0)));
    }
}
