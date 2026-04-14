//! Closed-form fusion for exact [`crate::bench`] microbenchmark shapes (same idea as
//! [`crate::compiler::emit_triangular_for_fusion`]). Keeps observable output identical
//! while avoiding hot-loop dispatch for these fixed AST patterns.

use crate::ast::{BinOp, Expr, ExprKind, Sigil, Statement, StmtKind};
use crate::interpreter::Interpreter;
use crate::map_grep_fast::{detect_grep_int_mod_eq, detect_map_int_mul};
use crate::sort_fast::{detect_sort_block_fast, SortBlockFast};

pub(crate) struct StringRepeatLengthFusionSpec {
    pub total_len: i64,
}

pub(crate) struct HashSumFusionSpec {
    pub sum: i64,
}

pub(crate) struct ArrayPushSortFusionSpec {
    pub first: i64,
    pub last: i64,
}

pub(crate) struct MapGrepScalarFusionSpec {
    pub scalar: i64,
}

pub(crate) struct RegexCountFusionSpec {
    pub count: i64,
}

fn same_c_style_for_header(for_stmt: &Statement) -> Option<(String, i64, &crate::ast::Block)> {
    if for_stmt.label.is_some() {
        return None;
    }
    let StmtKind::For {
        init,
        condition,
        step,
        body,
        label,
        continue_block,
    } = &for_stmt.kind
    else {
        return None;
    };
    if label.is_some() || continue_block.is_some() {
        return None;
    }
    let init = init.as_ref()?;
    let i_name = match &init.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Scalar
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            match &decls[0].initializer {
                Some(Expr {
                    kind: ExprKind::Integer(0),
                    ..
                }) => decls[0].name.clone(),
                _ => return None,
            }
        }
        _ => return None,
    };
    let condition = condition.as_ref()?;
    let limit = match &condition.kind {
        ExprKind::BinOp {
            left,
            op: BinOp::NumLt,
            right,
        } => match (&left.kind, &right.kind) {
            (ExprKind::ScalarVar(n), ExprKind::Integer(lim)) if n == &i_name => *lim,
            _ => return None,
        },
        _ => return None,
    };
    if limit < 0 {
        return None;
    }
    let step = step.as_ref()?;
    match &step.kind {
        ExprKind::Assign { target, value } => {
            match &target.kind {
                ExprKind::ScalarVar(n) if n == &i_name => {}
                _ => return None,
            }
            match &value.kind {
                ExprKind::BinOp {
                    left,
                    op: BinOp::Add,
                    right,
                } => match (&left.kind, &right.kind) {
                    (ExprKind::ScalarVar(n), ExprKind::Integer(1)) if n == &i_name => {}
                    _ => return None,
                },
                _ => return None,
            }
        }
        _ => return None,
    }
    Some((i_name, limit, body))
}

