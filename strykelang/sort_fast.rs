//! Detect `{ $a <=> $b }` / `{ $a cmp $b }` for native sort (no per-compare interpreter).

use std::cmp::Ordering;

use crate::ast::{BinOp, Block, Expr, ExprKind, StmtKind};
use crate::value::StrykeValue;
/// `SortBlockFast` — see variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBlockFast {
    /// `Numeric` variant.
    Numeric,
    /// `String` variant.
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
/// `sort_magic_cmp` — see implementation.
#[inline]
pub fn sort_magic_cmp(a: &StrykeValue, b: &StrykeValue, mode: SortBlockFast) -> Ordering {
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
    use crate::ast::{Expr, ExprKind, SortComparator, StmtKind};

    #[test]
    fn detects_spaceship_ab_from_sort_expr() {
        let p = crate::parse("sort { $a <=> $b } (3, 1, 2);").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::Expression(Expr {
                kind:
                    ExprKind::SortExpr {
                        cmp: Some(SortComparator::Block(b)),
                        ..
                    },
                ..
            }) => b,
            _ => panic!("expected sort"),
        };
        assert_eq!(detect_sort_block_fast(block), Some(SortBlockFast::Numeric));
    }

    #[test]
    fn detects_cmp_ab_from_sub_body() {
        let p = crate::parse("fn cmpab { $a cmp $b }").expect("parse");
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
                kind:
                    ExprKind::SortExpr {
                        cmp: Some(SortComparator::Block(b)),
                        ..
                    },
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
                kind:
                    ExprKind::SortExpr {
                        cmp: Some(SortComparator::Block(b)),
                        ..
                    },
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
        let p = crate::parse("fn two_stmt { $a <=> $b; 1; }").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::SubDecl { body, .. } => body,
            _ => panic!("expected sub"),
        };
        assert_eq!(detect_sort_block_fast(block), None);
    }

    #[test]
    fn sort_magic_cmp_numeric_ordering() {
        use crate::value::StrykeValue;
        let a = StrykeValue::integer(1);
        let b = StrykeValue::integer(2);
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
        use crate::value::StrykeValue;
        let a = StrykeValue::string("a".into());
        let b = StrykeValue::string("z".into());
        assert_eq!(
            sort_magic_cmp(&a, &b, SortBlockFast::String),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            sort_magic_cmp(&a, &b, SortBlockFast::StringRev),
            std::cmp::Ordering::Greater
        );
    }

    // ── sort_magic_cmp equality and reflexivity ──────────────────────

    #[test]
    fn sort_magic_cmp_numeric_equal_returns_equal_in_all_modes() {
        use crate::value::StrykeValue;
        let a = StrykeValue::integer(5);
        let b = StrykeValue::integer(5);
        for mode in [
            SortBlockFast::Numeric,
            SortBlockFast::NumericRev,
            SortBlockFast::String,
            SortBlockFast::StringRev,
        ] {
            assert_eq!(
                sort_magic_cmp(&a, &b, mode),
                Ordering::Equal,
                "mode {mode:?}"
            );
        }
    }

    #[test]
    fn sort_magic_cmp_numeric_handles_float_values() {
        use crate::value::StrykeValue;
        let a = StrykeValue::float(1.5);
        let b = StrykeValue::float(1.5001);
        assert_eq!(
            sort_magic_cmp(&a, &b, SortBlockFast::Numeric),
            Ordering::Less
        );
        assert_eq!(
            sort_magic_cmp(&b, &a, SortBlockFast::Numeric),
            Ordering::Greater
        );
    }

    #[test]
    fn sort_magic_cmp_string_compares_lexically_not_numerically() {
        use crate::value::StrykeValue;
        // "10" < "2" lexicographically because '1' < '2'.
        let a = StrykeValue::string("10".into());
        let b = StrykeValue::string("2".into());
        assert_eq!(
            sort_magic_cmp(&a, &b, SortBlockFast::String),
            Ordering::Less
        );
        assert_eq!(
            sort_magic_cmp(&a, &b, SortBlockFast::StringRev),
            Ordering::Greater
        );
    }

    #[test]
    fn sort_magic_cmp_rev_is_inverse_of_forward_for_distinct_values() {
        use crate::value::StrykeValue;
        let pairs = [
            (StrykeValue::integer(3), StrykeValue::integer(8)),
            (StrykeValue::integer(-1), StrykeValue::integer(0)),
            (StrykeValue::float(1.0), StrykeValue::float(2.0)),
        ];
        for (a, b) in &pairs {
            let fwd = sort_magic_cmp(a, b, SortBlockFast::Numeric);
            let rev = sort_magic_cmp(a, b, SortBlockFast::NumericRev);
            assert_eq!(fwd.reverse(), rev);
        }
        let sa = StrykeValue::string("apple".into());
        let sb = StrykeValue::string("banana".into());
        assert_eq!(
            sort_magic_cmp(&sa, &sb, SortBlockFast::String).reverse(),
            sort_magic_cmp(&sa, &sb, SortBlockFast::StringRev)
        );
    }

    // ── detect_sort_block_fast rejection cases ───────────────────────

    #[test]
    fn detect_rejects_non_expression_first_statement() {
        // `return $a <=> $b` is StmtKind::Return, not StmtKind::Expression.
        let p = crate::parse("fn r { return $a <=> $b; }").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::SubDecl { body, .. } => body,
            _ => panic!("expected sub"),
        };
        assert_eq!(detect_sort_block_fast(block), None);
    }

    #[test]
    fn detect_rejects_wrong_operand_pair() {
        // `$a <=> $a` is not the magic-pair shape.
        let p = crate::parse("fn r { $a <=> $a }").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::SubDecl { body, .. } => body,
            _ => panic!("expected sub"),
        };
        assert_eq!(detect_sort_block_fast(block), None);
    }

    #[test]
    fn detect_rejects_unrelated_binop() {
        // `$a + $b` is BinOp::Add — neither Spaceship nor StrCmp.
        let p = crate::parse("fn r { $a + $b }").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::SubDecl { body, .. } => body,
            _ => panic!("expected sub"),
        };
        assert_eq!(detect_sort_block_fast(block), None);
    }

    #[test]
    fn detect_rejects_named_var_not_magic_a_or_b() {
        // `$x <=> $y` reads as ScalarVar("x") / ScalarVar("y"), not the magic globals.
        let p = crate::parse("fn r { $x <=> $y }").expect("parse");
        let block = match &p.statements[0].kind {
            StmtKind::SubDecl { body, .. } => body,
            _ => panic!("expected sub"),
        };
        assert_eq!(detect_sort_block_fast(block), None);
    }
}
