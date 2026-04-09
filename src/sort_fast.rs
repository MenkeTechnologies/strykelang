//! Detect `{ $a <=> $b }` / `{ $a cmp $b }` for native sort (no per-compare interpreter).

use std::cmp::Ordering;

use crate::ast::{BinOp, Block, Expr, ExprKind, StmtKind};
use crate::value::PerlValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBlockFast {
    Numeric,
    String,
    /// `{ $b <=> $a }` — reverse numeric order
    NumericRev,
    /// `{ $b cmp $a }` — reverse string order
    StringRev,
}

fn is_magic_a(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::ScalarVar(s) if s == "a")
}

fn is_magic_b(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::ScalarVar(s) if s == "b")
}

/// Single-statement block `{ $a <=> $b }` or `{ $a cmp $b }`.
pub fn detect_sort_block_fast(block: &Block) -> Option<SortBlockFast> {
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
            op: BinOp::Spaceship,
            right,
        } if is_magic_a(left) && is_magic_b(right) => Some(SortBlockFast::Numeric),
        ExprKind::BinOp {
            left,
            op: BinOp::Spaceship,
            right,
        } if is_magic_b(left) && is_magic_a(right) => Some(SortBlockFast::NumericRev),
        ExprKind::BinOp {
            left,
            op: BinOp::StrCmp,
            right,
        } if is_magic_a(left) && is_magic_b(right) => Some(SortBlockFast::String),
        ExprKind::BinOp {
            left,
            op: BinOp::StrCmp,
            right,
        } if is_magic_b(left) && is_magic_a(right) => Some(SortBlockFast::StringRev),
        _ => None,
    }
}

#[inline]
pub fn sort_magic_cmp(a: &PerlValue, b: &PerlValue, mode: SortBlockFast) -> Ordering {
    match mode {
        SortBlockFast::Numeric => a
            .to_number()
            .partial_cmp(&b.to_number())
            .unwrap_or(Ordering::Equal),
        SortBlockFast::String => a.to_string().cmp(&b.to_string()),
        SortBlockFast::NumericRev => sort_magic_cmp(b, a, SortBlockFast::Numeric),
        SortBlockFast::StringRev => sort_magic_cmp(b, a, SortBlockFast::String),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expr, ExprKind, StmtKind};

    #[test]
    fn detects_spaceship_ab_from_sort_expr() {
        let p = crate::parse("sort { $a <=> $b } (3, 1, 2);").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::Expression(Expr {
                kind: ExprKind::SortExpr { cmp: Some(b), .. },
                ..
            }) => b,
            _ => panic!("expected sort"),
        };
        assert_eq!(detect_sort_block_fast(block), Some(SortBlockFast::Numeric));
    }

    #[test]
    fn detects_cmp_ab_from_sub_body() {
        let p = crate::parse("sub cmpab { $a cmp $b }").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::SubDecl { body, .. } => body,
            _ => panic!("expected sub"),
        };
        assert_eq!(detect_sort_block_fast(block), Some(SortBlockFast::String));
    }

    #[test]
    fn detects_reverse_spaceship_and_cmp() {
        let p = crate::parse("sort { $b <=> $a } (1);").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::Expression(Expr {
                kind: ExprKind::SortExpr { cmp: Some(b), .. },
                ..
            }) => b,
            _ => panic!("expected sort"),
        };
        assert_eq!(
            detect_sort_block_fast(block),
            Some(SortBlockFast::NumericRev)
        );
        let p2 = crate::parse("sort { $b cmp $a } (1);").expect("parse");
        let block2 = match &p2.statements[0].kind {
            StmtKind::Expression(Expr {
                kind: ExprKind::SortExpr { cmp: Some(b), .. },
                ..
            }) => b,
            _ => panic!("expected sort"),
        };
        assert_eq!(
            detect_sort_block_fast(block2),
            Some(SortBlockFast::StringRev)
        );
    }

    #[test]
    fn detect_sort_block_fast_rejects_empty_block() {
        let block: Block = vec![];
        assert_eq!(detect_sort_block_fast(&block), None);
    }

    #[test]
    fn detect_sort_block_fast_rejects_multi_statement_block() {
        let p = crate::parse("sub two_stmt { $a <=> $b; 1; }").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::SubDecl { body, .. } => body,
            _ => panic!("expected sub"),
        };
        assert_eq!(detect_sort_block_fast(block), None);
    }

    #[test]
    fn sort_magic_cmp_numeric_ordering() {
        use crate::value::PerlValue;
        let a = PerlValue::integer(1);
        let b = PerlValue::integer(2);
        assert_eq!(
            sort_magic_cmp(&a, &b, SortBlockFast::Numeric),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            sort_magic_cmp(&a, &b, SortBlockFast::NumericRev),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn sort_magic_cmp_string_ordering() {
        use crate::value::PerlValue;
        let a = PerlValue::string("a".into());
        let b = PerlValue::string("z".into());
        assert_eq!(
            sort_magic_cmp(&a, &b, SortBlockFast::String),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            sort_magic_cmp(&a, &b, SortBlockFast::StringRev),
            std::cmp::Ordering::Greater
        );
    }
}
