//! Detect `return f(...) + f(...)` with two calls to the same sub and a pure integer base case,
//! then evaluate with an explicit stack (no per-call scope push/pop).
//!
//! Supported shape (typical Fibonacci):
//! - `my $p = shift;` / `shift @_`
//! - `return $p if $p <= K` (or `unless ($p > K)` with the same meaning)
//! - `return f($p - a) + f($p - b);` with integer `a`, `b` and matching [`FuncCall`] names.

use crate::ast::{BinOp, Block, Expr, ExprKind, Sigil, StmtKind};
use crate::value::{FibLikeRecAddPattern, PerlSub};

fn is_shift_argv(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::Shift(b) if matches!(&b.kind, ExprKind::ArrayVar(a) if a == "_")
    )
}

fn extract_shift_param(body: &Block) -> Option<String> {
    for stmt in body {
        if let StmtKind::My(dcls) = &stmt.kind {
            for d in dcls {
                if d.sigil != Sigil::Scalar {
                    continue;
                }
                let Some(init) = &d.initializer else {
                    continue;
                };
                if is_shift_argv(init) {
                    return Some(d.name.clone());
                }
            }
        }
    }
    None
}

fn body_stmt_kinds_ok_for_fib_like(body: &Block) -> bool {
    for stmt in body {
        match &stmt.kind {
            StmtKind::My(_) => {}
            StmtKind::If { .. } | StmtKind::Unless { .. } => {}
            StmtKind::Return(_) => {}
            StmtKind::Empty => {}
            _ => return false,
        }
    }
    true
}

/// `return $p if $p <= K` → `If { cond: $p <= K, body: [Return $p] }`
fn parse_base_le_return(stmt: &crate::ast::Statement, param: &str) -> Option<i64> {
    let crate::ast::Statement { kind, .. } = stmt;
    match kind {
        StmtKind::If {
            condition,
            body,
            elsifs,
            else_block,
        } => {
            if !elsifs.is_empty() || else_block.is_some() || body.len() != 1 {
                return None;
            }
            let ExprKind::BinOp {
                left,
                op: BinOp::NumLe,
                right,
            } = &condition.kind
            else {
                return None;
            };
            let ExprKind::ScalarVar(pv) = &left.kind else {
                return None;
            };
            if pv != param {
                return None;
            }
            let ExprKind::Integer(k) = &right.kind else {
                return None;
            };
            let StmtKind::Return(Some(ret_e)) = &body[0].kind else {
                return None;
            };
            let ExprKind::ScalarVar(rv) = &ret_e.kind else {
                return None;
            };
            if rv != param {
                return None;
            }
            Some(*k)
        }
        StmtKind::Unless {
            condition,
            body,
            else_block,
        } => {
            if else_block.is_some() || body.len() != 1 {
                return None;
            }
            // `unless ($p > K) { return $p }`  ⇔  `if ($p <= K) { return $p }`
            let ExprKind::BinOp {
                left,
                op: BinOp::NumGt,
                right,
            } = &condition.kind
            else {
                return None;
            };
            let ExprKind::ScalarVar(pv) = &left.kind else {
                return None;
            };
            if pv != param {
                return None;
            }
            let ExprKind::Integer(k) = &right.kind else {
                return None;
            };
            let StmtKind::Return(Some(ret_e)) = &body[0].kind else {
                return None;
            };
            let ExprKind::ScalarVar(rv) = &ret_e.kind else {
                return None;
            };
            if rv != param {
                return None;
            }
            Some(*k)
        }
        _ => None,
    }
}

fn arg_as_param_minus(expr: &Expr, param: &str) -> Option<i64> {
    match &expr.kind {
        ExprKind::ScalarVar(s) if s == param => Some(0),
        ExprKind::BinOp {
            left,
            op: BinOp::Sub,
            right,
        } if matches!(&left.kind, ExprKind::ScalarVar(s) if s == param) => {
            let ExprKind::Integer(k) = &right.kind else {
                return None;
            };
            Some(*k)
        }
        _ => None,
    }
}