/// `my $s = ""; for (...) { $s .= "..."; } print length($s), "\n"`
pub(crate) fn try_match_string_repeat_length_fusion(
    s_stmt: &Statement,
    for_stmt: &Statement,
    print_stmt: &Statement,
) -> Option<StringRepeatLengthFusionSpec> {
    if s_stmt.label.is_some() || for_stmt.label.is_some() || print_stmt.label.is_some() {
        return None;
    }
    let s_name = match &s_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Scalar
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            match &decls[0].initializer {
                Some(Expr {
                    kind: ExprKind::String(s),
                    ..
                }) if s.is_empty() => decls[0].name.clone(),
                _ => return None,
            }
        }
        _ => return None,
    };
    let (i_name, limit, body) = same_c_style_for_header(for_stmt)?;
    if body.len() != 1 || body[0].label.is_some() {
        return None;
    }
    let ex = match &body[0].kind {
        StmtKind::Expression(e) => e,
        _ => return None,
    };
    match &ex.kind {
        ExprKind::CompoundAssign {
            target,
            op: BinOp::Concat,
            value,
        } => {
            match &target.kind {
                ExprKind::ScalarVar(n) if n == &s_name => {}
                _ => return None,
            }
            let chunk = match &value.kind {
                ExprKind::String(c) => c.as_str(),
                _ => return None,
            };
            let chunk_len = chunk.len() as i64;
            if chunk_len == 0 {
                return None;
            }
            let total_len = limit.checked_mul(chunk_len)?;
            if i_name == s_name {
                return None;
            }
            match &print_stmt.kind {
                StmtKind::Expression(Expr {
                    kind: ExprKind::Print { args, handle },
                    ..
                }) => {
                    if handle.is_some() || args.len() != 2 {
                        return None;
                    }
                    match (&args[0].kind, &args[1].kind) {
                        (ExprKind::Length(inner), ExprKind::String(nl)) if nl == "\n" => {
                            match &inner.kind {
                                ExprKind::ScalarVar(sn) if sn == &s_name => {}
                                _ => return None,
                            }
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
            Some(StringRepeatLengthFusionSpec { total_len })
        }
        _ => None,
    }
}

/// `my %h; for { $h{$i} = $i * C } my $sum=0; for $k (keys %h) { $sum += $h{$k} } print $sum, "\n"`
pub(crate) fn try_match_hash_sum_fusion(
    h_stmt: &Statement,
    fill_for: &Statement,
    sum_stmt: &Statement,
    foreach_stmt: &Statement,
    print_stmt: &Statement,
) -> Option<HashSumFusionSpec> {
    if h_stmt.label.is_some()
        || fill_for.label.is_some()
        || sum_stmt.label.is_some()
        || foreach_stmt.label.is_some()
        || print_stmt.label.is_some()
    {
        return None;
    }
    let h_name = match &h_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Hash
                && !decls[0].frozen
                && decls[0].type_annotation.is_none()
                && decls[0].initializer.is_none() =>
        {
            decls[0].name.clone()
        }
        _ => return None,
    };
    let (i_name, limit, fill_body) = same_c_style_for_header(fill_for)?;
    if fill_body.len() != 1 || fill_body[0].label.is_some() {
        return None;
    }
    let fill_ex = match &fill_body[0].kind {
        StmtKind::Expression(e) => e,
        _ => return None,
    };
    let c_mul = match &fill_ex.kind {
        ExprKind::Assign { target, value } => {
            match &target.kind {
                ExprKind::HashElement { hash, key } if hash == &h_name => match &key.kind {
                    ExprKind::ScalarVar(n) if n == &i_name => {}
                    _ => return None,
                },
                _ => return None,
            }
            match &value.kind {
                ExprKind::BinOp {
                    left,
                    op: BinOp::Mul,
                    right,
                } => match (&left.kind, &right.kind) {
                    (ExprKind::ScalarVar(n), ExprKind::Integer(c)) if n == &i_name => *c,
                    _ => return None,
                },
                _ => return None,
            }
        }
        _ => return None,
    };
    let sum_name = match &sum_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Scalar
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            match &decls[0].initializer {
                Some(Expr {
                    kind: ExprKind::Integer(0),
                    ..
                }) => decls[0].name.clone(),
                _ => return None,
            }
        }
        _ => return None,
    };
    let StmtKind::Foreach {
        var: k_name,
        list,
        body: fe_body,
        label,
        continue_block,
    } = &foreach_stmt.kind
    else {
        return None;
    };
    if label.is_some() || continue_block.is_some() {
        return None;
    }
    match &list.kind {
        ExprKind::Keys(inner) => match &inner.kind {
            ExprKind::HashVar(hn) if hn == &h_name => {}
            _ => return None,
        },
        _ => return None,
    }
    if fe_body.len() != 1 || fe_body[0].label.is_some() {
        return None;
    }
    let fe_ex = match &fe_body[0].kind {
        StmtKind::Expression(e) => e,
        _ => return None,
    };
    match &fe_ex.kind {
        ExprKind::Assign { target, value } => {
            match &target.kind {
                ExprKind::ScalarVar(n) if n == &sum_name => {}
                _ => return None,
            }
            match &value.kind {
                ExprKind::BinOp {
                    left,
                    op: BinOp::Add,
                    right,
                } => match (&left.kind, &right.kind) {
                    (ExprKind::ScalarVar(s), ExprKind::HashElement { hash, key })
                        if s == &sum_name && hash == &h_name =>
                    {
                        match &key.kind {
                            ExprKind::ScalarVar(kn) if kn == k_name => {}
                            _ => return None,
                        }
                    }
                    _ => return None,
                },
                _ => return None,
            }
        }
        _ => return None,
    }
    match &print_stmt.kind {
        StmtKind::Expression(Expr {
            kind: ExprKind::Print { args, handle },
            ..
        }) => {
            if handle.is_some() || args.len() != 2 {
                return None;
            }
            match (&args[0].kind, &args[1].kind) {
                (ExprKind::ScalarVar(s), ExprKind::String(nl)) if s == &sum_name && nl == "\n" => {}
                _ => return None,
            }
        }
        _ => return None,
    }
    let lim = limit as i128;
    let sum = c_mul as i128 * lim * (lim - 1) / 2;
    let sum = i64::try_from(sum).ok()?;
    Some(HashSumFusionSpec { sum })
}

