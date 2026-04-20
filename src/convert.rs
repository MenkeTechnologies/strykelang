//! Convert standard Perl source to idiomatic stryke syntax.
//!
//! Transformations applied:
//! - Nested function/builtin calls → `|>` pipe-forward chains
//! - `map { BLOCK } LIST` → `LIST |> map { BLOCK }`
//! - `grep { BLOCK } LIST` → `LIST |> grep { BLOCK }`
//! - `sort [{ CMP }] LIST` → `LIST |> sort [{ CMP }]`
//! - `join(SEP, LIST)` → `LIST |> join SEP`
//! - No trailing semicolons (newline terminates statements)
//! - 4-space indentation for block bodies
//! - `#!/usr/bin/env stryke` shebang prepended
//! - Pipe RHS uses bare args: `|> binmode ":utf8"` not `|> binmode(":utf8")`

#![allow(unused_variables)]

use crate::ast::*;
use crate::fmt;
use std::cell::RefCell;

const INDENT: &str = "    ";

thread_local! {
    static OUTPUT_DELIM: RefCell<Option<char>> = const { RefCell::new(None) };
}

/// Options for the convert module.
#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// Custom delimiter for s///, tr///, m// patterns (e.g., '|', '#', '!').
    pub output_delim: Option<char>,
}

fn get_output_delim() -> Option<char> {
    OUTPUT_DELIM.with(|d| *d.borrow())
}

fn set_output_delim(delim: Option<char>) {
    OUTPUT_DELIM.with(|d| *d.borrow_mut() = delim);
}

/// Choose the output delimiter: custom if set, else the original from the AST.
fn choose_delim(original: char) -> char {
    get_output_delim().unwrap_or(original)
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Convert a parsed Perl program to stryke syntax.
pub fn convert_program(p: &Program) -> String {
    convert_program_with_options(p, &ConvertOptions::default())
}

/// Convert a parsed Perl program to stryke syntax with custom options.
pub fn convert_program_with_options(p: &Program, opts: &ConvertOptions) -> String {
    set_output_delim(opts.output_delim);
    let body = convert_statements(&p.statements, 0);
    set_output_delim(None);
    format!("#!/usr/bin/env stryke\n{}", body)
}

// ── Block / Statement ───────────────────────────────────────────────────────

fn convert_block(b: &Block, depth: usize) -> String {
    convert_statements(b, depth)
}

/// Convert a slice of statements, merging bare say/print with following string literals.
fn convert_statements(stmts: &[Statement], depth: usize) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < stmts.len() {
        // Check for bare say/print followed by string literal
        if let Some(merged) = try_merge_say_print(&stmts[i..], depth) {
            out.push(merged);
            i += 2; // skip both statements
        } else {
            out.push(convert_statement(&stmts[i], depth));
            i += 1;
        }
    }
    out.join("\n")
}

