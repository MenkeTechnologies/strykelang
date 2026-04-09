//! Pretty-print parsed Perl back to source (`pe --fmt`).
//! Regenerate with `python3 tools/gen_fmt.py` after `ast.rs` changes.

#![allow(unused_variables)] // generated `match` arms name fields not always used

use crate::ast::*;

/// Format a whole program as Perl-like source.
pub fn format_program(p: &Program) -> String {
    p.statements
        .iter()
        .map(format_statement)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_statement(s: &Statement) -> String {
    let lab = s
        .label
        .as_ref()
        .map(|l| format!("{}: ", l))
        .unwrap_or_default();
    let body = match &s.kind {
        StmtKind::Expression(e) => format!("{};", format_expr(e)),
        StmtKind::If {
            condition,
            body,
            elsifs,
            else_block,
        } => {
            let mut s = format!(
                "if ({}) {{\n{}\n}}",
                format_expr(condition),
                format_block(body)
            );
            for (c, b) in elsifs {
                s.push_str(&format!(
                    " elsif ({}) {{\n{}\n}}",
                    format_expr(c),
                    format_block(b)
                ));
            }
            if let Some(eb) = else_block {
                s.push_str(&format!(" else {{\n{}\n}}", format_block(eb)));
            }
            s
        }
        StmtKind::Unless {
            condition,
            body,
            else_block,
        } => {
            let mut s = format!(
                "unless ({}) {{\n{}\n}}",
                format_expr(condition),
                format_block(body)
            );
            if let Some(eb) = else_block {
                s.push_str(&format!(" else {{\n{}\n}}", format_block(eb)));
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
                "{}while ({}) {{\n{}\n}}",
                lb,
                format_expr(condition),
                format_block(body)
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(" continue {{\n{}\n}}", format_block(cb)));
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
                "{}until ({}) {{\n{}\n}}",
                lb,
                format_expr(condition),
                format_block(body)
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(" continue {{\n{}\n}}", format_block(cb)));
            }
            s
        }
        StmtKind::DoWhile { body, condition } => {
            format!(
                "do {{\n{}\n}} while ({})",
                format_block(body),
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
                .map(|s| format_statement(s))
                .unwrap_or_default();
            let cond = condition.as_ref().map(format_expr).unwrap_or_default();
            let st = step.as_ref().map(format_expr).unwrap_or_default();
            let mut s = format!(
                "{}for ({}; {}; {}) {{\n{}\n}}",
                lb,
                ini,
                cond,
                st,
                format_block(body)
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(" continue {{\n{}\n}}", format_block(cb)));
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
                "{}foreach ${} ({}) {{\n{}\n}}",
                lb,
                var,
                format_expr(list),
                format_block(body)
            );
            if let Some(cb) = continue_block {
                s.push_str(&format!(" continue {{\n{}\n}}", format_block(cb)));
            }
            s
        }
        StmtKind::SubDecl {
            name,
            params: _params,
            body,
            prototype,
        } => {
            let proto = prototype
                .as_ref()
                .map(|p| format!(" ({})", p))
                .unwrap_or_default();
            format!("sub {}{} {{\n{}\n}}", name, proto, format_block(body))
        }
        StmtKind::Package { name } => format!("package {};", name),
        StmtKind::Use { module, imports } => {
            if imports.is_empty() {
                format!("use {};", module)
            } else {
                format!("use {} {};", module, format_expr_list(imports))
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
            format!("use overload {inner};")
        }
        StmtKind::No { module, imports } => {
            if imports.is_empty() {
                format!("no {};", module)
            } else {
                format!("no {} {};", module, format_expr_list(imports))
            }
        }
        StmtKind::Return(e) => e
            .as_ref()
            .map(|x| format!("return {};", format_expr(x)))
            .unwrap_or_else(|| "return;".to_string()),
        StmtKind::Last(l) => l
            .as_ref()
            .map(|x| format!("last {};", x))
            .unwrap_or_else(|| "last;".to_string()),
        StmtKind::Next(l) => l
            .as_ref()
            .map(|x| format!("next {};", x))
            .unwrap_or_else(|| "next;".to_string()),
        StmtKind::Redo(l) => l
            .as_ref()
            .map(|x| format!("redo {};", x))
            .unwrap_or_else(|| "redo;".to_string()),
        StmtKind::My(decls) => format!("my {};", format_var_decls(decls)),
        StmtKind::Our(decls) => format!("our {};", format_var_decls(decls)),
        StmtKind::Local(decls) => format!("local {};", format_var_decls(decls)),
        StmtKind::MySync(decls) => format!("mysync {};", format_var_decls(decls)),
        StmtKind::Block(b) => format!("{{\n{}\n}}", format_block(b)),
        StmtKind::Begin(b) => format!("BEGIN {{\n{}\n}}", format_block(b)),
        StmtKind::End(b) => format!("END {{\n{}\n}}", format_block(b)),
        StmtKind::Empty => ";".to_string(),
        StmtKind::Goto { target } => format!("goto {};", format_expr(target)),
        StmtKind::Continue(b) => format!("continue {{\n{}\n}}", format_block(b)),
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
                "eval_timeout {} {{\n{}\n}}",
                format_expr(timeout),
                format_block(body)
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
                .map(|b| format!("\nfinally {{\n{}\n}}", format_block(b)))
                .unwrap_or_default();
            format!(
                "try {{\n{}\n}} catch (${}) {{\n{}\n}}{}",
                format_block(try_block),
                catch_var,
                format_block(catch_block),
                fin
            )
        }
        StmtKind::Given { topic, body } => {
            format!(
                "given ({}) {{\n{}\n}}",
                format_expr(topic),
                format_block(body)
            )
        }
        StmtKind::When { cond, body } => {
            format!(
                "when ({}) {{\n{}\n}}",
                format_expr(cond),
                format_block(body)
            )
        }
        StmtKind::DefaultCase { body } => format!("default {{\n{}\n}}", format_block(body)),
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
            s.push(';');
            s
        }
    };
    format!("{}{}", lab, body)
}