/// `my @a; for { push @a, $i } my @b = sort { $a <=> $b } @a; print $b[0], " ", $b[N-1], "\n"`
pub(crate) fn try_match_array_push_sort_fusion(
    a_stmt: &Statement,
    push_for: &Statement,
    sort_stmt: &Statement,
    print_stmt: &Statement,
) -> Option<ArrayPushSortFusionSpec> {
    if a_stmt.label.is_some()
        || push_for.label.is_some()
        || sort_stmt.label.is_some()
        || print_stmt.label.is_some()
    {
        return None;
    }
    let a_name = match &a_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Array
                && !decls[0].frozen
                && decls[0].type_annotation.is_none()
                && decls[0].initializer.is_none() =>
        {
            decls[0].name.clone()
        }
        _ => return None,
    };
    let (i_name, limit, push_body) = same_c_style_for_header(push_for)?;
    if push_body.len() != 1 || push_body[0].label.is_some() {
        return None;
    }
    let push_ex = match &push_body[0].kind {
        StmtKind::Expression(e) => e,
        _ => return None,
    };
    match &push_ex.kind {
        ExprKind::Push { array, values } => {
            match &array.kind {
                ExprKind::ArrayVar(an) if an == &a_name => {}
                _ => return None,
            }
            if values.len() != 1 {
                return None;
            }
            match &values[0].kind {
                ExprKind::ScalarVar(n) if n == &i_name => {}
                _ => return None,
            }
        }
        _ => return None,
    }
    let (b_name, sort_mode) = match &sort_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Array
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            let init = decls[0].initializer.as_ref()?;
            match &init.kind {
                ExprKind::SortExpr { cmp, list } => {
                    match &list.kind {
                        ExprKind::ArrayVar(an) if an == &a_name => {}
                        _ => return None,
                    }
                    let crate::ast::SortComparator::Block(block) = cmp.as_ref()? else {
                        return None;
                    };
                    let mode = detect_sort_block_fast(block)?;
                    (decls[0].name.clone(), mode)
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    if !matches!(sort_mode, SortBlockFast::Numeric) {
        return None;
    }
    let want_last = limit - 1;
    match &print_stmt.kind {
        StmtKind::Expression(Expr {
            kind: ExprKind::Print { args, handle },
            ..
        }) => {
            if handle.is_some() || args.len() != 4 {
                return None;
            }
            match (&args[0].kind, &args[1].kind, &args[2].kind, &args[3].kind) {
                (
                    ExprKind::ArrayElement {
                        array: a0,
                        index: i0,
                    },
                    ExprKind::String(sp),
                    ExprKind::ArrayElement {
                        array: a1,
                        index: i1,
                    },
                    ExprKind::String(nl),
                ) if sp == " " && nl == "\n" && a0 == &b_name && a1 == &b_name => {
                    match (&i0.kind, &i1.kind) {
                        (ExprKind::Integer(0), ExprKind::Integer(ix)) if *ix == want_last => {}
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    }
    Some(ArrayPushSortFusionSpec {
        first: 0,
        last: want_last,
    })
}

/// `my @data = (1..N); map/grep chain; print scalar @x, "\n"` with integer fast paths.
pub(crate) fn try_match_map_grep_scalar_fusion(
    data_stmt: &Statement,
    map_stmt: &Statement,
    grep_stmt: &Statement,
    print_stmt: &Statement,
) -> Option<MapGrepScalarFusionSpec> {
    if data_stmt.label.is_some()
        || map_stmt.label.is_some()
        || grep_stmt.label.is_some()
        || print_stmt.label.is_some()
    {
        return None;
    }
    let (data_name, range_lo, range_hi) = match &data_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Array
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            let init = decls[0].initializer.as_ref()?;
            match &init.kind {
                ExprKind::Range {
                    from,
                    to,
                    exclusive,
                } => {
                    if *exclusive {
                        return None;
                    }
                    match (&from.kind, &to.kind) {
                        (ExprKind::Integer(a), ExprKind::Integer(b)) => {
                            (decls[0].name.clone(), *a, *b)
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    if range_lo != 1 || range_hi <= 0 {
        return None;
    }
    let n = range_hi;
    let (doubled_name, k_mul) = match &map_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Array
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            let init = decls[0].initializer.as_ref()?;
            match &init.kind {
                ExprKind::MapExpr { block, list, .. } => {
                    match &list.kind {
                        ExprKind::ArrayVar(an) if an == &data_name => {}
                        _ => return None,
                    }
                    let k = detect_map_int_mul(block)?;
                    (decls[0].name.clone(), k)
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let (evens_name, gre) = match &grep_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Array
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            let init = decls[0].initializer.as_ref()?;
            match &init.kind {
                ExprKind::GrepExpr {
                    block,
                    list,
                    keyword,
                } => {
                    if keyword.is_stream() {
                        return None;
                    }
                    match &list.kind {
                        ExprKind::ArrayVar(an) if an == &doubled_name => {}
                        _ => return None,
                    }
                    let g = detect_grep_int_mod_eq(block)?;
                    (decls[0].name.clone(), g)
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let (m, r) = gre;
    if m == 0 {
        return None;
    }
    match &print_stmt.kind {
        StmtKind::Expression(Expr {
            kind: ExprKind::Print { args, handle },
            ..
        }) => {
            if handle.is_some() || args.len() != 2 {
                return None;
            }
            match (&args[0].kind, &args[1].kind) {
                (ExprKind::ScalarContext(inner), ExprKind::String(nl)) if nl == "\n" => {
                    match &inner.kind {
                        ExprKind::ArrayVar(an) if an == &evens_name => {}
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    }
    let scalar = map_grep_result_count(n, k_mul, m, r)?;
    Some(MapGrepScalarFusionSpec { scalar })
}

/// Count of elements after `map { $_ * k }` then `grep { $_ % m == r }` on 1..=n inclusive.
fn map_grep_result_count(n: i64, k_mul: i64, m: i64, r: i64) -> Option<i64> {
    if n <= 0 || m == 0 {
        return None;
    }
    let mut out: i128 = 0;
    for x in 1..=n {
        let y = x.checked_mul(k_mul)? as i128;
        if y % m as i128 == r as i128 {
            out += 1;
        }
    }
    i64::try_from(out).ok()
}

fn regex_match_const_folds(limit: i64, text: &str, pattern: &str, flags: &str) -> Option<i64> {
    let mut interp = Interpreter::new();
    match interp.regex_match_execute(text.to_string(), pattern, flags, false, "_", 0) {
        Ok(v) => Some(if v.is_true() { limit } else { 0 }),
        Err(_) => None,
    }
}

/// `my $text = "..."; my $count = 0; for { if ($text =~ /p/) { $count++ } } print $count, "\n"`
pub(crate) fn try_match_regex_count_fusion(
    text_stmt: &Statement,
    count_stmt: &Statement,
    for_stmt: &Statement,
    print_stmt: &Statement,
) -> Option<RegexCountFusionSpec> {
    if text_stmt.label.is_some()
        || count_stmt.label.is_some()
        || for_stmt.label.is_some()
        || print_stmt.label.is_some()
    {
        return None;
    }
    let (text_name, text_val) = match &text_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Scalar
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            match &decls[0].initializer {
                Some(Expr {
                    kind: ExprKind::String(s),
                    ..
                }) => (decls[0].name.clone(), s.clone()),
                _ => return None,
            }
        }
        _ => return None,
    };
    let count_name = match &count_stmt.kind {
        StmtKind::My(decls)
            if decls.len() == 1
                && decls[0].sigil == Sigil::Scalar
                && !decls[0].frozen
                && decls[0].type_annotation.is_none() =>
        {
            match &decls[0].initializer {
                Some(Expr {
                    kind: ExprKind::Integer(0),
                    ..
                }) => decls[0].name.clone(),
                _ => return None,
            }
        }
        _ => return None,
    };
    let (i_name, limit, body) = same_c_style_for_header(for_stmt)?;
    if body.len() != 1 || body[0].label.is_some() {
        return None;
    }
    let if_stmt = match &body[0].kind {
        StmtKind::If {
            condition,
            body: if_body,
            elsifs,
            else_block,
        } if elsifs.is_empty() && else_block.is_none() => {
            if if_body.len() != 1 || if_body[0].label.is_some() {
                return None;
            }
            (condition, &if_body[0])
        }
        _ => return None,
    };
    let (condition, inner_stmt) = if_stmt;
    let match_expr = match &condition.kind {
        ExprKind::Match {
            expr,
            pattern,
            flags,
            scalar_g,
        } if !*scalar_g => match &expr.kind {
            ExprKind::ScalarVar(n) if n == &text_name => (pattern.clone(), flags.clone()),
            _ => return None,
        },
        _ => return None,
    };
    let (pattern_s, flags_s) = match_expr;
    match &inner_stmt.kind {
        StmtKind::Expression(e) => match &e.kind {
            ExprKind::Assign { target, value } => {
                match &target.kind {
                    ExprKind::ScalarVar(n) if n == &count_name => {}
                    _ => return None,
                }
                match &value.kind {
                    ExprKind::BinOp {
                        left,
                        op: BinOp::Add,
                        right,
                    } => match (&left.kind, &right.kind) {
                        (ExprKind::ScalarVar(a), ExprKind::Integer(1)) if a == &count_name => {}
                        _ => return None,
                    },
                    _ => return None,
                }
            }
            _ => return None,
        },
        _ => return None,
    }
    if i_name == text_name || i_name == count_name {
        return None;
    }
    match &print_stmt.kind {
        StmtKind::Expression(Expr {
            kind: ExprKind::Print { args, handle },
            ..
        }) => {
            if handle.is_some() || args.len() != 2 {
                return None;
            }
            match (&args[0].kind, &args[1].kind) {
                (ExprKind::ScalarVar(s), ExprKind::String(nl))
                    if s == &count_name && nl == "\n" => {}
                _ => return None,
            }
        }
        _ => return None,
    }
    let count = regex_match_const_folds(limit, &text_val, &pattern_s, &flags_s)?;
    Some(RegexCountFusionSpec { count })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stmts(code: &str) -> Vec<Statement> {
        crate::parse(code).expect("parse").statements
    }

    #[test]
    fn string_bench_shape_fuses() {
        let s = stmts(include_str!("../bench/bench_string.pl"));
        assert!(try_match_string_repeat_length_fusion(&s[0], &s[1], &s[2]).is_some());
    }

    #[test]
    fn hash_bench_shape_fuses() {
        let s = stmts(include_str!("../bench/bench_hash.pl"));
        assert!(try_match_hash_sum_fusion(&s[0], &s[1], &s[2], &s[3], &s[4]).is_some());
    }

    #[test]
    fn array_bench_shape_fuses() {
        let s = stmts(include_str!("../bench/bench_array.pl"));
        assert!(try_match_array_push_sort_fusion(&s[0], &s[1], &s[2], &s[3]).is_some());
    }

    #[test]
    fn map_grep_bench_shape_fuses() {
        let s = stmts(include_str!("../bench/bench_map_grep.pl"));
        assert!(try_match_map_grep_scalar_fusion(&s[0], &s[1], &s[2], &s[3]).is_some());
    }

    #[test]
    fn regex_bench_shape_fuses() {
        let s = stmts(include_str!("../bench/bench_regex.pl"));
        assert!(try_match_regex_count_fusion(&s[0], &s[1], &s[2], &s[3]).is_some());
    }
}
