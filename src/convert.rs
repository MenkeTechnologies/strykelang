//! Convert standard Perl source to idiomatic perlrs syntax.
//!
//! Transformations applied:
//! - Nested function/builtin calls → `|>` pipe-forward chains
//! - `map { BLOCK } LIST` → `LIST |> map { BLOCK }`
//! - `grep { BLOCK } LIST` → `LIST |> grep { BLOCK }`
//! - `sort [{ CMP }] LIST` → `LIST |> sort [{ CMP }]`
//! - `join(SEP, LIST)` → `LIST |> join SEP`
//! - No trailing semicolons (newline terminates statements)
//! - 4-space indentation for block bodies
//! - `#!/usr/bin/env perlrs` shebang prepended
//! - Pipe RHS uses bare args: `|> binmode ":utf8"` not `|> binmode(":utf8")`

#![allow(unused_variables)]

use crate::ast::*;
use crate::fmt;

const INDENT: &str = "    ";

// ── Public API ──────────────────────────────────────────────────────────────

/// Convert a parsed Perl program to perlrs syntax.
pub fn convert_program(p: &Program) -> String {
    let body = p
        .statements
        .iter()
        .map(|s| convert_statement(s, 0))
        .collect::<Vec<_>>()
        .join("\n");
    format!("#!/usr/bin/env perlrs\n{}", body)
}

// ── Block / Statement ───────────────────────────────────────────────────────

