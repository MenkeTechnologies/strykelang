//! Pretty-print parsed Perl back to source (`stryke --fmt`).
//! Regenerate with `python3 tools/gen_fmt.py` after `ast.rs` changes.

#![allow(unused_variables)] // generated `match` arms name fields not always used

use crate::ast::*;

const INDENT: &str = "    ";

/// Format a whole program as Perl-like source.
pub fn format_program(p: &Program) -> String {
    p.statements
        .iter()
        .map(|s| format_statement_indent(s, 0))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn format_sub_sig_param(p: &SubSigParam) -> String {
    use crate::ast::MatchArrayElem;
    match p {
        SubSigParam::Scalar(name, ty, default) => {
            let mut s = format!("${}", name);
            if let Some(t) = ty {
                s.push_str(": ");
                s.push_str(&t.display_name());
            }
            if let Some(d) = default {
                s.push_str(" = ");
                s.push_str(&format_expr(d));
            }
            s
        }
        SubSigParam::Array(name, default) => {
            let mut s = format!("@{}", name);
            if let Some(d) = default {
                s.push_str(" = ");
                s.push_str(&format_expr(d));
            }
            s
        }
        SubSigParam::Hash(name, default) => {
            let mut s = format!("%{}", name);
            if let Some(d) = default {
                s.push_str(" = ");
                s.push_str(&format_expr(d));
            }
            s
        }
        SubSigParam::ArrayDestruct(elems) => {
            let inner = elems
                .iter()
                .map(|x| match x {
                    MatchArrayElem::Expr(e) => format_expr(e),
                    MatchArrayElem::CaptureScalar(name) => format!("${}", name),
                    MatchArrayElem::Rest => "*".to_string(),
                    MatchArrayElem::RestBind(name) => format!("@{}", name),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        SubSigParam::HashDestruct(pairs) => {
            let inner = pairs
                .iter()
                .map(|(k, v)| format!("{} => ${}", k, v))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {inner} }}")
        }
    }
}

#[allow(dead_code)]
fn format_statement(s: &Statement) -> String {
    format_statement_indent(s, 0)
}

fn format_statement_indent(s: &Statement, depth: usize) -> String {
    let prefix = INDENT.repeat(depth);
    let lab = s
        .label
        .as_ref()
        .map(|l| format!("{}: ", l))
        .unwrap_or_default();
    let body = match &s.kind {
        StmtKind::Expression(e) => format_expr(e),
        StmtKind::If {
            condition,
            body,
            elsifs,
            else_block,
        } => {
            let mut s = format!(
                "if ({}) {{\n{}\n{}}}",
                format_expr(condition),
                format_block_indent(body, depth + 1),
                prefix
            );
            for (c, b) in elsifs {
                s.push_str(&format!(
                    " elsif ({}) {{\n{}\n{}}}",
                    format_expr(c),
                    format_block_indent(b, depth + 1),
                    prefix
                ));
            }
            if let Some(eb) = else_block {
                s.push_str(&format!(
                    " else {{\n{}\n{}}}",
                    format_block_indent(eb, depth + 1),
                    prefix
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
                format_expr(condition),
                format_block_indent(body, depth + 1),
                prefix
            );
            if let Some(eb) = else_block {
                s.push_str(&format!(
                    " else {{\n{}\n{}}}",
                    format_block_indent(eb, depth + 1),
                    prefix
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
                format_expr(condition),
                format_block_indent(body, depth + 1),
                prefix
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(
                    " continue {{\n{}\n{}}}",
                    format_block_indent(cb, depth + 1),
                    prefix
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
                format_expr(condition),
                format_block_indent(body, depth + 1),
                prefix
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(
                    " continue {{\n{}\n{}}}",
                    format_block_indent(cb, depth + 1),
                    prefix
                ));
            }
            s
        }
        StmtKind::DoWhile { body, condition } => {
            format!(
                "do {{\n{}\n{}}} while ({})",
                format_block_indent(body, depth + 1),
                prefix,
                format_expr(condition)
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
                .map(|s| format_statement_indent(s, 0))
                .unwrap_or_default();
            let cond = condition.as_ref().map(format_expr).unwrap_or_default();
            let st = step.as_ref().map(format_expr).unwrap_or_default();
            let mut s = format!(
                "{}for ({}; {}; {}) {{\n{}\n{}}}",
                lb,
                ini,
                cond,
                st,
                format_block_indent(body, depth + 1),
                prefix
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(
                    " continue {{\n{}\n{}}}",
                    format_block_indent(cb, depth + 1),
                    prefix
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
                format_expr(list),
                format_block_indent(body, depth + 1),
                prefix
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(
                    " continue {{\n{}\n{}}}",
                    format_block_indent(cb, depth + 1),
                    prefix
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
                        .map(format_sub_sig_param)
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
                format_block_indent(body, depth + 1),
                prefix
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
                format!("use {} {}", module, format_expr_list(imports))
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
                format!("no {} {}", module, format_expr_list(imports))
            }
        }
        StmtKind::Return(e) => e
            .as_ref()
            .map(|x| format!("return {}", format_expr(x)))
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
        StmtKind::My(decls) => format!("my {}", format_var_decls(decls)),
        StmtKind::Our(decls) => format!("our {}", format_var_decls(decls)),
        StmtKind::Local(decls) => format!("local {}", format_var_decls(decls)),
        StmtKind::State(decls) => format!("state {}", format_var_decls(decls)),
        StmtKind::LocalExpr {
            target,
            initializer,
        } => {
            let mut s = format!("local {}", format_expr(target));
            if let Some(init) = initializer {
                s.push_str(&format!(" = {}", format_expr(init)));
            }
            s
        }
        StmtKind::MySync(decls) => format!("mysync {}", format_var_decls(decls)),
        StmtKind::StmtGroup(b) => format_block_indent(b, depth),
        StmtKind::Block(b) => format!("{{\n{}\n{}}}", format_block_indent(b, depth + 1), prefix),
        StmtKind::Begin(b) => format!(
            "BEGIN {{\n{}\n{}}}",
            format_block_indent(b, depth + 1),
            prefix
        ),
        StmtKind::UnitCheck(b) => format!(
            "UNITCHECK {{\n{}\n{}}}",
            format_block_indent(b, depth + 1),
            prefix
        ),
        StmtKind::Check(b) => format!(
            "CHECK {{\n{}\n{}}}",
            format_block_indent(b, depth + 1),
            prefix
        ),
        StmtKind::Init(b) => format!(
            "INIT {{\n{}\n{}}}",
            format_block_indent(b, depth + 1),
            prefix
        ),
        StmtKind::End(b) => format!(
            "END {{\n{}\n{}}}",
            format_block_indent(b, depth + 1),
            prefix
        ),
        StmtKind::Empty => String::new(),
        StmtKind::Goto { target } => format!("goto {}", format_expr(target)),
        StmtKind::Continue(b) => format!(
            "continue {{\n{}\n{}}}",
            format_block_indent(b, depth + 1),
            prefix
        ),
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
            let mut header = format!("{}class {}", prefix, def.name);
            if !def.extends.is_empty() {
                header.push_str(&format!(" extends {}", def.extends.join(", ")));
            }
            if !def.implements.is_empty() {
                header.push_str(&format!(" impl {}", def.implements.join(", ")));
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
            format!("{} {{ {} }}", header, fields)
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
                format_expr(timeout),
                format_block_indent(body, depth + 1),
                prefix
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
                        " finally {{\n{}\n{}}}",
                        format_block_indent(b, depth + 1),
                        prefix
                    )
                })
                .unwrap_or_default();
            format!(
                "try {{\n{}\n{}}} catch (${}) {{\n{}\n{}}}{}",
                format_block_indent(try_block, depth + 1),
                prefix,
                catch_var,
                format_block_indent(catch_block, depth + 1),
                prefix,
                fin
            )
        }
        StmtKind::Given { topic, body } => {
            format!(
                "given ({}) {{\n{}\n{}}}",
                format_expr(topic),
                format_block_indent(body, depth + 1),
                prefix
            )
        }
        StmtKind::When { cond, body } => {
            format!(
                "when ({}) {{\n{}\n{}}}",
                format_expr(cond),
                format_block_indent(body, depth + 1),
                prefix
            )
        }
        StmtKind::DefaultCase { body } => format!(
            "default {{\n{}\n{}}}",
            format_block_indent(body, depth + 1),
            prefix
        ),
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
            let mut s = format!("tie {} {}", target_s, format_expr(class));
            for a in args {
                s.push_str(&format!(", {}", format_expr(a)));
            }
            s
        }
    };
    format!("{}{}{}", prefix, lab, body)
}

pub fn format_block(b: &Block) -> String {
    format_block_indent(b, 0)
}

fn format_block_indent(b: &Block, depth: usize) -> String {
    b.iter()
        .map(|s| format_statement_indent(s, depth))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format a block as a single line for inline use (short blocks in expressions).
fn format_block_inline(b: &Block) -> String {
    b.iter()
        .map(|s| format_statement_indent(s, 0))
        .collect::<Vec<_>>()
        .join("; ")
}

fn format_var_decls(decls: &[VarDecl]) -> String {
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
                s.push_str(&format!(" = {}", format_expr(init)));
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn format_expr_list(es: &[Expr]) -> String {
    es.iter().map(format_expr).collect::<Vec<_>>().join(", ")
}

pub(crate) fn format_binop(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Concat => ".",
        BinOp::NumEq => "==",
        BinOp::NumNe => "!=",
        BinOp::NumLt => "<",
        BinOp::NumGt => ">",
        BinOp::NumLe => "<=",
        BinOp::NumGe => ">=",
        BinOp::Spaceship => "<=>",
        BinOp::StrEq => "eq",
        BinOp::StrNe => "ne",
        BinOp::StrLt => "lt",
        BinOp::StrGt => "gt",
        BinOp::StrLe => "le",
        BinOp::StrGe => "ge",
        BinOp::StrCmp => "cmp",
        BinOp::LogAnd => "&&",
        BinOp::LogOr => "||",
        BinOp::DefinedOr => "//",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::ShiftLeft => "<<",
        BinOp::ShiftRight => ">>",
        BinOp::LogAndWord => "and",
        BinOp::LogOrWord => "or",
        BinOp::BindMatch => "=~",
        BinOp::BindNotMatch => "!~",
    }
}

pub(crate) fn format_unary(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Negate => "-",
        UnaryOp::LogNot => "!",
        UnaryOp::BitNot => "~",
        UnaryOp::LogNotWord => "not",
        UnaryOp::PreIncrement => "++",
        UnaryOp::PreDecrement => "--",
        UnaryOp::Ref => "\\",
    }
}

pub(crate) fn format_postfix(op: PostfixOp) -> &'static str {
    match op {
        PostfixOp::Increment => "++",
        PostfixOp::Decrement => "--",
    }
}

pub(crate) fn format_string_part(p: &StringPart) -> String {
    match p {
        StringPart::Literal(s) => escape_interpolated_literal(s),
        StringPart::ScalarVar(n) => format!("${{{}}}", n),
        StringPart::ArrayVar(n) => format!("@{{{}}}", n),
        StringPart::Expr(e) => format_expr(e),
    }
}

/// Escape special characters inside the literal portions of an interpolated string.
pub(crate) fn escape_interpolated_literal(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x1b' => out.push_str("\\e"),
            c if c.is_control() => {
                out.push_str(&format!("\\x{{{:02x}}}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

/// Escape control characters in regex pattern/replacement strings.
/// Does not escape `/` since that's handled by the delimiter.
pub(crate) fn escape_regex_part(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x1b' => out.push_str("\\x1b"),
            c if c.is_control() => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn format_string_literal(s: &str) -> String {
    let mut out = String::new();
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x1b' => out.push_str("\\e"),
            c if c.is_control() => {
                out.push_str(&format!("\\x{{{:02x}}}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Format an expression; aims for readable Perl-like output.
pub fn format_expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Integer(n) => n.to_string(),
        ExprKind::Float(f) => format!("{}", f),
        ExprKind::String(s) => format_string_literal(s),
        ExprKind::Bareword(s) => s.clone(),
        ExprKind::Regex(p, fl) => format!("/{}/{}/", p, fl),
        ExprKind::QW(ws) => format!("qw({})", ws.join(" ")),
        ExprKind::Undef => "undef".to_string(),
        ExprKind::MagicConst(crate::ast::MagicConstKind::File) => "__FILE__".to_string(),
        ExprKind::MagicConst(crate::ast::MagicConstKind::Line) => "__LINE__".to_string(),
        ExprKind::MagicConst(crate::ast::MagicConstKind::Sub) => "__SUB__".to_string(),
        ExprKind::InterpolatedString(parts) => {
            format!(
                "\"{}\"",
                parts.iter().map(format_string_part).collect::<String>()
            )
        }
        ExprKind::ScalarVar(name) => format!("${}", name),
        ExprKind::ArrayVar(name) => format!("@{}", name),
        ExprKind::HashVar(name) => format!("%{}", name),
        ExprKind::Typeglob(name) => format!("*{}", name),
        ExprKind::TypeglobExpr(e) => format!("*{{ {} }}", format_expr(e)),
        ExprKind::ArrayElement { array, index } => format!("${}[{}]", array, format_expr(index)),
        ExprKind::HashElement { hash, key } => format!("${}{{{}}}", hash, format_expr(key)),
        ExprKind::ArraySlice { array, indices } => format!(
            "@{}[{}]",
            array,
            indices
                .iter()
                .map(format_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::HashSlice { hash, keys } => format!(
            "@{}{{{}}}",
            hash,
            keys.iter().map(format_expr).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::HashSliceDeref { container, keys } => format!(
            "@{}{{{}}}",
            format_expr(container),
            keys.iter().map(format_expr).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::AnonymousListSlice { source, indices } => format!(
            "({})[{}]",
            format_expr(source),
            indices
                .iter()
                .map(format_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::ScalarRef(inner) => format!("\\{}", format_expr(inner)),
        ExprKind::ArrayRef(elems) => format!("[{}]", format_expr_list(elems)),
        ExprKind::HashRef(pairs) => {
            let inner = pairs
                .iter()
                .map(|(k, v)| format!("{} => {}", format_expr(k), format_expr(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{}}}", inner)
        }
        ExprKind::CodeRef { params, body } => {
            if params.is_empty() {
                format!("sub {{ {} }}", format_block_inline(body))
            } else {
                let sig = params
                    .iter()
                    .map(format_sub_sig_param)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("sub ({}) {{ {} }}", sig, format_block_inline(body))
            }
        }
        ExprKind::SubroutineRef(name) => format!("&{}", name),
        ExprKind::SubroutineCodeRef(name) => format!("\\&{}", name),
        ExprKind::DynamicSubCodeRef(e) => format!("\\&{{ {} }}", format_expr(e)),
        ExprKind::Deref { expr, kind } => match kind {
            Sigil::Scalar => format!("${{{}}}", format_expr(expr)),
            Sigil::Array => format!("@{{${}}}", format_expr(expr)),
            Sigil::Hash => format!("%{{${}}}", format_expr(expr)),
            Sigil::Typeglob => format!("*{{${}}}", format_expr(expr)),
        },
        ExprKind::ArrowDeref { expr, index, kind } => match kind {
            DerefKind::Array => format!("({})->[{}]", format_expr(expr), format_expr(index)),
            DerefKind::Hash => format!("({})->{{{}}}", format_expr(expr), format_expr(index)),
            DerefKind::Call => format!("({})->({})", format_expr(expr), format_expr(index)),
        },
        ExprKind::BinOp { left, op, right } => format!(
            "{} {} {}",
            format_expr(left),
            format_binop(*op),
            format_expr(right)
        ),
        ExprKind::UnaryOp { op, expr } => format!("{}{}", format_unary(*op), format_expr(expr)),
        ExprKind::PostfixOp { expr, op } => {
            format!("{}{}", format_expr(expr), format_postfix(*op))
        }
        ExprKind::Assign { target, value } => {
            format!("{} = {}", format_expr(target), format_expr(value))
        }
        ExprKind::CompoundAssign { target, op, value } => format!(
            "{} {}= {}",
            format_expr(target),
            format_binop(*op),
            format_expr(value)
        ),
        ExprKind::Ternary {
            condition,
            then_expr,
            else_expr,
        } => format!(
            "{} ? {} : {}",
            format_expr(condition),
            format_expr(then_expr),
            format_expr(else_expr)
        ),
        ExprKind::Repeat { expr, count } => {
            format!("{} x {}", format_expr(expr), format_expr(count))
        }
        ExprKind::Range {
            from,
            to,
            exclusive,
        } => {
            let op = if *exclusive { "..." } else { ".." };
            format!("{} {} {}", format_expr(from), op, format_expr(to))
        }
        ExprKind::FuncCall { name, args } => format!(
            "{}({})",
            name,
            args.iter().map(format_expr).collect::<Vec<_>>().join(", ")
        ),
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
                format_expr(object),
                m,
                args.iter().map(format_expr).collect::<Vec<_>>().join(", ")
            )
        }
        ExprKind::IndirectCall {
            target,
            args,
            ampersand,
            pass_caller_arglist,
        } => {
            if *pass_caller_arglist && args.is_empty() {
                format!("&{}", format_expr(target))
            } else {
                let inner = format!(
                    "{}({})",
                    format_expr(target),
                    args.iter().map(format_expr).collect::<Vec<_>>().join(", ")
                );
                if *ampersand {
                    format!("&{}", inner)
                } else {
                    inner
                }
            }
        }
        ExprKind::Print { handle, args } => {
            let h = handle
                .as_ref()
                .map(|h| format!("{} ", h))
                .unwrap_or_default();
            format!("print {}{}", h, format_expr_list(args))
        }
        ExprKind::Say { handle, args } => {
            let h = handle
                .as_ref()
                .map(|h| format!("{} ", h))
                .unwrap_or_default();
            format!("say {}{}", h, format_expr_list(args))
        }
        ExprKind::Printf { handle, args } => {
            let h = handle
                .as_ref()
                .map(|h| format!("{} ", h))
                .unwrap_or_default();
            format!("printf {}{}", h, format_expr_list(args))
        }
        ExprKind::Die(args) => {
            if args.is_empty() {
                "die".to_string()
            } else {
                format!("die {}", format_expr_list(args))
            }
        }
        ExprKind::Warn(args) => {
            if args.is_empty() {
                "warn".to_string()
            } else {
                format!("warn {}", format_expr_list(args))
            }
        }
        ExprKind::Match {
            expr,
            pattern,
            flags,
            scalar_g: _,
            delim: _,
        } => format!("{} =~ /{}/{}", format_expr(expr), pattern, flags),
        ExprKind::Substitution {
            expr,
            pattern,
            replacement,
            flags,
            delim: _,
        } => format!(
            "{} =~ s/{}/{}/{}",
            format_expr(expr),
            pattern,
            replacement,
            flags
        ),
        ExprKind::Transliterate {
            expr,
            from,
            to,
            flags,
            delim: _,
        } => format!("{} =~ tr/{}/{}/{}", format_expr(expr), from, to, flags),
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
                "{kw} {{ {} }} {}",
                format_block_inline(block),
                format_expr(list)
            )
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
            format!("{kw} {}, {}", format_expr(expr), format_expr(list))
        }
        ExprKind::GrepExpr {
            block,
            list,
            keyword,
        } => {
            format!(
                "{} {{ {} }} {}",
                keyword.as_str(),
                format_block_inline(block),
                format_expr(list)
            )
        }
        ExprKind::GrepExprComma {
            expr,
            list,
            keyword,
        } => {
            format!(
                "{} {}, {}",
                keyword.as_str(),
                format_expr(expr),
                format_expr(list)
            )
        }
        ExprKind::ForEachExpr { block, list } => {
            format!(
                "fore {{ {} }} {}",
                format_block_inline(block),
                format_expr(list)
            )
        }
        ExprKind::SortExpr { cmp, list } => match cmp {
            Some(crate::ast::SortComparator::Block(b)) => {
                format!(
                    "sort {{ {} }} {}",
                    format_block_inline(b),
                    format_expr(list)
                )
            }
            Some(crate::ast::SortComparator::Code(e)) => {
                format!("sort {} {}", format_expr(e), format_expr(list))
            }
            None => format!("sort {}", format_expr(list)),
        },
        ExprKind::ReverseExpr(e) => format!("reverse {}", format_expr(e)),
        ExprKind::ScalarReverse(e) => format!("rev {}", format_expr(e)),
        ExprKind::JoinExpr { separator, list } => {
            format!("join({}, {})", format_expr(separator), format_expr(list))
        }
        ExprKind::SplitExpr {
            pattern,
            string,
            limit,
        } => match limit {
            Some(l) => format!(
                "split({}, {}, {})",
                format_expr(pattern),
                format_expr(string),
                format_expr(l)
            ),
            None => format!("split({}, {})", format_expr(pattern), format_expr(string)),
        },
        ExprKind::PMapExpr {
            block,
            list,
            progress,
            flat_outputs,
            on_cluster,
            stream: _,
        } => {
            let kw = match (flat_outputs, on_cluster.is_some()) {
                (true, true) => "pflat_map_on",
                (true, false) => "pflat_map",
                (false, true) => "pmap_on",
                (false, false) => "pmap",
            };
            let base = if let Some(c) = on_cluster {
                format!(
                    "{kw} {} {{ {} }} {}",
                    format_expr(c),
                    format_block_inline(block),
                    format_expr(list)
                )
            } else {
                format!(
                    "{kw} {{ {} }} {}",
                    format_block_inline(block),
                    format_expr(list)
                )
            };
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::PMapChunkedExpr {
            chunk_size,
            block,
            list,
            progress,
        } => {
            let base = format!(
                "pmap_chunked {} {{ {} }} {}",
                format_expr(chunk_size),
                format_block_inline(block),
                format_expr(list)
            );
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::PGrepExpr {
            block,
            list,
            progress,
            stream: _,
        } => {
            let base = format!(
                "pgrep {{ {} }} {}",
                format_block_inline(block),
                format_expr(list)
            );
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::PForExpr {
            block,
            list,
            progress,
        } => {
            let base = format!(
                "pfor {{ {} }} {}",
                format_block_inline(block),
                format_expr(list)
            );
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::ParLinesExpr {
            path,
            callback,
            progress,
        } => match progress {
            Some(p) => format!(
                "par_lines({}, {}, progress => {})",
                format_expr(path),
                format_expr(callback),
                format_expr(p)
            ),
            None => format!(
                "par_lines({}, {})",
                format_expr(path),
                format_expr(callback)
            ),
        },
        ExprKind::ParWalkExpr {
            path,
            callback,
            progress,
        } => match progress {
            Some(p) => format!(
                "par_walk({}, {}, progress => {})",
                format_expr(path),
                format_expr(callback),
                format_expr(p)
            ),
            None => format!("par_walk({}, {})", format_expr(path), format_expr(callback)),
        },
        ExprKind::PwatchExpr { path, callback } => {
            format!("pwatch({}, {})", format_expr(path), format_expr(callback))
        }
        ExprKind::PSortExpr {
            cmp,
            list,
            progress,
        } => {
            let base = match cmp {
                Some(b) => format!(
                    "psort {{ {} }} {}",
                    format_block_inline(b),
                    format_expr(list)
                ),
                None => format!("psort {}", format_expr(list)),
            };
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::ReduceExpr { block, list } => format!(
            "reduce {{ {} }} {}",
            format_block_inline(block),
            format_expr(list)
        ),
        ExprKind::PReduceExpr {
            block,
            list,
            progress,
        } => {
            let base = format!(
                "preduce {{ {} }} {}",
                format_block_inline(block),
                format_expr(list)
            );
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::PReduceInitExpr {
            init,
            block,
            list,
            progress,
        } => {
            let base = format!(
                "preduce_init {}, {{ {} }} {}",
                format_expr(init),
                format_block_inline(block),
                format_expr(list)
            );
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::PMapReduceExpr {
            map_block,
            reduce_block,
            list,
            progress,
        } => {
            let base = format!(
                "pmap_reduce {{ {} }} {{ {} }} {}",
                format_block_inline(map_block),
                format_block_inline(reduce_block),
                format_expr(list)
            );
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::PcacheExpr {
            block,
            list,
            progress,
        } => {
            let base = format!(
                "pcache {{ {} }} {}",
                format_block_inline(block),
                format_expr(list)
            );
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::PselectExpr { receivers, timeout } => {
            let inner = receivers
                .iter()
                .map(format_expr)
                .collect::<Vec<_>>()
                .join(", ");
            match timeout {
                Some(t) => format!("pselect({}, timeout => {})", inner, format_expr(t)),
                None => format!("pselect({})", inner),
            }
        }
        ExprKind::FanExpr {
            count,
            block,
            progress,
            capture,
        } => {
            let kw = if *capture { "fan_cap" } else { "fan" };
            let base = match count {
                Some(c) => format!(
                    "{} {} {{ {} }}",
                    kw,
                    format_expr(c),
                    format_block_inline(block)
                ),
                None => format!("{} {{ {} }}", kw, format_block_inline(block)),
            };
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::AsyncBlock { body } => format!("async {{ {} }}", format_block_inline(body)),
        ExprKind::SpawnBlock { body } => format!("spawn {{ {} }}", format_block_inline(body)),
        ExprKind::Trace { body } => format!("trace {{ {} }}", format_block_inline(body)),
        ExprKind::Timer { body } => format!("timer {{ {} }}", format_block_inline(body)),
        ExprKind::Bench { body, times } => format!(
            "bench {{ {} }} {}",
            format_block_inline(body),
            format_expr(times)
        ),
        ExprKind::Await(e) => format!("await {}", format_expr(e)),
        ExprKind::Slurp(e) => format!("slurp {}", format_expr(e)),
        ExprKind::Capture(e) => format!("capture {}", format_expr(e)),
        ExprKind::Qx(e) => format!("qx {}", format_expr(e)),
        ExprKind::FetchUrl(e) => format!("fetch_url {}", format_expr(e)),
        ExprKind::Pchannel { capacity } => match capacity {
            Some(c) => format!("pchannel({})", format_expr(c)),
            None => "pchannel()".to_string(),
        },
        ExprKind::Push { array, values } => {
            format!("push({}, {})", format_expr(array), format_expr_list(values))
        }
        ExprKind::Pop(e) => format!("pop {}", format_expr(e)),
        ExprKind::Shift(e) => format!("shift {}", format_expr(e)),
        ExprKind::Unshift { array, values } => format!(
            "unshift({}, {})",
            format_expr(array),
            format_expr_list(values)
        ),
        ExprKind::Splice {
            array,
            offset,
            length,
            replacement,
        } => {
            let mut parts = vec![format_expr(array)];
            if let Some(o) = offset {
                parts.push(format_expr(o));
            }
            if let Some(l) = length {
                parts.push(format_expr(l));
            }
            if !replacement.is_empty() {
                parts.push(format_expr_list(replacement));
            }
            format!("splice({})", parts.join(", "))
        }
        ExprKind::Delete(e) => format!("delete {}", format_expr(e)),
        ExprKind::Exists(e) => format!("exists {}", format_expr(e)),
        ExprKind::Keys(e) => format!("keys {}", format_expr(e)),
        ExprKind::Values(e) => format!("values {}", format_expr(e)),
        ExprKind::Each(e) => format!("each {}", format_expr(e)),
        ExprKind::Chomp(e) => format!("chomp {}", format_expr(e)),
        ExprKind::Chop(e) => format!("chop {}", format_expr(e)),
        ExprKind::Length(e) => format!("length {}", format_expr(e)),
        ExprKind::Substr {
            string,
            offset,
            length,
            replacement,
        } => {
            let mut parts = vec![format_expr(string), format_expr(offset)];
            if let Some(l) = length {
                parts.push(format_expr(l));
            }
            if let Some(r) = replacement {
                parts.push(format_expr(r));
            }
            format!("substr({})", parts.join(", "))
        }
        ExprKind::Index {
            string,
            substr,
            position,
        } => match position {
            Some(p) => format!(
                "index({}, {}, {})",
                format_expr(string),
                format_expr(substr),
                format_expr(p)
            ),
            None => format!("index({}, {})", format_expr(string), format_expr(substr)),
        },
        ExprKind::Rindex {
            string,
            substr,
            position,
        } => match position {
            Some(p) => format!(
                "rindex({}, {}, {})",
                format_expr(string),
                format_expr(substr),
                format_expr(p)
            ),
            None => format!("rindex({}, {})", format_expr(string), format_expr(substr)),
        },
        ExprKind::Sprintf { format, args } => format!(
            "sprintf({}, {})",
            format_expr(format),
            format_expr_list(args)
        ),
        ExprKind::Abs(e) => format!("abs {}", format_expr(e)),
        ExprKind::Int(e) => format!("int {}", format_expr(e)),
        ExprKind::Sqrt(e) => format!("sqrt {}", format_expr(e)),
        ExprKind::Sin(e) => format!("sin {}", format_expr(e)),
        ExprKind::Cos(e) => format!("cos {}", format_expr(e)),
        ExprKind::Atan2 { y, x } => format!("atan2({}, {})", format_expr(y), format_expr(x)),
        ExprKind::Exp(e) => format!("exp {}", format_expr(e)),
        ExprKind::Log(e) => format!("log {}", format_expr(e)),
        ExprKind::Rand(opt) => match opt {
            Some(e) => format!("rand({})", format_expr(e)),
            None => "rand".to_string(),
        },
        ExprKind::Srand(opt) => match opt {
            Some(e) => format!("srand({})", format_expr(e)),
            None => "srand".to_string(),
        },
        ExprKind::Hex(e) => format!("hex {}", format_expr(e)),
        ExprKind::Oct(e) => format!("oct {}", format_expr(e)),
        ExprKind::Lc(e) => format!("lc {}", format_expr(e)),
        ExprKind::Uc(e) => format!("uc {}", format_expr(e)),
        ExprKind::Lcfirst(e) => format!("lcfirst {}", format_expr(e)),
        ExprKind::Ucfirst(e) => format!("ucfirst {}", format_expr(e)),
        ExprKind::Fc(e) => format!("fc {}", format_expr(e)),
        ExprKind::Crypt { plaintext, salt } => {
            format!("crypt({}, {})", format_expr(plaintext), format_expr(salt))
        }
        ExprKind::Pos(opt) => match opt {
            Some(e) => format!("pos({})", format_expr(e)),
            None => "pos".to_string(),
        },
        ExprKind::Study(e) => format!("study {}", format_expr(e)),
        ExprKind::Defined(e) => format!("defined {}", format_expr(e)),
        ExprKind::Ref(e) => format!("ref {}", format_expr(e)),
        ExprKind::ScalarContext(e) => format!("scalar {}", format_expr(e)),
        ExprKind::Chr(e) => format!("chr {}", format_expr(e)),
        ExprKind::Ord(e) => format!("ord {}", format_expr(e)),
        ExprKind::OpenMyHandle { name } => format!("my ${}", name),
        ExprKind::Open { handle, mode, file } => match file {
            Some(f) => format!(
                "open({}, {}, {})",
                format_expr(handle),
                format_expr(mode),
                format_expr(f)
            ),
            None => format!("open({}, {})", format_expr(handle), format_expr(mode)),
        },
        ExprKind::Close(e) => format!("close {}", format_expr(e)),
        ExprKind::ReadLine(handle) => match handle {
            Some(h) => {
                if h.starts_with(|c: char| c.is_uppercase()) {
                    format!("<{}>", h)
                } else {
                    format!("<${}>", h)
                }
            }
            None => "<STDIN>".to_string(),
        },
        ExprKind::Eof(opt) => match opt {
            Some(e) => format!("eof({})", format_expr(e)),
            None => "eof".to_string(),
        },
        ExprKind::Opendir { handle, path } => {
            format!("opendir({}, {})", format_expr(handle), format_expr(path))
        }
        ExprKind::Readdir(e) => format!("readdir {}", format_expr(e)),
        ExprKind::Closedir(e) => format!("closedir {}", format_expr(e)),
        ExprKind::Rewinddir(e) => format!("rewinddir {}", format_expr(e)),
        ExprKind::Telldir(e) => format!("telldir {}", format_expr(e)),
        ExprKind::Seekdir { handle, position } => format!(
            "seekdir({}, {})",
            format_expr(handle),
            format_expr(position)
        ),
        ExprKind::FileTest { op, expr } => format!("-{}{}", op, format_expr(expr)),
        ExprKind::System(args) => format!("system({})", format_expr_list(args)),
        ExprKind::Exec(args) => format!("exec({})", format_expr_list(args)),
        ExprKind::Eval(e) => format!("eval {}", format_expr(e)),
        ExprKind::Do(e) => format!("do {}", format_expr(e)),
        ExprKind::Require(e) => format!("require {}", format_expr(e)),
        ExprKind::Exit(opt) => match opt {
            Some(e) => format!("exit({})", format_expr(e)),
            None => "exit".to_string(),
        },
        ExprKind::Chdir(e) => format!("chdir {}", format_expr(e)),
        ExprKind::Mkdir { path, mode } => match mode {
            Some(m) => format!("mkdir({}, {})", format_expr(path), format_expr(m)),
            None => format!("mkdir({})", format_expr(path)),
        },
        ExprKind::Unlink(args) => format!("unlink({})", format_expr_list(args)),
        ExprKind::Rename { old, new } => {
            format!("rename({}, {})", format_expr(old), format_expr(new))
        }
        ExprKind::Chmod(args) => format!("chmod({})", format_expr_list(args)),
        ExprKind::Chown(args) => format!("chown({})", format_expr_list(args)),
        ExprKind::Stat(e) => format!("stat {}", format_expr(e)),
        ExprKind::Lstat(e) => format!("lstat {}", format_expr(e)),
        ExprKind::Link { old, new } => format!("link({}, {})", format_expr(old), format_expr(new)),
        ExprKind::Symlink { old, new } => {
            format!("symlink({}, {})", format_expr(old), format_expr(new))
        }
        ExprKind::Readlink(e) => format!("readlink {}", format_expr(e)),
        ExprKind::Glob(args) => format!("glob({})", format_expr_list(args)),
        ExprKind::Files(args) => format!("files({})", format_expr_list(args)),
        ExprKind::Filesf(args) => format!("filesf({})", format_expr_list(args)),
        ExprKind::FilesfRecursive(args) => format!("fr({})", format_expr_list(args)),
        ExprKind::Dirs(args) => format!("dirs({})", format_expr_list(args)),
        ExprKind::DirsRecursive(args) => format!("dr({})", format_expr_list(args)),
        ExprKind::SymLinks(args) => format!("sym_links({})", format_expr_list(args)),
        ExprKind::Sockets(args) => format!("sockets({})", format_expr_list(args)),
        ExprKind::Pipes(args) => format!("pipes({})", format_expr_list(args)),
        ExprKind::BlockDevices(args) => format!("block_devices({})", format_expr_list(args)),
        ExprKind::CharDevices(args) => format!("char_devices({})", format_expr_list(args)),
        ExprKind::GlobPar { args, progress } => {
            let base = format!("glob_par({})", format_expr_list(args));
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::ParSed { args, progress } => {
            let base = format!("par_sed({})", format_expr_list(args));
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::Bless { ref_expr, class } => match class {
            Some(c) => format!("bless({}, {})", format_expr(ref_expr), format_expr(c)),
            None => format!("bless({})", format_expr(ref_expr)),
        },
        ExprKind::Caller(opt) => match opt {
            Some(e) => format!("caller({})", format_expr(e)),
            None => "caller".to_string(),
        },
        ExprKind::Wantarray => "wantarray".to_string(),
        ExprKind::List(exprs) => format!("({})", format_expr_list(exprs)),
        ExprKind::PostfixIf { expr, condition } => {
            format!("{} if {}", format_expr(expr), format_expr(condition))
        }
        ExprKind::PostfixUnless { expr, condition } => {
            format!("{} unless {}", format_expr(expr), format_expr(condition))
        }
        ExprKind::PostfixWhile { expr, condition } => {
            format!("{} while {}", format_expr(expr), format_expr(condition))
        }
        ExprKind::PostfixUntil { expr, condition } => {
            format!("{} until {}", format_expr(expr), format_expr(condition))
        }
        ExprKind::PostfixForeach { expr, list } => {
            format!("{} foreach {}", format_expr(expr), format_expr(list))
        }
        ExprKind::AlgebraicMatch { subject, arms } => {
            let arms_s = arms
                .iter()
                .map(|a| {
                    let guard_s = a
                        .guard
                        .as_ref()
                        .map(|g| format!(" if {}", format_expr(g)))
                        .unwrap_or_default();
                    format!(
                        "{}{} => {}",
                        format_match_pattern(&a.pattern),
                        guard_s,
                        format_expr(&a.body)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("match ({}) {{ {} }}", format_expr(subject), arms_s)
        }
        ExprKind::RetryBlock {
            body,
            times,
            backoff,
        } => {
            let bo = match backoff {
                crate::ast::RetryBackoff::None => "none",
                crate::ast::RetryBackoff::Linear => "linear",
                crate::ast::RetryBackoff::Exponential => "exponential",
            };
            format!(
                "retry {{ {} }} times => {}, backoff => {}",
                format_block_inline(body),
                format_expr(times),
                bo
            )
        }
        ExprKind::RateLimitBlock {
            max, window, body, ..
        } => {
            format!(
                "rate_limit({}, {}) {{ {} }}",
                format_expr(max),
                format_expr(window),
                format_block_inline(body)
            )
        }
        ExprKind::EveryBlock { interval, body } => {
            format!(
                "every({}) {{ {} }}",
                format_expr(interval),
                format_block_inline(body)
            )
        }
        ExprKind::GenBlock { body } => {
            format!("gen {{ {} }}", format_block_inline(body))
        }
        ExprKind::Yield(e) => {
            format!("yield {}", format_expr(e))
        }
        ExprKind::Spinner { message, body } => {
            format!(
                "spinner {} {{ {} }}",
                format_expr(message),
                body.iter()
                    .map(format_statement)
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        }
        ExprKind::MyExpr { keyword, decls } => {
            // Render `my $x = …` etc. inline. Single-decl is the common case
            // (e.g. `if (my $x = …)`); list-decl reuses the same formatter.
            let parts: Vec<String> = decls
                .iter()
                .map(|d| {
                    let sigil = match d.sigil {
                        crate::ast::Sigil::Scalar => '$',
                        crate::ast::Sigil::Array => '@',
                        crate::ast::Sigil::Hash => '%',
                        crate::ast::Sigil::Typeglob => '*',
                    };
                    let mut s = format!("{}{}", sigil, d.name);
                    if let Some(init) = &d.initializer {
                        s.push_str(" = ");
                        s.push_str(&format_expr(init));
                    }
                    s
                })
                .collect();
            if parts.len() == 1 {
                format!("{} {}", keyword, parts[0])
            } else {
                format!("{} ({})", keyword, parts.join(", "))
            }
        }
    }
}

pub(crate) fn format_match_pattern(p: &crate::ast::MatchPattern) -> String {
    use crate::ast::{MatchArrayElem, MatchHashPair, MatchPattern};
    match p {
        MatchPattern::Any => "_".to_string(),
        MatchPattern::Regex { pattern, flags } => {
            if flags.is_empty() {
                format!("/{}/", pattern)
            } else {
                format!("/{}/{}/", pattern, flags)
            }
        }
        MatchPattern::Value(e) => format_expr(e),
        MatchPattern::Array(elems) => {
            let inner = elems
                .iter()
                .map(|x| match x {
                    MatchArrayElem::Expr(e) => format_expr(e),
                    MatchArrayElem::CaptureScalar(name) => format!("${}", name),
                    MatchArrayElem::Rest => "*".to_string(),
                    MatchArrayElem::RestBind(name) => format!("@{}", name),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", inner)
        }
        MatchPattern::Hash(pairs) => {
            let inner = pairs
                .iter()
                .map(|pair| match pair {
                    MatchHashPair::KeyOnly { key } => {
                        format!("{} => _", format_expr(key))
                    }
                    MatchHashPair::Capture { key, name } => {
                        format!("{} => ${}", format_expr(key), name)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {} }}", inner)
        }
        MatchPattern::OptionSome(name) => format!("Some({})", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn format_program_expression_statement_includes_binop() {
        let p = parse("2 + 3").expect("parse");
        let out = format_program(&p);
        assert!(
            out.contains("2") && out.contains("3") && out.contains("+"),
            "unexpected format: {out}"
        );
    }

    #[test]
    fn format_program_if_block() {
        let p = parse("if (1) { 2; }").expect("parse");
        let out = format_program(&p);
        assert!(out.contains("if") && out.contains('1'));
    }

    #[test]
    fn format_program_package_line() {
        let p = parse("package Foo::Bar").expect("parse");
        let out = format_program(&p);
        assert!(out.contains("package Foo::Bar"));
    }

    #[test]
    fn format_program_string_literal_escapes_quote() {
        let p = parse(r#"my $s = "a\"b""#).expect("parse");
        let out = format_program(&p);
        assert!(out.contains("\\\""), "expected escaped quote in: {out}");
    }
}