fn find_recursive_add_return<'a>(
    body: &'a Block,
    sub: &PerlSub,
    param: &str,
) -> Option<(&'a Expr, i64, i64)> {
    for stmt in body {
        let StmtKind::Return(Some(e)) = &stmt.kind else {
            continue;
        };
        let ExprKind::BinOp {
            left,
            op: BinOp::Add,
            right,
        } = &e.kind
        else {
            continue;
        };
        let ExprKind::FuncCall { name: nl, args: al } = &left.kind else {
            continue;
        };
        let ExprKind::FuncCall { name: nr, args: ar } = &right.kind else {
            continue;
        };
        if nl != nr || nl != sub.name.as_str() {
            continue;
        }
        if al.len() != 1 || ar.len() != 1 {
            continue;
        }
        let left_k = arg_as_param_minus(&al[0], param)?;
        let right_k = arg_as_param_minus(&ar[0], param)?;
        return Some((e, left_k, right_k));
    }
    None
}

fn find_base_k(body: &Block, param: &str) -> Option<i64> {
    for stmt in body {
        if let Some(k) = parse_base_le_return(stmt, param) {
            return Some(k);
        }
    }
    None
}

/// When the subroutine body matches a fib-like recursive add, return the pattern.
pub(crate) fn detect_fib_like_recursive_add(sub: &PerlSub) -> Option<FibLikeRecAddPattern> {
    if sub.closure_env.is_some() || !sub.params.is_empty() {
        return None;
    }
    let body = &sub.body;
    if !body_stmt_kinds_ok_for_fib_like(body) {
        return None;
    }
    let param = extract_shift_param(body)?;
    let base_k = find_base_k(body, &param)?;
    let (_, left_k, right_k) = find_recursive_add_return(body, sub, &param)?;
    Some(FibLikeRecAddPattern {
        param,
        base_k,
        left_k,
        right_k,
    })
}

enum Frame {
    Eval(i64),
    Add,
}

/// Iterative post-order evaluation of `f(n) = f(n-a)+f(n-b)` with base `n <= base_k ⇒ n`.
pub(crate) fn eval_fib_like_recursive_add(n0: i64, pat: &FibLikeRecAddPattern) -> i64 {
    let mut stack = vec![Frame::Eval(n0)];
    let mut vals: Vec<i64> = Vec::new();
    while let Some(fr) = stack.pop() {
        match fr {
            Frame::Eval(n) => {
                if n <= pat.base_k {
                    vals.push(n);
                } else {
                    let ln = n.saturating_sub(pat.left_k);
                    let rn = n.saturating_sub(pat.right_k);
                    stack.push(Frame::Add);
                    stack.push(Frame::Eval(rn));
                    stack.push(Frame::Eval(ln));
                }
            }
            Frame::Add => {
                let r = vals.pop().expect("fib-like add rhs");
                let l = vals.pop().expect("fib-like add lhs");
                vals.push(l.saturating_add(r));
            }
        }
    }
    vals.pop().expect("fib-like result")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::StmtKind;
    use crate::parse;

    #[test]
    fn detect_and_eval_fib_style() {
        let code =
            "fn fib_n { my $n = shift @_; return $n if $n <= 1; return fib_n($n-1) + fib_n($n-2); }";
        let program = parse(code).expect("parse");
        let sub_stmt = program.statements.iter().find_map(|s| {
            if let StmtKind::SubDecl { name, body, .. } = &s.kind {
                if name == "fib_n" {
                    return Some(body.clone());
                }
            }
            None
        });
        let body = sub_stmt.expect("fn fib_n");
        let ps = PerlSub {
            name: "fib_n".into(),
            params: vec![],
            body,
            closure_env: None,
            prototype: None,
            fib_like: None,
        };
        let pat = detect_fib_like_recursive_add(&ps).expect("detect");
        assert_eq!(pat.base_k, 1);
        assert_eq!(pat.left_k, 1);
        assert_eq!(pat.right_k, 2);
        assert_eq!(eval_fib_like_recursive_add(10, &pat), 55);
    }
}