fn convert_block(b: &Block, depth: usize) -> String {
    b.iter()
        .map(|s| convert_statement(s, depth))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Indent a string by `depth` levels of 4 spaces.
fn indent(depth: usize) -> String {
    INDENT.repeat(depth)
}

fn convert_statement(s: &Statement, depth: usize) -> String {
    let lab = s
        .label
        .as_ref()
        .map(|l| format!("{}: ", l))
        .unwrap_or_default();
    let pfx = indent(depth);
    let body = match &s.kind {
        StmtKind::Expression(e) => convert_expr_top(e),
        StmtKind::If {
            condition,
            body,
            elsifs,
            else_block,
        } => {
            let mut s = format!(
                "if ({}) {{\n{}\n{}}}",
                convert_expr(condition),
                convert_block(body, depth + 1),
                pfx
            );
            for (c, b) in elsifs {
                s.push_str(&format!(
                    " elsif ({}) {{\n{}\n{}}}",
                    convert_expr(c),
                    convert_block(b, depth + 1),
                    pfx
                ));
            }
            if let Some(eb) = else_block {
                s.push_str(&format!(
                    " else {{\n{}\n{}}}",
                    convert_block(eb, depth + 1),
                    pfx
                ));
            }
            s
        }
        StmtKind::Unless {
            condition,
            body,
            else_block,
        } => {
            let mut s = format!(
                "unless ({}) {{\n{}\n{}}}",
                convert_expr(condition),
                convert_block(body, depth + 1),
                pfx
            );
            if let Some(eb) = else_block {
                s.push_str(&format!(
                    " else {{\n{}\n{}}}",
                    convert_block(eb, depth + 1),
                    pfx
                ));
            }
            s
        }
        StmtKind::While {
            condition,
            body,
            label,
            continue_block,
        } => {
            let lb = label
                .as_ref()
                .map(|l| format!("{}: ", l))
                .unwrap_or_default();
            let mut s = format!(
                "{}while ({}) {{\n{}\n{}}}",
                lb,
                convert_expr(condition),
                convert_block(body, depth + 1),
                pfx
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(
                    " continue {{\n{}\n{}}}",
                    convert_block(cb, depth + 1),
                    pfx
                ));
            }
            s
        }
        StmtKind::Until {
            condition,
            body,
            label,
            continue_block,
        } => {
            let lb = label
                .as_ref()
                .map(|l| format!("{}: ", l))
                .unwrap_or_default();
            let mut s = format!(
                "{}until ({}) {{\n{}\n{}}}",
                lb,
                convert_expr(condition),
                convert_block(body, depth + 1),
                pfx
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(
                    " continue {{\n{}\n{}}}",
                    convert_block(cb, depth + 1),
                    pfx
                ));
            }
            s
        }
        StmtKind::DoWhile { body, condition } => {
            format!(
                "do {{\n{}\n{}}} while ({})",
                convert_block(body, depth + 1),
                pfx,
                convert_expr(condition)
            )
        }
        StmtKind::For {
            init,
            condition,
            step,
            body,
            label,
            continue_block,
        } => {
            let lb = label
                .as_ref()
                .map(|l| format!("{}: ", l))
                .unwrap_or_default();
            let ini = init
                .as_ref()
                .map(|s| convert_statement_body(s))
                .unwrap_or_default();
            let cond = condition.as_ref().map(convert_expr).unwrap_or_default();
            let st = step.as_ref().map(convert_expr).unwrap_or_default();
            let mut s = format!(
                "{}for ({}; {}; {}) {{\n{}\n{}}}",
                lb,
                ini,
                cond,
                st,
                convert_block(body, depth + 1),
                pfx
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(
                    " continue {{\n{}\n{}}}",
                    convert_block(cb, depth + 1),
                    pfx
                ));
            }
            s
        }
        StmtKind::Foreach {
            var,
            list,
            body,
            label,
            continue_block,
        } => {
            let lb = label
                .as_ref()
                .map(|l| format!("{}: ", l))
                .unwrap_or_default();
            let mut s = format!(
                "{}for ${} ({}) {{\n{}\n{}}}",
                lb,
                var,
                convert_expr(list),
                convert_block(body, depth + 1),
                pfx
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(
                    " continue {{\n{}\n{}}}",
                    convert_block(cb, depth + 1),
                    pfx
                ));
            }
            s
        }
        StmtKind::SubDecl {
            name,
            params,
            body,
            prototype,
        } => {
            let sig = if !params.is_empty() {
                format!(
                    " ({})",
                    params
                        .iter()
                        .map(fmt::format_sub_sig_param)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                prototype
                    .as_ref()
                    .map(|p| format!(" ({})", p))
                    .unwrap_or_default()
            };
            format!(
                "sub {}{} {{\n{}\n{}}}",
                name,
                sig,
                convert_block(body, depth + 1),
                pfx
            )
        }
        StmtKind::Package { name } => format!("package {}", name),
        StmtKind::UsePerlVersion { version } => {
            if version.fract() == 0.0 && *version >= 0.0 {
                format!("use {}", *version as i64)
            } else {
                format!("use {}", version)
            }
        }
        StmtKind::Use { module, imports } => {
            if imports.is_empty() {
                format!("use {}", module)
            } else {
                format!("use {} {}", module, convert_expr_list(imports))
            }
        }
        StmtKind::UseOverload { pairs } => {
            let inner = pairs
                .iter()
                .map(|(k, v)| {
                    format!(
                        "'{}' => '{}'",
                        k.replace('\'', "\\'"),
                        v.replace('\'', "\\'")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("use overload {inner}")
        }
        StmtKind::No { module, imports } => {
            if imports.is_empty() {
                format!("no {}", module)
            } else {
                format!("no {} {}", module, convert_expr_list(imports))
            }
        }
        StmtKind::Return(e) => e
            .as_ref()
            .map(|x| format!("return {}", convert_expr_top(x)))
            .unwrap_or_else(|| "return".to_string()),
        StmtKind::Last(l) => l
            .as_ref()
            .map(|x| format!("last {}", x))
            .unwrap_or_else(|| "last".to_string()),
        StmtKind::Next(l) => l
            .as_ref()
            .map(|x| format!("next {}", x))
            .unwrap_or_else(|| "next".to_string()),
        StmtKind::Redo(l) => l
            .as_ref()
            .map(|x| format!("redo {}", x))
            .unwrap_or_else(|| "redo".to_string()),
        StmtKind::My(decls) => format!("my {}", convert_var_decls(decls)),
        StmtKind::Our(decls) => format!("our {}", convert_var_decls(decls)),
        StmtKind::Local(decls) => format!("local {}", convert_var_decls(decls)),
        StmtKind::State(decls) => format!("state {}", convert_var_decls(decls)),
        StmtKind::LocalExpr {
            target,
            initializer,
        } => {
            let mut s = format!("local {}", convert_expr(target));
            if let Some(init) = initializer {
                s.push_str(&format!(" = {}", convert_expr_top(init)));
            }
            s
        }
        StmtKind::MySync(decls) => format!("mysync {}", convert_var_decls(decls)),
        StmtKind::StmtGroup(b) => convert_block(b, depth),
        StmtKind::Block(b) => format!("{{\n{}\n{}}}", convert_block(b, depth + 1), pfx),
        StmtKind::Begin(b) => format!("BEGIN {{\n{}\n{}}}", convert_block(b, depth + 1), pfx),
        StmtKind::UnitCheck(b) => {
            format!("UNITCHECK {{\n{}\n{}}}", convert_block(b, depth + 1), pfx)
        }
        StmtKind::Check(b) => format!("CHECK {{\n{}\n{}}}", convert_block(b, depth + 1), pfx),
        StmtKind::Init(b) => format!("INIT {{\n{}\n{}}}", convert_block(b, depth + 1), pfx),
        StmtKind::End(b) => format!("END {{\n{}\n{}}}", convert_block(b, depth + 1), pfx),
        StmtKind::Empty => String::new(),
        StmtKind::Goto { target } => format!("goto {}", convert_expr(target)),
        StmtKind::Continue(b) => format!("continue {{\n{}\n{}}}", convert_block(b, depth + 1), pfx),
        StmtKind::StructDecl { def } => {
            let fields = def
                .fields
                .iter()
                .map(|(n, t)| format!("{} => {:?}", n, t))
                .collect::<Vec<_>>()
                .join(", ");
            format!("struct {} {{ {} }}", def.name, fields)
        }
        StmtKind::EvalTimeout { timeout, body } => {
            format!(
                "eval_timeout {} {{\n{}\n{}}}",
                convert_expr(timeout),
                convert_block(body, depth + 1),
                pfx
            )
        }
        StmtKind::TryCatch {
            try_block,
            catch_var,
            catch_block,
            finally_block,
        } => {
            let fin = finally_block
                .as_ref()
                .map(|b| {
                    format!(
                        "\n{}finally {{\n{}\n{}}}",
                        pfx,
                        convert_block(b, depth + 1),
                        pfx
                    )
                })
                .unwrap_or_default();
            format!(
                "try {{\n{}\n{}}} catch (${}) {{\n{}\n{}}}{}",
                convert_block(try_block, depth + 1),
                pfx,
                catch_var,
                convert_block(catch_block, depth + 1),
                pfx,
                fin
            )
        }
        StmtKind::Given { topic, body } => {
            format!(
                "given ({}) {{\n{}\n{}}}",
                convert_expr(topic),
                convert_block(body, depth + 1),
                pfx
            )
        }
        StmtKind::When { cond, body } => {
            format!(
                "when ({}) {{\n{}\n{}}}",
                convert_expr(cond),
                convert_block(body, depth + 1),
                pfx
            )
        }
        StmtKind::DefaultCase { body } => {
            format!("default {{\n{}\n{}}}", convert_block(body, depth + 1), pfx)
        }
        StmtKind::FormatDecl { name, lines } => {
            let mut s = format!("format {} =\n", name);
            for ln in lines {
                s.push_str(ln);
                s.push('\n');
            }
            s.push('.');
            s
        }
        StmtKind::Tie {
            target,
            class,
            args,
        } => {
            let target_s = match target {
                crate::ast::TieTarget::Hash(h) => format!("%{}", h),
                crate::ast::TieTarget::Array(a) => format!("@{}", a),
                crate::ast::TieTarget::Scalar(s) => format!("${}", s),
            };
            let mut s = format!("tie {} {}", target_s, convert_expr(class));
            for a in args {
                s.push_str(&format!(", {}", convert_expr(a)));
            }
            s
        }
    };
    format!("{}{}{}", pfx, lab, body)
}

/// Convert a statement body without indentation prefix (for C-style for init).
fn convert_statement_body(s: &Statement) -> String {
    let lab = s
        .label
        .as_ref()
        .map(|l| format!("{}: ", l))
        .unwrap_or_default();
    let body = match &s.kind {
        StmtKind::Expression(e) => convert_expr_top(e),
        StmtKind::My(decls) => format!("my {}", convert_var_decls(decls)),
        _ => convert_statement(s, 0).trim().to_string(),
    };
    format!("{}{}", lab, body)
}

// ── Variable declarations ───────────────────────────────────────────────────

fn convert_var_decls(decls: &[VarDecl]) -> String {
    decls
        .iter()
        .map(|d| {
            let sig = match d.sigil {
                Sigil::Scalar => "$",
                Sigil::Array => "@",
                Sigil::Hash => "%",
                Sigil::Typeglob => "*",
            };
            let mut s = format!("{}{}", sig, d.name);
            if let Some(t) = d.type_annotation {
                s.push_str(&format!(" : {:?}", t));
            }
            if let Some(ref init) = d.initializer {
                s.push_str(&format!(" = {}", convert_expr_top(init)));
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ── Expression conversion ───────────────────────────────────────────────────

fn convert_expr_list(es: &[Expr]) -> String {
    es.iter().map(convert_expr).collect::<Vec<_>>().join(", ")
}

/// Convert an expression at statement level or assignment RHS — pipe chains
/// are emitted without outer parentheses.
fn convert_expr_top(e: &Expr) -> String {
    convert_expr_impl(e, true)
}

/// Convert an expression in a sub-expression context — pipe chains are wrapped
/// in parentheses to preserve precedence.
fn convert_expr(e: &Expr) -> String {
    convert_expr_impl(e, false)
}

fn convert_expr_impl(e: &Expr, top: bool) -> String {
    let mut segments: Vec<String> = Vec::new();
    let source = extract_pipe_source(e, &mut segments);
    if !segments.is_empty() {
        segments.reverse();
        let mut result = source;
        for seg in segments {
            result = format!("{} |> {}", result, seg);
        }
        if !top {
            result = format!("({})", result);
        }
        return result;
    }
    // No pipe chain — format with recursive sub-expression conversion.
    convert_expr_direct(e, top)
}

// ── Pipe chain extraction ───────────────────────────────────────────────────
//
// Walks the expression tree from the outermost call inward, peeling off
// each pipeable layer as a segment string.  Segments are pushed in
// outer-to-inner order; the caller reverses before joining with `|>`.

fn extract_pipe_source(e: &Expr, segments: &mut Vec<String>) -> String {
    match &e.kind {
        // ── Unary builtins ──────────────────────────────────────────────
        ExprKind::Uc(inner) => {
            segments.push("uc".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Lc(inner) => {
            segments.push("lc".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Ucfirst(inner) => {
            segments.push("ucfirst".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Lcfirst(inner) => {
            segments.push("lcfirst".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Fc(inner) => {
            segments.push("fc".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Chomp(inner) => {
            segments.push("chomp".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Chop(inner) => {
            segments.push("chop".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Length(inner) => {
            segments.push("length".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Abs(inner) => {
            segments.push("abs".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Int(inner) => {
            segments.push("int".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Sqrt(inner) => {
            segments.push("sqrt".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Sin(inner) => {
            segments.push("sin".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Cos(inner) => {
            segments.push("cos".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Exp(inner) => {
            segments.push("exp".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Log(inner) => {
            segments.push("log".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Hex(inner) => {
            segments.push("hex".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Oct(inner) => {
            segments.push("oct".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Chr(inner) => {
            segments.push("chr".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Ord(inner) => {
            segments.push("ord".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Defined(inner) => {
            segments.push("defined".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Ref(inner) => {
            segments.push("ref".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::ScalarContext(inner) => {
            segments.push("scalar".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Keys(inner) => {
            segments.push("keys".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Values(inner) => {
            segments.push("values".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Each(inner) => {
            segments.push("each".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Pop(inner) => {
            segments.push("pop".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Shift(inner) => {
            segments.push("shift".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::ReverseExpr(inner) => {
            segments.push("reverse".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Slurp(inner) => {
            segments.push("slurp".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Chdir(inner) => {
            segments.push("chdir".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Stat(inner) => {
            segments.push("stat".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Lstat(inner) => {
            segments.push("lstat".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Readlink(inner) => {
            segments.push("readlink".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Study(inner) => {
            segments.push("study".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Close(inner) => {
            segments.push("close".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Readdir(inner) => {
            segments.push("readdir".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Eval(inner) => {
            segments.push("eval".into());
            extract_pipe_source(inner, segments)
        }
        ExprKind::Require(inner) => {
            segments.push("require".into());
            extract_pipe_source(inner, segments)
        }

        // ── List-taking higher-order builtins ────────────────────────────
        ExprKind::MapExpr {
            block,
            list,
            flatten_array_refs,
        } => {
            let kw = if *flatten_array_refs {
                "flat_map"
            } else {
                "map"
            };
            segments.push(format!("{} {{\n{}\n}}", kw, convert_block(block, 0)));
            extract_pipe_source(list, segments)
        }
        ExprKind::MapExprComma {
            expr,
            list,
            flatten_array_refs,
        } => {
            let kw = if *flatten_array_refs {
                "flat_map"
            } else {
                "map"
            };
            // Convert comma form to block form for cleaner pipe syntax.
            segments.push(format!("{} {{ {} }}", kw, convert_expr_top(expr)));
            extract_pipe_source(list, segments)
        }
        ExprKind::GrepExpr { block, list } => {
            segments.push(format!("grep {{\n{}\n}}", convert_block(block, 0)));
            extract_pipe_source(list, segments)
        }
        ExprKind::GrepExprComma { expr, list } => {
            segments.push(format!("grep {{ {} }}", convert_expr_top(expr)));
            extract_pipe_source(list, segments)
        }
        ExprKind::SortExpr { cmp, list } => {
            let seg = match cmp {
                Some(SortComparator::Block(b)) => {
                    format!("sort {{\n{}\n}}", convert_block(b, 0))
                }
                Some(SortComparator::Code(e)) => {
                    format!("sort {}", convert_expr(e))
                }
                None => "sort".to_string(),
            };
            segments.push(seg);
            extract_pipe_source(list, segments)
        }
        ExprKind::JoinExpr { separator, list } => {
            segments.push(format!("join {}", convert_expr(separator)));
            extract_pipe_source(list, segments)
        }
        ExprKind::ReduceExpr { block, list } => {
            segments.push(format!("reduce {{\n{}\n}}", convert_block(block, 0)));
            extract_pipe_source(list, segments)
        }
        ExprKind::ForEachExpr { block, list } => {
            segments.push(format!("fore {{\n{}\n}}", convert_block(block, 0)));
            extract_pipe_source(list, segments)
        }

        // ── Parallel higher-order builtins ───────────────────────────────
        ExprKind::PMapExpr {
            block,
            list,
            progress,
            flat_outputs,
            on_cluster,
        } if progress.is_none() && on_cluster.is_none() => {
            let kw = if *flat_outputs { "pflat_map" } else { "pmap" };
            segments.push(format!("{} {{\n{}\n}}", kw, convert_block(block, 0)));
            extract_pipe_source(list, segments)
        }
        ExprKind::PGrepExpr {
            block,
            list,
            progress,
        } if progress.is_none() => {
            segments.push(format!("pgrep {{\n{}\n}}", convert_block(block, 0)));
            extract_pipe_source(list, segments)
        }
        ExprKind::PSortExpr {
            cmp,
            list,
            progress,
        } if progress.is_none() => {
            let seg = match cmp {
                Some(b) => format!("psort {{\n{}\n}}", convert_block(b, 0)),
                None => "psort".to_string(),
            };
            segments.push(seg);
            extract_pipe_source(list, segments)
        }

        // ── Generic function calls ───────────────────────────────────────
        ExprKind::FuncCall { name, args } if !args.is_empty() => {
            let seg = if args.len() == 1 {
                name.clone()
            } else {
                let rest = args[1..]
                    .iter()
                    .map(convert_expr)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {}", name, rest)
            };
            segments.push(seg);
            extract_pipe_source(&args[0], segments)
        }

        // ── Substitution with /r flag (value-returning) ──────────────────
        ExprKind::Substitution {
            expr,
            pattern,
            replacement,
            flags,
        } if flags.contains('r') => {
            // `$str =~ s/old/new/r` → `$str |> s/old/new/r`
            // In pipe context the parser auto-injects `r`, but keeping it
            // is harmless and explicit.
            segments.push(format!("s/{}/{}/{}", pattern, replacement, flags));
            extract_pipe_source(expr, segments)
        }

        // ── Transliterate with /r flag ───────────────────────────────────
        ExprKind::Transliterate {
            expr,
            from,
            to,
            flags,
        } if flags.contains('r') => {
            segments.push(format!("tr/{}/{}/{}", from, to, flags));
            extract_pipe_source(expr, segments)
        }

        // ── Base case: not pipeable ──────────────────────────────────────
        _ => convert_expr_direct(e, false),
    }
}

// ── Direct expression formatting (no pipe extraction) ───────────────────────
//
// Handles the common expression types with recursive `convert_expr` calls
// for sub-expressions.  Rare / complex variants delegate to `fmt::format_expr`.

fn convert_expr_direct(e: &Expr, top: bool) -> String {
    match &e.kind {
        // ── Leaf / simple (delegate to fmt) ──────────────────────────────
        ExprKind::Integer(_)
        | ExprKind::Float(_)
        | ExprKind::String(_)
        | ExprKind::Bareword(_)
        | ExprKind::Regex(..)
        | ExprKind::QW(_)
        | ExprKind::Undef
        | ExprKind::MagicConst(_)
        | ExprKind::ScalarVar(_)
        | ExprKind::ArrayVar(_)
        | ExprKind::HashVar(_)
        | ExprKind::Typeglob(_)
        | ExprKind::Wantarray
        | ExprKind::SubroutineRef(_)
        | ExprKind::SubroutineCodeRef(_) => fmt::format_expr(e),

        // ── Interpolated strings — parts may embed expressions ───────────
        ExprKind::InterpolatedString(parts) => {
            format!(
                "\"{}\"",
                parts
                    .iter()
                    .map(fmt::format_string_part)
                    .collect::<String>()
            )
        }

        // ── Binary operations ────────────────────────────────────────────
        ExprKind::BinOp { left, op, right } => {
            let inner = format!(
                "{} {} {}",
                convert_expr(left),
                fmt::format_binop(*op),
                convert_expr(right)
            );
            if top {
                inner
            } else {
                format!("({})", inner)
            }
        }

        // ── Unary / postfix ──────────────────────────────────────────────
        ExprKind::UnaryOp { op, expr } => {
            let inner = format!("{}{}", fmt::format_unary(*op), convert_expr(expr));
            if top {
                inner
            } else {
                format!("({})", inner)
            }
        }
        ExprKind::PostfixOp { expr, op } => {
            let inner = format!("{}{}", convert_expr(expr), fmt::format_postfix(*op));
            if top {
                inner
            } else {
                format!("({})", inner)
            }
        }

        // ── Assignment ───────────────────────────────────────────────────
        ExprKind::Assign { target, value } => {
            format!("{} = {}", convert_expr(target), convert_expr_top(value))
        }
        ExprKind::CompoundAssign { target, op, value } => format!(
            "{} {}= {}",
            convert_expr(target),
            fmt::format_binop(*op),
            convert_expr_top(value)
        ),

        // ── Ternary ──────────────────────────────────────────────────────
        ExprKind::Ternary {
            condition,
            then_expr,
            else_expr,
        } => format!(
            "({} ? {} : {})",
            convert_expr(condition),
            convert_expr(then_expr),
            convert_expr(else_expr)
        ),

        // ── Range / repeat ───────────────────────────────────────────────
        ExprKind::Range {
            from,
            to,
            exclusive,
        } => {
            let op = if *exclusive { "..." } else { ".." };
            format!("({} {} {})", convert_expr(from), op, convert_expr(to))
        }
        ExprKind::Repeat { expr, count } => {
            format!("({} x {})", convert_expr(expr), convert_expr(count))
        }

        // ── Calls ────────────────────────────────────────────────────────
        ExprKind::FuncCall { name, args } => format!("{}({})", name, convert_expr_list(args)),
        ExprKind::MethodCall {
            object,
            method,
            args,
            super_call,
        } => {
            let m = if *super_call {
                format!("SUPER::{}", method)
            } else {
                method.clone()
            };
            format!(
                "{}->{}({})",
                convert_expr(object),
                m,
                convert_expr_list(args)
            )
        }
        ExprKind::IndirectCall {
            target,
            args,
            ampersand,
            pass_caller_arglist,
        } => {
            if *pass_caller_arglist && args.is_empty() {
                format!("&{}", convert_expr(target))
            } else {
                let inner = format!("{}({})", convert_expr(target), convert_expr_list(args));
                if *ampersand {
                    format!("&{}", inner)
                } else {
                    inner
                }
            }
        }

        // ── Data structures ──────────────────────────────────────────────
        ExprKind::List(exprs) => format!("({})", convert_expr_list(exprs)),
        ExprKind::ArrayRef(elems) => format!("[{}]", convert_expr_list(elems)),
        ExprKind::HashRef(pairs) => {
            let inner = pairs
                .iter()
                .map(|(k, v)| format!("{} => {}", convert_expr(k), convert_expr(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{}}}", inner)
        }
        ExprKind::CodeRef { params, body } => {
            if params.is_empty() {
                format!("sub {{\n{}\n}}", convert_block(body, 0))
            } else {
                let sig = params
                    .iter()
                    .map(fmt::format_sub_sig_param)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("sub ({}) {{\n{}\n}}", sig, convert_block(body, 0))
            }
        }

        // ── Access / deref ───────────────────────────────────────────────
        ExprKind::ArrayElement { array, index } => {
            format!("${}[{}]", array, convert_expr(index))
        }
        ExprKind::HashElement { hash, key } => {
            format!("${}{{{}}}", hash, convert_expr(key))
        }
        ExprKind::ScalarRef(inner) => format!("\\{}", convert_expr(inner)),
        ExprKind::ArrowDeref { expr, index, kind } => match kind {
            DerefKind::Array => {
                format!("({})->[{}]", convert_expr(expr), convert_expr(index))
            }
            DerefKind::Hash => {
                format!("({})->{{{}}}", convert_expr(expr), convert_expr(index))
            }
            DerefKind::Call => {
                format!("({})->({})", convert_expr(expr), convert_expr(index))
            }
        },
        ExprKind::Deref { expr, kind } => match kind {
            Sigil::Scalar => format!("${{{}}}", convert_expr(expr)),
            Sigil::Array => format!("@{{${}}}", convert_expr(expr)),
            Sigil::Hash => format!("%{{${}}}", convert_expr(expr)),
            Sigil::Typeglob => format!("*{{${}}}", convert_expr(expr)),
        },

        // ── Print / say / die / warn ─────────────────────────────────────
        ExprKind::Print { handle, args } => {
            let h = handle
                .as_ref()
                .map(|h| format!("{} ", h))
                .unwrap_or_default();
            format!("print {}{}", h, convert_expr_list(args))
        }
        ExprKind::Say { handle, args } => {
            let h = handle
                .as_ref()
                .map(|h| format!("{} ", h))
                .unwrap_or_default();
            format!("say {}{}", h, convert_expr_list(args))
        }
        ExprKind::Printf { handle, args } => {
            let h = handle
                .as_ref()
                .map(|h| format!("{} ", h))
                .unwrap_or_default();
            format!("printf {}{}", h, convert_expr_list(args))
        }
        ExprKind::Die(args) => {
            if args.is_empty() {
                "die".to_string()
            } else {
                format!("die {}", convert_expr_list(args))
            }
        }
        ExprKind::Warn(args) => {
            if args.is_empty() {
                "warn".to_string()
            } else {
                format!("warn {}", convert_expr_list(args))
            }
        }

        // ── Regex (non-piped) ────────────────────────────────────────────
        ExprKind::Match {
            expr,
            pattern,
            flags,
            ..
        } => format!("({} =~ /{}/{})", convert_expr(expr), pattern, flags),
        ExprKind::Substitution {
            expr,
            pattern,
            replacement,
            flags,
        } => format!(
            "({} =~ s/{}/{}/{})",
            convert_expr(expr),
            pattern,
            replacement,
            flags
        ),
        ExprKind::Transliterate {
            expr,
            from,
            to,
            flags,
        } => format!("({} =~ tr/{}/{}/{})", convert_expr(expr), from, to, flags),

        // ── Postfix modifiers ────────────────────────────────────────────
        ExprKind::PostfixIf { expr, condition } => {
            format!("{} if {}", convert_expr(expr), convert_expr(condition))
        }
        ExprKind::PostfixUnless { expr, condition } => {
            format!("{} unless {}", convert_expr(expr), convert_expr(condition))
        }
        ExprKind::PostfixWhile { expr, condition } => {
            format!("{} while {}", convert_expr(expr), convert_expr(condition))
        }
        ExprKind::PostfixUntil { expr, condition } => {
            format!("{} until {}", convert_expr(expr), convert_expr(condition))
        }
        ExprKind::PostfixForeach { expr, list } => {
            format!("{} foreach {}", convert_expr(expr), convert_expr(list))
        }

        // ── Higher-order forms (fallback when not piped — e.g. empty list) ─
        ExprKind::MapExpr {
            block,
            list,
            flatten_array_refs,
        } => {
            let kw = if *flatten_array_refs {
                "flat_map"
            } else {
                "map"
            };
            format!(
                "{} {{\n{}\n}} {}",
                kw,
                convert_block(block, 0),
                convert_expr(list)
            )
        }
        ExprKind::GrepExpr { block, list } => {
            format!(
                "grep {{\n{}\n}} {}",
                convert_block(block, 0),
                convert_expr(list)
            )
        }
        ExprKind::SortExpr { cmp, list } => match cmp {
            Some(SortComparator::Block(b)) => {
                format!(
                    "sort {{\n{}\n}} {}",
                    convert_block(b, 0),
                    convert_expr(list)
                )
            }
            Some(SortComparator::Code(e)) => {
                format!("sort {} {}", convert_expr(e), convert_expr(list))
            }
            None => format!("sort {}", convert_expr(list)),
        },
        ExprKind::JoinExpr { separator, list } => {
            format!("join({}, {})", convert_expr(separator), convert_expr(list))
        }
        ExprKind::SplitExpr {
            pattern,
            string,
            limit,
        } => match limit {
            Some(l) => format!(
                "split({}, {}, {})",
                convert_expr(pattern),
                convert_expr(string),
                convert_expr(l)
            ),
            None => format!("split({}, {})", convert_expr(pattern), convert_expr(string)),
        },

        // ── Bless ────────────────────────────────────────────────────────
        ExprKind::Bless { ref_expr, class } => match class {
            Some(c) => format!("bless({}, {})", convert_expr(ref_expr), convert_expr(c)),
            None => format!("bless({})", convert_expr(ref_expr)),
        },

        // ── Push / unshift / splice ──────────────────────────────────────
        ExprKind::Push { array, values } => {
            format!(
                "push({}, {})",
                convert_expr(array),
                convert_expr_list(values)
            )
        }
        ExprKind::Unshift { array, values } => {
            format!(
                "unshift({}, {})",
                convert_expr(array),
                convert_expr_list(values)
            )
        }

        // ── Algebraic match ──────────────────────────────────────────────
        ExprKind::AlgebraicMatch { subject, arms } => {
            let arms_s = arms
                .iter()
                .map(|a| {
                    let guard_s = a
                        .guard
                        .as_ref()
                        .map(|g| format!(" if {}", convert_expr(g)))
                        .unwrap_or_default();
                    format!(
                        "{}{} => {}",
                        fmt::format_match_pattern(&a.pattern),
                        guard_s,
                        convert_expr(&a.body)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("match ({}) {{ {} }}", convert_expr(subject), arms_s)
        }

        // ── Everything else: delegate to fmt ─────────────────────────────
        _ => fmt::format_expr(e),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// Helper: convert code and strip the shebang line for easier assertions.
    fn convert(code: &str) -> String {
        let p = parse(code).expect("parse failed");
        let out = convert_program(&p);
        // Strip shebang line for test comparisons
        out.strip_prefix("#!/usr/bin/env perlrs\n")
            .unwrap_or(&out)
            .to_string()
    }

    #[test]
    fn unary_builtin_pipe() {
        assert_eq!(convert("uc($x);"), "$x |> uc");
        assert_eq!(convert("length($str);"), "$str |> length");
    }

    #[test]
    fn nested_unary_pipe() {
        let out = convert("uc(lc($x));");
        assert_eq!(out, "$x |> lc |> uc");
    }

    #[test]
    fn nested_builtin_chain_pipe() {
        let out = convert("chomp(lc(uc($x)));");
        assert_eq!(out, "$x |> uc |> lc |> chomp");
    }

    #[test]
    fn deeply_nested_pipe() {
        let out = convert("length(chomp(lc(uc($x))));");
        assert_eq!(out, "$x |> uc |> lc |> chomp |> length");
    }

    #[test]
    fn map_grep_sort_pipe() {
        let out = convert("sort { $a <=> $b } map { $_ * 2 } grep { $_ > 0 } @numbers;");
        assert!(out.contains("|> grep"));
        assert!(out.contains("|> map"));
        assert!(out.contains("|> sort"));
    }

    #[test]
    fn join_pipe() {
        let out = convert(r#"join(",", sort(@arr));"#);
        assert!(out.contains("|> sort"));
        assert!(out.contains("|> join"));
    }

    #[test]
    fn no_semicolons() {
        let out = convert("my $x = 1;\nmy $y = 2;");
        assert!(!out.contains(';'));
        assert!(out.contains("my $x = 1"));
        assert!(out.contains("my $y = 2"));
    }

    #[test]
    fn assignment_rhs_pipe() {
        let out = convert("my $x = uc(lc($str));");
        assert_eq!(out, "my $x = $str |> lc |> uc");
    }

    #[test]
    fn pipe_in_subexpression_parenthesized() {
        let out = convert("$x + uc(lc($str));");
        // The pipe chain should be parenthesized inside the binary op.
        assert!(out.contains("($str |> lc |> uc)"));
    }

    #[test]
    fn sub_body_indented() {
        let out = convert("sub foo { return uc(lc($x)); }");
        assert!(out.contains("|> lc |> uc"));
        // Body should be indented
        assert!(out.contains("    return"));
    }

    #[test]
    fn if_condition_converted() {
        let out = convert("if (defined(length($x))) { 1; }");
        assert!(out.contains("|> length |> defined"));
    }

    #[test]
    fn method_call_preserved() {
        let out = convert("$obj->method($x);");
        assert!(out.contains("->method"));
    }

    #[test]
    fn substitution_r_flag_piped() {
        let out = convert(r#"($str =~ s/old/new/r);"#);
        assert!(out.contains("|> s/old/new/r"));
    }

    #[test]
    fn user_func_call_pipe() {
        let out = convert("sub trim { } trim(uc($x));");
        assert!(out.contains("$x |> uc |> trim"));
    }

    #[test]
    fn user_func_extra_args_pipe() {
        let out = convert("sub process { } process(uc($x), 42);");
        // Pipe RHS uses bare args, not parens
        assert!(out.contains("$x |> uc |> process 42"));
    }

    #[test]
    fn map_grep_sort_chain_pipe() {
        let out = convert("join(',', sort { $a <=> $b } map { $_ * 2 } grep { $_ > 0 } @nums);");
        assert!(out.contains("@nums |> grep"));
        assert!(out.contains("|> map"));
        assert!(out.contains("|> sort"));
        assert!(out.contains("|> join"));
    }

    #[test]
    fn reduce_pipe() {
        let out = convert("use List::Util 'reduce';\nreduce { $a + $b } @nums;");
        assert!(out.contains("|> reduce"));
    }

    #[test]
    fn shebang_prepended() {
        let p = parse("print 1;").expect("parse failed");
        let out = convert_program(&p);
        assert!(out.starts_with("#!/usr/bin/env perlrs\n"));
    }

    #[test]
    fn indentation_in_blocks() {
        let out = convert("if ($x) { print 1; print 2; }");
        // Inner statements should have 4-space indent
        assert!(out.contains("\n    print 1\n    print 2\n"));
    }

    #[test]
    fn binop_no_parens_at_top() {
        let out = convert("my $x = $a + $b;");
        // At top level / assignment RHS, no parens around binop
        assert!(out.contains("= $a + $b"));
        assert!(!out.contains("= ($a + $b)"));
    }
}