/// Try to merge a bare say/print statement with a following string literal.
/// Returns Some(merged_string) if merge happened, None otherwise.
fn try_merge_say_print(stmts: &[Statement], depth: usize) -> Option<String> {
    if stmts.len() < 2 {
        return None;
    }
    let pfx = indent(depth);

    // First statement must be bare say or print (no args, no handle)
    let (is_say, handle) = match &stmts[0].kind {
        StmtKind::Expression(e) => match &e.kind {
            ExprKind::Say { handle, args } if args.is_empty() => (true, handle),
            ExprKind::Print { handle, args } if args.is_empty() => (false, handle),
            _ => return None,
        },
        _ => return None,
    };

    // No handle allowed for merge
    if handle.is_some() {
        return None;
    }

    // Second statement must be a bare string expression
    let str_expr = match &stmts[1].kind {
        StmtKind::Expression(e) => e,
        _ => return None,
    };

    // Format as: p "string" or print "string"
    let cmd = if is_say { "p" } else { "print" };
    let arg = convert_expr_top(str_expr);
    Some(format!("{}{} {}", pfx, cmd, arg))
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
                convert_expr_top(condition),
                convert_block(body, depth + 1),
                pfx
            );
            for (c, b) in elsifs {
                s.push_str(&format!(
                    " elsif ({}) {{\n{}\n{}}}",
                    convert_expr_top(c),
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
                convert_expr_top(condition),
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
                convert_expr_top(condition),
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
                convert_expr_top(condition),
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
                convert_expr_top(condition)
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
                "fn {}{} {{\n{}\n{}}}",
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
                .map(|f| format!("{} => {}", f.name, f.ty.display_name()))
                .collect::<Vec<_>>()
                .join(", ");
            format!("struct {} {{ {} }}", def.name, fields)
        }
        StmtKind::EnumDecl { def } => {
            let variants = def
                .variants
                .iter()
                .map(|v| {
                    if let Some(ty) = &v.ty {
                        format!("{} => {}", v.name, ty.display_name())
                    } else {
                        v.name.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("enum {} {{ {} }}", def.name, variants)
        }
        StmtKind::ClassDecl { def } => {
            let prefix = if def.is_abstract {
                "abstract "
            } else if def.is_final {
                "final "
            } else {
                ""
            };
            let mut parts = vec![format!("{}class {}", prefix, def.name)];
            if !def.extends.is_empty() {
                parts.push(format!("extends {}", def.extends.join(", ")));
            }
            if !def.implements.is_empty() {
                parts.push(format!("impl {}", def.implements.join(", ")));
            }
            let fields = def
                .fields
                .iter()
                .map(|f| {
                    let vis = match f.visibility {
                        crate::ast::Visibility::Private => "priv ",
                        crate::ast::Visibility::Protected => "prot ",
                        crate::ast::Visibility::Public => "",
                    };
                    format!("{}{}: {}", vis, f.name, f.ty.display_name())
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!("{} {{ {} }}", parts.join(" "), fields)
        }
        StmtKind::TraitDecl { def } => {
            let methods = def
                .methods
                .iter()
                .map(|m| format!("fn {}", m.name))
                .collect::<Vec<_>>()
                .join("; ");
            format!("trait {} {{ {} }}", def.name, methods)
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
            if let Some(ref t) = d.type_annotation {
                s.push_str(&format!(" : {}", t.display_name()));
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

/// Format a string part for converted output.
/// Uses simple `$name` when possible, `${name}` only when needed.
fn convert_string_part(p: &StringPart) -> String {
    match p {
        StringPart::Literal(s) => fmt::escape_interpolated_literal(s),
        StringPart::ScalarVar(n) => {
            // Use ${} only if name has special chars or would be ambiguous
            if needs_braces(n) {
                format!("${{{}}}", n)
            } else {
                format!("${}", n)
            }
        }
        StringPart::ArrayVar(n) => {
            if needs_braces(n) {
                format!("@{{{}}}", n)
            } else {
                format!("@{}", n)
            }
        }
        StringPart::Expr(e) => fmt::format_expr(e),
    }
}

/// Check if a variable name needs braces in interpolation.
fn needs_braces(name: &str) -> bool {
    // Empty or starts with digit needs braces
    if name.is_empty() || name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return true;
    }
    // Contains non-identifier chars
    !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
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
        // 1-2 stages: direct call syntax (e.g., `print "$x"`, `uc lc $x`)
        // 3+ stages: thread macro (e.g., `t $x lc uc print`)
        if segments.len() <= 2 {
            let result = format!("{} {}", segments.join(" "), source);
            if !top {
                return format!("({})", result);
            }
            return result;
        }
        // 3+ stages: use thread macro
        let stages = segments.join(" ");
        // Strip outer parens from source if it's a parenthesized list/thread
        let source = if source.starts_with("(t ") || source.starts_with("((") {
            source[1..source.len() - 1].to_string()
        } else {
            source
        };
        let result = format!("t {} {}", source, stages);
        if !top {
            return format!("({})", result);
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
            stream,
        } => {
            let kw = match (*flatten_array_refs, *stream) {
                (true, true) => "flat_maps",
                (true, false) => "flat_map",
                (false, true) => "maps",
                (false, false) => "map",
            };
            segments.push(format!("{} {{\n{}\n}}", kw, convert_block(block, 0)));
            extract_pipe_source(list, segments)
        }
        ExprKind::MapExprComma {
            expr,
            list,
            flatten_array_refs,
            stream,
        } => {
            let kw = match (*flatten_array_refs, *stream) {
                (true, true) => "flat_maps",
                (true, false) => "flat_map",
                (false, true) => "maps",
                (false, false) => "map",
            };
            // Convert comma form to block form for cleaner pipe syntax.
            segments.push(format!("{} {{ {} }}", kw, convert_expr_top(expr)));
            extract_pipe_source(list, segments)
        }
        ExprKind::GrepExpr {
            block,
            list,
            keyword,
        } => {
            segments.push(format!(
                "{} {{\n{}\n}}",
                keyword.as_str(),
                convert_block(block, 0)
            ));
            extract_pipe_source(list, segments)
        }
        ExprKind::GrepExprComma {
            expr,
            list,
            keyword,
        } => {
            segments.push(format!(
                "{} {{ {} }}",
                keyword.as_str(),
                convert_expr_top(expr)
            ));
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
            stream: _,
        } if progress.is_none() && on_cluster.is_none() => {
            let kw = if *flat_outputs { "pflat_map" } else { "pmap" };
            segments.push(format!("{} {{\n{}\n}}", kw, convert_block(block, 0)));
            extract_pipe_source(list, segments)
        }
        ExprKind::PGrepExpr {
            block,
            list,
            progress,
            stream: _,
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

        // ── Print / say with single arg → pipe ───────────────────────────
        // say adds newline → p; print does not → print
        ExprKind::Say { handle: None, args } if args.len() == 1 => {
            segments.push("p".into());
            extract_pipe_source(&args[0], segments)
        }
        ExprKind::Print { handle: None, args } if args.len() == 1 => {
            segments.push("print".into());
            extract_pipe_source(&args[0], segments)
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
            delim,
        } if flags.contains('r') => {
            // `$str =~ s/old/new/r` → `$str |> s/old/new/r`
            // In pipe context the parser auto-injects `r`, but keeping it
            // is harmless and explicit.
            let d = choose_delim(*delim);
            segments.push(format!(
                "s{}{}{}{}{}{}",
                d,
                fmt::escape_regex_part(pattern),
                d,
                fmt::escape_regex_part(replacement),
                d,
                flags
            ));
            extract_pipe_source(expr, segments)
        }

        // ── Transliterate with /r flag ───────────────────────────────────
        ExprKind::Transliterate {
            expr,
            from,
            to,
            flags,
            delim,
        } if flags.contains('r') => {
            let d = choose_delim(*delim);
            segments.push(format!(
                "tr{}{}{}{}{}{}",
                d,
                fmt::escape_regex_part(from),
                d,
                fmt::escape_regex_part(to),
                d,
                flags
            ));
            extract_pipe_source(expr, segments)
        }

        // ── Single-element list: unwrap and continue extraction ──────────
        ExprKind::List(elems) if elems.len() == 1 => extract_pipe_source(&elems[0], segments),

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
                parts.iter().map(convert_string_part).collect::<String>()
            )
        }

        // ── Binary operations ────────────────────────────────────────────
        ExprKind::BinOp { left, op, right } => {
            format!(
                "{} {} {}",
                convert_expr(left),
                fmt::format_binop(*op),
                convert_expr(right)
            )
        }

        // ── Unary / postfix ──────────────────────────────────────────────
        ExprKind::UnaryOp { op, expr } => {
            format!("{}{}", fmt::format_unary(*op), convert_expr(expr))
        }
        ExprKind::PostfixOp { expr, op } => {
            format!("{}{}", convert_expr(expr), fmt::format_postfix(*op))
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
            "{} ? {} : {}",
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
            format!("{} {} {}", convert_expr(from), op, convert_expr(to))
        }
        ExprKind::Repeat { expr, count } => {
            format!("{} x {}", convert_expr(expr), convert_expr(count))
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
                format!("fn {{\n{}\n}}", convert_block(body, 0))
            } else {
                let sig = params
                    .iter()
                    .map(fmt::format_sub_sig_param)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn ({}) {{\n{}\n}}", sig, convert_block(body, 0))
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
        // print has no newline; say/p adds newline
        ExprKind::Print { handle, args } => {
            let h = handle
                .as_ref()
                .map(|h| format!("{} ", h))
                .unwrap_or_default();
            format!("print {}{}", h, convert_expr_list(args))
        }
        ExprKind::Say { handle, args } => {
            if let Some(h) = handle {
                format!("say {} {}", h, convert_expr_list(args))
            } else {
                format!("p {}", convert_expr_list(args))
            }
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
            delim,
            ..
        } => {
            let d = choose_delim(*delim);
            format!(
                "{} =~ {}{}{}{}",
                convert_expr(expr),
                d,
                fmt::escape_regex_part(pattern),
                d,
                flags
            )
        }
        ExprKind::Substitution {
            expr,
            pattern,
            replacement,
            flags,
            delim,
        } => {
            let d = choose_delim(*delim);
            format!(
                "{} =~ s{}{}{}{}{}{}",
                convert_expr(expr),
                d,
                fmt::escape_regex_part(pattern),
                d,
                fmt::escape_regex_part(replacement),
                d,
                flags
            )
        }
        ExprKind::Transliterate {
            expr,
            from,
            to,
            flags,
            delim,
        } => {
            let d = choose_delim(*delim);
            format!(
                "{} =~ tr{}{}{}{}{}{}",
                convert_expr(expr),
                d,
                fmt::escape_regex_part(from),
                d,
                fmt::escape_regex_part(to),
                d,
                flags
            )
        }

        // ── Postfix modifiers ────────────────────────────────────────────
        ExprKind::PostfixIf { expr, condition } => {
            format!("{} if {}", convert_expr_top(expr), convert_expr(condition))
        }
        ExprKind::PostfixUnless { expr, condition } => {
            format!(
                "{} unless {}",
                convert_expr_top(expr),
                convert_expr(condition)
            )
        }
        ExprKind::PostfixWhile { expr, condition } => {
            format!(
                "{} while {}",
                convert_expr_top(expr),
                convert_expr(condition)
            )
        }
        ExprKind::PostfixUntil { expr, condition } => {
            format!(
                "{} until {}",
                convert_expr_top(expr),
                convert_expr(condition)
            )
        }
        ExprKind::PostfixForeach { expr, list } => {
            format!("{} for {}", convert_expr_top(expr), convert_expr(list))
        }

        // ── Higher-order forms (fallback when not piped — e.g. empty list) ─
        ExprKind::MapExpr {
            block,
            list,
            flatten_array_refs,
            stream,
        } => {
            let kw = match (*flatten_array_refs, *stream) {
                (true, true) => "flat_maps",
                (true, false) => "flat_map",
                (false, true) => "maps",
                (false, false) => "map",
            };
            format!(
                "{} {{\n{}\n}} {}",
                kw,
                convert_block(block, 0),
                convert_expr(list)
            )
        }
        ExprKind::GrepExpr {
            block,
            list,
            keyword,
        } => {
            format!(
                "{} {{\n{}\n}} {}",
                keyword.as_str(),
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
        out.strip_prefix("#!/usr/bin/env stryke\n")
            .unwrap_or(&out)
            .to_string()
    }

    #[test]
    fn unary_builtin_direct() {
        // Single-stage: direct call syntax
        assert_eq!(convert("uc($x)"), "uc $x");
        assert_eq!(convert("length($str)"), "length $str");
    }

    #[test]
    fn nested_unary_direct() {
        // 2-stage: direct call syntax (inner-to-outer order)
        let out = convert("uc(lc($x))");
        assert_eq!(out, "lc uc $x");
    }

    #[test]
    fn nested_builtin_chain_thread() {
        let out = convert("chomp(lc(uc($x)))");
        assert_eq!(out, "t $x uc lc chomp");
    }

    #[test]
    fn deeply_nested_thread() {
        let out = convert("length(chomp(lc(uc($x))))");
        assert_eq!(out, "t $x uc lc chomp length");
    }

    #[test]
    fn map_grep_sort_thread() {
        let out = convert("sort { $a <=> $b } map { $_ * 2 } grep { $_ > 0 } @numbers");
        assert!(out.contains("t @numbers grep"));
        assert!(out.contains(" map"));
        assert!(out.contains(" sort"));
    }

    #[test]
    fn join_direct() {
        let out = convert(r#"join(",", sort(@arr))"#);
        // 2-stage: direct call (inner-to-outer)
        assert!(out.contains("sort join \",\" @arr"));
    }

    #[test]
    fn no_semicolons() {
        let out = convert("my $x = 1;\nmy $y = 2");
        assert!(!out.contains(';'));
        assert!(out.contains("my $x = 1"));
        assert!(out.contains("my $y = 2"));
    }

    #[test]
    fn assignment_rhs_direct() {
        let out = convert("my $x = uc(lc($str))");
        // 2-stage: direct call
        assert_eq!(out, "my $x = lc uc $str");
    }

    #[test]
    fn chain_in_subexpression_parenthesized() {
        let out = convert("$x + uc(lc($str))");
        // 2-stage chain should be parenthesized inside the binary op.
        assert!(out.contains("(lc uc $str)"));
    }

    #[test]
    fn fn_body_indented() {
        let out = convert("sub foo { return uc(lc($x)); }");
        assert!(out.contains("fn foo"));
        // 2-stage: direct call
        assert!(out.contains("lc uc $x"));
        // Body should be indented
        assert!(out.contains("    return"));
    }

    #[test]
    fn if_condition_converted() {
        let out = convert("if (defined(length($x))) { 1; }");
        // 2-stage: direct call
        assert!(out.contains("length defined $x"));
    }

    #[test]
    fn method_call_preserved() {
        let out = convert("$obj->method($x)");
        assert!(out.contains("->method"));
    }

    #[test]
    fn substitution_r_flag_direct() {
        // Single stage: direct syntax
        let out = convert(r#"($str =~ s/old/new/r)"#);
        assert!(out.contains("s/old/new/r $str"));
    }

    #[test]
    fn user_func_call_direct() {
        let out = convert("sub trim { } trim(uc($x))");
        assert!(out.contains("fn trim"));
        // 2-stage: direct call (inner-to-outer)
        assert!(out.contains("uc trim $x"));
    }

    #[test]
    fn user_func_extra_args_direct() {
        let out = convert("sub process { } process(uc($x), 42)");
        assert!(out.contains("fn process"));
        // Direct call (inner-to-outer): uc process 42 $x
        assert!(out.contains("uc process 42 $x"));
    }

    #[test]
    fn map_grep_sort_chain_thread() {
        let out = convert("join(',', sort { $a <=> $b } map { $_ * 2 } grep { $_ > 0 } @nums)");
        assert!(out.contains("t @nums grep"));
        assert!(out.contains(" map"));
        assert!(out.contains(" sort"));
        assert!(out.contains(" join"));
    }

    #[test]
    fn reduce_direct() {
        // Single stage with block: direct syntax
        let out = convert("use List::Util 'reduce';\nreduce { $a + $b } @nums");
        assert!(out.contains("reduce {\n$a + $b\n} @nums"));
    }

    #[test]
    fn shebang_prepended() {
        let p = parse("print 1").expect("parse failed");
        let out = convert_program(&p);
        assert!(out.starts_with("#!/usr/bin/env stryke\n"));
    }

    #[test]
    fn indentation_in_blocks() {
        let out = convert("if ($x) { print 1; print 2; }");
        // Single stage: direct call syntax
        assert!(out.contains("\n    print 1\n    print 2\n"));
    }

    #[test]
    fn binop_no_parens_at_top() {
        let out = convert("my $x = $a + $b");
        // At top level / assignment RHS, no parens around binop
        assert!(out.contains("= $a + $b"));
        assert!(!out.contains("= ($a + $b)"));
    }

    fn convert_with_delim(code: &str, delim: char) -> String {
        let p = parse(code).expect("parse failed");
        let opts = ConvertOptions {
            output_delim: Some(delim),
        };
        let out = convert_program_with_options(&p, &opts);
        out.strip_prefix("#!/usr/bin/env stryke\n")
            .unwrap_or(&out)
            .to_string()
    }

    #[test]
    fn output_delim_substitution() {
        let out = convert_with_delim("$x =~ s/foo/bar/g;", '|');
        assert_eq!(out, "$x =~ s|foo|bar|g");
    }

    #[test]
    fn output_delim_transliterate() {
        let out = convert_with_delim("$y =~ tr/a-z/A-Z/;", '#');
        assert_eq!(out, "$y =~ tr#a-z#A-Z#");
    }

    #[test]
    fn output_delim_match() {
        let out = convert_with_delim("$z =~ m/pattern/i;", '!');
        assert_eq!(out, "$z =~ !pattern!i");
    }

    #[test]
    fn output_delim_preserves_original_when_none() {
        let out = convert("$x =~ s#old#new#g");
        assert_eq!(out, "$x =~ s#old#new#g");
    }
}