fn format_block(b: &Block) -> String {
    b.iter()
        .map(format_statement)
        .collect::<Vec<_>>()
        .join("\n")
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
            if let Some(t) = d.type_annotation {
                s.push_str(&format!(" : {:?}", t));
            }
            if let Some(ref init) = d.initializer {
                s.push_str(&format!(" = {}", format_expr(init)));
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_expr_list(es: &[Expr]) -> String {
    es.iter().map(format_expr).collect::<Vec<_>>().join(", ")
}

fn format_binop(op: BinOp) -> &'static str {
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

fn format_unary(op: UnaryOp) -> &'static str {
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

fn format_postfix(op: PostfixOp) -> &'static str {
    match op {
        PostfixOp::Increment => "++",
        PostfixOp::Decrement => "--",
    }
}

fn format_string_part(p: &StringPart) -> String {
    match p {
        StringPart::Literal(s) => s.clone(),
        StringPart::ScalarVar(n) => format!("${{{}}}", n),
        StringPart::ArrayVar(n) => format!("@{{{}}}", n),
        StringPart::Expr(e) => format_expr(e),
    }
}

fn format_string_literal(s: &str) -> String {
    let mut out = String::new();
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
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
        ExprKind::Regex(p, fl) => format!("/{}/{}/", p, fl),
        ExprKind::QW(ws) => format!("qw({})", ws.join(" ")),
        ExprKind::Undef => "undef".to_string(),
        ExprKind::MagicConst(crate::ast::MagicConstKind::File) => "__FILE__".to_string(),
        ExprKind::MagicConst(crate::ast::MagicConstKind::Line) => "__LINE__".to_string(),
        ExprKind::InterpolatedString(parts) => {
            parts.iter().map(format_string_part).collect::<String>()
        }
        ExprKind::ScalarVar(name) => format!("${}", name),
        ExprKind::ArrayVar(name) => format!("@{}", name),
        ExprKind::HashVar(name) => format!("%{}", name),
        ExprKind::Typeglob(name) => format!("*{}", name),
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
        ExprKind::ScalarRef(_) => "/* ExprKind::ScalarRef */".to_string(),
        ExprKind::ArrayRef(_) => "/* ExprKind::ArrayRef */".to_string(),
        ExprKind::HashRef(_) => "/* ExprKind::HashRef */".to_string(),
        ExprKind::CodeRef { params, body } => format!("sub {{\n{}\n}}", format_block(body)),
        ExprKind::SubroutineRef(name) => format!("&{}", name),
        ExprKind::SubroutineCodeRef(name) => format!("\\&{}", name),
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
            "({} {} {})",
            format_expr(left),
            format_binop(*op),
            format_expr(right)
        ),
        ExprKind::UnaryOp { op, expr } => format!("({}{})", format_unary(*op), format_expr(expr)),
        ExprKind::PostfixOp { expr, op } => {
            format!("({}{})", format_expr(expr), format_postfix(*op))
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
            "({} ? {} : {})",
            format_expr(condition),
            format_expr(then_expr),
            format_expr(else_expr)
        ),
        ExprKind::Repeat { expr, count } => {
            format!("({} x {})", format_expr(expr), format_expr(count))
        }
        ExprKind::Range { from, to } => format!("({} .. {})", format_expr(from), format_expr(to)),
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
        ExprKind::Print { handle, args } => {
            let mut s = String::new();
            if let Some(h) = handle {
                s.push_str(h);
                s.push_str(": ");
            }
            s.push_str("print ");
            s.push_str(&format_expr_list(args));
            s
        }
        ExprKind::Say { handle, args } => {
            let mut s = String::new();
            if let Some(h) = handle {
                s.push_str(h);
                s.push_str(": ");
            }
            s.push_str("say ");
            s.push_str(&format_expr_list(args));
            s
        }
        ExprKind::Printf { handle, args } => {
            let mut s = String::new();
            if let Some(h) = handle {
                s.push_str(h);
                s.push_str(": ");
            }
            s.push_str("printf ");
            s.push_str(&format_expr_list(args));
            s
        }
        ExprKind::Die(_) => "/* ExprKind::Die */".to_string(),
        ExprKind::Warn(_) => "/* ExprKind::Warn */".to_string(),
        ExprKind::Match {
            expr,
            pattern,
            flags,
            scalar_g,
        } => format!("({} =~ /{}/{})", format_expr(expr), pattern, flags),
        ExprKind::Substitution {
            expr,
            pattern,
            replacement,
            flags,
        } => format!(
            "({} =~ s/{}/{}/{})",
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
        } => format!("({} =~ tr/{}/{}/{})", format_expr(expr), from, to, flags),
        ExprKind::MapExpr { block, list } => {
            format!("map {{\n{}\n}} {}", format_block(block), format_expr(list))
        }
        ExprKind::GrepExpr { block, list } => {
            format!("grep {{\n{}\n}} {}", format_block(block), format_expr(list))
        }
        ExprKind::SortExpr { cmp, list } => match cmp {
            Some(b) => format!("sort {{\n{}\n}} {}", format_block(b), format_expr(list)),
            None => format!("sort {}", format_expr(list)),
        },
        ExprKind::ReverseExpr(e) => format!("reverse {}", format_expr(e)),
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
        } => {
            let base = format!("pmap {{\n{}\n}} {}", format_block(block), format_expr(list));
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
                "pmap_chunked {} {{\n{}\n}} {}",
                format_expr(chunk_size),
                format_block(block),
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
        } => {
            let base = format!(
                "pgrep {{\n{}\n}} {}",
                format_block(block),
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
            let base = format!("pfor {{\n{}\n}} {}", format_block(block), format_expr(list));
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
        ExprKind::PwatchExpr { path, callback } => {
            format!("pwatch({}, {})", format_expr(path), format_expr(callback))
        }
        ExprKind::PSortExpr {
            cmp,
            list,
            progress,
        } => {
            let base = match cmp {
                Some(b) => format!("psort {{\n{}\n}} {}", format_block(b), format_expr(list)),
                None => format!("psort {}", format_expr(list)),
            };
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::ReduceExpr { block, list } => format!(
            "reduce {{\n{}\n}} {}",
            format_block(block),
            format_expr(list)
        ),
        ExprKind::PReduceExpr {
            block,
            list,
            progress,
        } => {
            let base = format!(
                "preduce {{\n{}\n}} {}",
                format_block(block),
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
                "preduce_init {}, {{\n{}\n}} {}",
                format_expr(init),
                format_block(block),
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
                "pmap_reduce {{\n{}\n}} {{\n{}\n}} {}",
                format_block(map_block),
                format_block(reduce_block),
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
                "pcache {{\n{}\n}} {}",
                format_block(block),
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
        } => {
            let base = match count {
                Some(c) => format!("fan {} {{\n{}\n}}", format_expr(c), format_block(block)),
                None => format!("fan {{\n{}\n}}", format_block(block)),
            };
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }
        ExprKind::AsyncBlock { body } => format!("async {{\n{}\n}}", format_block(body)),
        ExprKind::Trace { body } => format!("trace {{\n{}\n}}", format_block(body)),
        ExprKind::Timer { body } => format!("timer {{\n{}\n}}", format_block(body)),
        ExprKind::Await(e) => format!("await {}", format_expr(e)),
        ExprKind::Slurp(e) => format!("slurp {}", format_expr(e)),
        ExprKind::Capture(e) => format!("capture {}", format_expr(e)),
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
        } => format!("splice({}, ...)", format_expr(array)),
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
        } => format!("substr({}, ...)", format_expr(string)),
        ExprKind::Index {
            string,
            substr,
            position,
        } => format!("index({}, {})", format_expr(string), format_expr(substr)),
        ExprKind::Rindex {
            string,
            substr,
            position,
        } => format!("rindex({}, {})", format_expr(string), format_expr(substr)),
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
        ExprKind::Rand(_) => "/* ExprKind::Rand */".to_string(),
        ExprKind::Srand(_) => "/* ExprKind::Srand */".to_string(),
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
        ExprKind::Pos(_) => "/* ExprKind::Pos */".to_string(),
        ExprKind::Study(e) => format!("study {}", format_expr(e)),
        ExprKind::Defined(e) => format!("defined {}", format_expr(e)),
        ExprKind::Ref(e) => format!("ref {}", format_expr(e)),
        ExprKind::ScalarContext(e) => format!("scalar {}", format_expr(e)),
        ExprKind::Chr(e) => format!("chr {}", format_expr(e)),
        ExprKind::Ord(e) => format!("ord {}", format_expr(e)),
        ExprKind::Open { handle, mode, file } => {
            format!("open({}, {}, ...)", format_expr(handle), format_expr(mode))
        }
        ExprKind::Close(e) => format!("close {}", format_expr(e)),
        ExprKind::ReadLine(_) => "/* ExprKind::ReadLine */".to_string(),
        ExprKind::Eof(_) => "/* ExprKind::Eof */".to_string(),
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
        ExprKind::System(_) => "/* ExprKind::System */".to_string(),
        ExprKind::Exec(_) => "/* ExprKind::Exec */".to_string(),
        ExprKind::Eval(_) => "/* ExprKind::Eval */".to_string(),
        ExprKind::Do(_) => "/* ExprKind::Do */".to_string(),
        ExprKind::Require(_) => "/* ExprKind::Require */".to_string(),
        ExprKind::Exit(_) => "/* ExprKind::Exit */".to_string(),
        ExprKind::Chdir(_) => "/* ExprKind::Chdir */".to_string(),
        ExprKind::Mkdir { path, mode } => format!("mkdir({}, ...)", format_expr(path)),
        ExprKind::Unlink(_) => "/* ExprKind::Unlink */".to_string(),
        ExprKind::Rename { old, new } => {
            format!("rename({}, {})", format_expr(old), format_expr(new))
        }
        ExprKind::Chmod(_) => "/* ExprKind::Chmod */".to_string(),
        ExprKind::Chown(_) => "/* ExprKind::Chown */".to_string(),
        ExprKind::Stat(e) => format!("stat {}", format_expr(e)),
        ExprKind::Lstat(e) => format!("lstat {}", format_expr(e)),
        ExprKind::Link { old, new } => format!("link({}, {})", format_expr(old), format_expr(new)),
        ExprKind::Symlink { old, new } => {
            format!("symlink({}, {})", format_expr(old), format_expr(new))
        }
        ExprKind::Readlink(e) => format!("readlink {}", format_expr(e)),
        ExprKind::Glob(_) => "/* ExprKind::Glob */".to_string(),
        ExprKind::GlobPar(_) => "/* ExprKind::GlobPar */".to_string(),
        ExprKind::Bless { ref_expr, class } => match class {
            Some(c) => format!("bless({}, {})", format_expr(ref_expr), format_expr(c)),
            None => format!("bless({})", format_expr(ref_expr)),
        },
        ExprKind::Caller(_) => "/* ExprKind::Caller */".to_string(),
        ExprKind::Wantarray => "wantarray".to_string(),
        ExprKind::List(_) => "/* ExprKind::List */".to_string(),
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
                    format!(
                        "{} => {}",
                        format_match_pattern(&a.pattern),
                        format_expr(&a.body)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("match ({}) {{ {} }}", format_expr(subject), arms_s)
        }
    }
}

fn format_match_pattern(p: &crate::ast::MatchPattern) -> String {
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
                    MatchArrayElem::Rest => "*".to_string(),
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
    }
}
