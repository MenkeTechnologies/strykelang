//! AST-to-source deparser for serializing code refs.
//!
//! Converts [`Expr`], [`Statement`], and [`Block`] back to valid stryke source code.

use crate::ast::*;
use std::cell::RefCell;
use std::fmt::Write;

thread_local! {
    static OUTPUT_DELIM: RefCell<Option<char>> = const { RefCell::new(None) };
}

fn get_output_delim() -> Option<char> {
    OUTPUT_DELIM.with(|d| *d.borrow())
}

fn set_output_delim(delim: Option<char>) {
    OUTPUT_DELIM.with(|d| *d.borrow_mut() = delim);
}

/// Choose the output delimiter: custom if set, else default `/`.
fn choose_delim() -> char {
    get_output_delim().unwrap_or('/')
}

pub fn deparse_block(block: &Block) -> String {
    let mut buf = String::new();
    deparse_block_into(&mut buf, block, 0);
    buf
}

/// Deparse a block with a custom delimiter for regex operations.
pub fn deparse_block_with_delim(block: &Block, delim: char) -> String {
    set_output_delim(Some(delim));
    let mut buf = String::new();
    deparse_block_into(&mut buf, block, 0);
    set_output_delim(None);
    buf
}

pub fn deparse_expr(expr: &Expr) -> String {
    let mut buf = String::new();
    deparse_expr_into(&mut buf, expr);
    buf
}

fn deparse_block_into(buf: &mut String, block: &Block, indent: usize) {
    for (i, stmt) in block.iter().enumerate() {
        if i > 0 {
            buf.push('\n');
        }
        deparse_stmt_into(buf, stmt, indent);
    }
}

fn indent_str(indent: usize) -> String {
    "    ".repeat(indent)
}

fn deparse_stmt_into(buf: &mut String, stmt: &Statement, indent: usize) {
    let ind = indent_str(indent);

    if let Some(label) = &stmt.label {
        let _ = write!(buf, "{}{}: ", ind, label);
    } else {
        buf.push_str(&ind);
    }

    match &stmt.kind {
        StmtKind::Expression(expr) => {
            deparse_expr_into(buf, expr);
            buf.push(';');
        }
        StmtKind::If {
            condition,
            body,
            elsifs,
            else_block,
        } => {
            buf.push_str("if (");
            deparse_expr_into(buf, condition);
            buf.push_str(") {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
            for (cond, blk) in elsifs {
                buf.push_str(" elsif (");
                deparse_expr_into(buf, cond);
                buf.push_str(") {\n");
                deparse_block_into(buf, blk, indent + 1);
                buf.push('\n');
                buf.push_str(&ind);
                buf.push('}');
            }
            if let Some(else_blk) = else_block {
                buf.push_str(" else {\n");
                deparse_block_into(buf, else_blk, indent + 1);
                buf.push('\n');
                buf.push_str(&ind);
                buf.push('}');
            }
        }
        StmtKind::Unless {
            condition,
            body,
            else_block,
        } => {
            buf.push_str("unless (");
            deparse_expr_into(buf, condition);
            buf.push_str(") {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
            if let Some(else_blk) = else_block {
                buf.push_str(" else {\n");
                deparse_block_into(buf, else_blk, indent + 1);
                buf.push('\n');
                buf.push_str(&ind);
                buf.push('}');
            }
        }
        StmtKind::While {
            condition,
            body,
            label,
            continue_block,
        } => {
            if let Some(lbl) = label {
                let _ = write!(buf, "{}: ", lbl);
            }
            buf.push_str("while (");
            deparse_expr_into(buf, condition);
            buf.push_str(") {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
            if let Some(cont) = continue_block {
                buf.push_str(" continue {\n");
                deparse_block_into(buf, cont, indent + 1);
                buf.push('\n');
                buf.push_str(&ind);
                buf.push('}');
            }
        }
        StmtKind::Until {
            condition,
            body,
            label,
            continue_block,
        } => {
            if let Some(lbl) = label {
                let _ = write!(buf, "{}: ", lbl);
            }
            buf.push_str("until (");
            deparse_expr_into(buf, condition);
            buf.push_str(") {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
            if let Some(cont) = continue_block {
                buf.push_str(" continue {\n");
                deparse_block_into(buf, cont, indent + 1);
                buf.push('\n');
                buf.push_str(&ind);
                buf.push('}');
            }
        }
        StmtKind::DoWhile { body, condition } => {
            buf.push_str("do {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push_str("} while (");
            deparse_expr_into(buf, condition);
            buf.push_str(");");
        }
        StmtKind::For {
            init,
            condition,
            step,
            body,
            label,
            continue_block,
        } => {
            if let Some(lbl) = label {
                let _ = write!(buf, "{}: ", lbl);
            }
            buf.push_str("for (");
            if let Some(i) = init {
                deparse_stmt_into_no_semi(buf, i);
            }
            buf.push_str("; ");
            if let Some(c) = condition {
                deparse_expr_into(buf, c);
            }
            buf.push_str("; ");
            if let Some(s) = step {
                deparse_expr_into(buf, s);
            }
            buf.push_str(") {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
            if let Some(cont) = continue_block {
                buf.push_str(" continue {\n");
                deparse_block_into(buf, cont, indent + 1);
                buf.push('\n');
                buf.push_str(&ind);
                buf.push('}');
            }
        }
        StmtKind::Foreach {
            var,
            list,
            body,
            label,
            continue_block,
        } => {
            if let Some(lbl) = label {
                let _ = write!(buf, "{}: ", lbl);
            }
            let _ = write!(buf, "for my ${} (", var);
            deparse_expr_into(buf, list);
            buf.push_str(") {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
            if let Some(cont) = continue_block {
                buf.push_str(" continue {\n");
                deparse_block_into(buf, cont, indent + 1);
                buf.push('\n');
                buf.push_str(&ind);
                buf.push('}');
            }
        }
        StmtKind::SubDecl {
            name,
            params,
            body,
            prototype,
        } => {
            buf.push_str("sub ");
            buf.push_str(name);
            if let Some(proto) = prototype {
                let _ = write!(buf, " ({}) ", proto);
            } else if !params.is_empty() {
                buf.push_str(" (");
                deparse_params(buf, params);
                buf.push_str(") ");
            } else {
                buf.push(' ');
            }
            buf.push_str("{\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::Return(opt_expr) => {
            buf.push_str("return");
            if let Some(e) = opt_expr {
                buf.push(' ');
                deparse_expr_into(buf, e);
            }
            buf.push(';');
        }
        StmtKind::Last(label) => {
            buf.push_str("last");
            if let Some(lbl) = label {
                let _ = write!(buf, " {}", lbl);
            }
            buf.push(';');
        }
        StmtKind::Next(label) => {
            buf.push_str("next");
            if let Some(lbl) = label {
                let _ = write!(buf, " {}", lbl);
            }
            buf.push(';');
        }
        StmtKind::Redo(label) => {
            buf.push_str("redo");
            if let Some(lbl) = label {
                let _ = write!(buf, " {}", lbl);
            }
            buf.push(';');
        }
        StmtKind::My(decls) => {
            buf.push_str("my ");
            deparse_var_decls(buf, decls);
            buf.push(';');
        }
        StmtKind::Our(decls) => {
            buf.push_str("our ");
            deparse_var_decls(buf, decls);
            buf.push(';');
        }
        StmtKind::Local(decls) => {
            buf.push_str("local ");
            deparse_var_decls(buf, decls);
            buf.push(';');
        }
        StmtKind::State(decls) => {
            buf.push_str("state ");
            deparse_var_decls(buf, decls);
            buf.push(';');
        }
        StmtKind::MySync(decls) => {
            buf.push_str("mysync ");
            deparse_var_decls(buf, decls);
            buf.push(';');
        }
        StmtKind::LocalExpr {
            target,
            initializer,
        } => {
            buf.push_str("local ");
            deparse_expr_into(buf, target);
            if let Some(init) = initializer {
                buf.push_str(" = ");
                deparse_expr_into(buf, init);
            }
            buf.push(';');
        }
        StmtKind::Block(blk) => {
            buf.push_str("{\n");
            deparse_block_into(buf, blk, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::StmtGroup(blk) => {
            deparse_block_into(buf, blk, indent);
        }
        StmtKind::Package { name } => {
            let _ = write!(buf, "package {};", name);
        }
        StmtKind::Use { module, imports } => {
            let _ = write!(buf, "use {}", module);
            if !imports.is_empty() {
                buf.push_str(" qw(");
                for (i, imp) in imports.iter().enumerate() {
                    if i > 0 {
                        buf.push(' ');
                    }
                    deparse_expr_into(buf, imp);
                }
                buf.push(')');
            }
            buf.push(';');
        }
        StmtKind::UsePerlVersion { version } => {
            let _ = write!(buf, "use {};", version);
        }
        StmtKind::UseOverload { pairs } => {
            buf.push_str("use overload ");
            for (i, (op, meth)) in pairs.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                let _ = write!(buf, "'{}' => '{}'", op, meth);
            }
            buf.push(';');
        }
        StmtKind::No { module, imports } => {
            let _ = write!(buf, "no {}", module);
            if !imports.is_empty() {
                buf.push_str(" qw(");
                for (i, imp) in imports.iter().enumerate() {
                    if i > 0 {
                        buf.push(' ');
                    }
                    deparse_expr_into(buf, imp);
                }
                buf.push(')');
            }
            buf.push(';');
        }
        StmtKind::Begin(blk) => {
            buf.push_str("BEGIN {\n");
            deparse_block_into(buf, blk, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::End(blk) => {
            buf.push_str("END {\n");
            deparse_block_into(buf, blk, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::UnitCheck(blk) => {
            buf.push_str("UNITCHECK {\n");
            deparse_block_into(buf, blk, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::Check(blk) => {
            buf.push_str("CHECK {\n");
            deparse_block_into(buf, blk, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::TryCatch {
            try_block,
            catch_var,
            catch_block,
            finally_block,
        } => {
            buf.push_str("try {\n");
            deparse_block_into(buf, try_block, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            let _ = writeln!(buf, "}} catch (my ${}) {{", catch_var);
            deparse_block_into(buf, catch_block, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
            if let Some(fin) = finally_block {
                buf.push_str(" finally {\n");
                deparse_block_into(buf, fin, indent + 1);
                buf.push('\n');
                buf.push_str(&ind);
                buf.push('}');
            }
        }
        StmtKind::Given { topic, body } => {
            buf.push_str("given (");
            deparse_expr_into(buf, topic);
            buf.push_str(") {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::When { cond, body } => {
            buf.push_str("when (");
            deparse_expr_into(buf, cond);
            buf.push_str(") {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::DefaultCase { body } => {
            buf.push_str("default {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::StructDecl { def } => {
            let _ = write!(buf, "struct {} {{ ", def.name);
            for (i, field) in def.fields.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                let _ = write!(buf, "{} => {}", field.name, field.ty.display_name());
            }
            buf.push_str(" }");
        }
        StmtKind::EnumDecl { def } => {
            let _ = write!(buf, "enum {} {{ ", def.name);
            for (i, variant) in def.variants.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                if let Some(ty) = &variant.ty {
                    let _ = write!(buf, "{} => {}", variant.name, ty.display_name());
                } else {
                    buf.push_str(&variant.name);
                }
            }
            buf.push_str(" }");
        }
        StmtKind::ClassDecl { def } => {
            let _ = write!(buf, "class {}", def.name);
            if !def.extends.is_empty() {
                let _ = write!(buf, " extends {}", def.extends.join(", "));
            }
            if !def.implements.is_empty() {
                let _ = write!(buf, " impl {}", def.implements.join(", "));
            }
            buf.push_str(" { ");
            for (i, field) in def.fields.iter().enumerate() {
                if i > 0 {
                    buf.push_str("; ");
                }
                if matches!(field.visibility, crate::ast::Visibility::Private) {
                    buf.push_str("priv ");
                }
                let _ = write!(buf, "{}: {}", field.name, field.ty.display_name());
            }
            buf.push_str(" }");
        }
        StmtKind::TraitDecl { def } => {
            let _ = write!(buf, "trait {} {{ ", def.name);
            for (i, method) in def.methods.iter().enumerate() {
                if i > 0 {
                    buf.push_str("; ");
                }
                let _ = write!(buf, "fn {}", method.name);
            }
            buf.push_str(" }");
        }
        StmtKind::Empty => {
            buf.push(';');
        }
        StmtKind::Goto { target } => {
            buf.push_str("goto ");
            deparse_expr_into(buf, target);
            buf.push(';');
        }
        StmtKind::Continue(blk) => {
            buf.push_str("continue {\n");
            deparse_block_into(buf, blk, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::Init(blk) => {
            buf.push_str("INIT {\n");
            deparse_block_into(buf, blk, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::EvalTimeout { timeout, body } => {
            buf.push_str("eval_timeout ");
            deparse_expr_into(buf, timeout);
            buf.push_str(" {\n");
            deparse_block_into(buf, body, indent + 1);
            buf.push('\n');
            buf.push_str(&ind);
            buf.push('}');
        }
        StmtKind::Tie {
            target,
            class,
            args,
        } => {
            buf.push_str("tie ");
            match target {
                TieTarget::Hash(n) => {
                    let _ = write!(buf, "%{}", n);
                }
                TieTarget::Array(n) => {
                    let _ = write!(buf, "@{}", n);
                }
                TieTarget::Scalar(n) => {
                    let _ = write!(buf, "${}", n);
                }
            }
            buf.push_str(", ");
            deparse_expr_into(buf, class);
            for a in args {
                buf.push_str(", ");
                deparse_expr_into(buf, a);
            }
            buf.push(';');
        }
        StmtKind::FormatDecl { name, lines } => {
            let _ = writeln!(buf, "format {} =", name);
            for l in lines {
                buf.push_str(l);
                buf.push('\n');
            }
            buf.push('.');
        }
    }
}

fn deparse_stmt_into_no_semi(buf: &mut String, stmt: &Statement) {
    match &stmt.kind {
        StmtKind::Expression(e) => deparse_expr_into(buf, e),
        StmtKind::My(decls) => {
            buf.push_str("my ");
            deparse_var_decls(buf, decls);
        }
        _ => deparse_stmt_into(buf, stmt, 0),
    }
}

fn sigil_char(s: &Sigil) -> char {
    match s {
        Sigil::Scalar => '$',
        Sigil::Array => '@',
        Sigil::Hash => '%',
        Sigil::Typeglob => '*',
    }
}

fn deparse_var_decls(buf: &mut String, decls: &[VarDecl]) {
    if decls.len() == 1 {
        let d = &decls[0];
        let _ = write!(buf, "{}{}", sigil_char(&d.sigil), d.name);
        if let Some(init) = &d.initializer {
            buf.push_str(" = ");
            deparse_expr_into(buf, init);
        }
    } else {
        buf.push('(');
        for (i, d) in decls.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            let _ = write!(buf, "{}{}", sigil_char(&d.sigil), d.name);
        }
        buf.push(')');
        let has_init = decls.iter().any(|d| d.initializer.is_some());
        if has_init {
            buf.push_str(" = (");
            for (i, d) in decls.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                if let Some(init) = &d.initializer {
                    deparse_expr_into(buf, init);
                } else {
                    buf.push_str("undef");
                }
            }
            buf.push(')');
        }
    }
}

fn deparse_params(buf: &mut String, params: &[SubSigParam]) {
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        match p {
            SubSigParam::Scalar(name, ty) => {
                let _ = write!(buf, "${}", name);
                if let Some(t) = ty {
                    buf.push_str(": ");
                    buf.push_str(&t.display_name());
                }
            }
            SubSigParam::ArrayDestruct(elems) => {
                buf.push('[');
                for (j, elem) in elems.iter().enumerate() {
                    if j > 0 {
                        buf.push_str(", ");
                    }
                    match elem {
                        MatchArrayElem::CaptureScalar(n) => {
                            let _ = write!(buf, "${}", n);
                        }
                        MatchArrayElem::RestBind(n) => {
                            let _ = write!(buf, "@{}", n);
                        }
                        MatchArrayElem::Rest => buf.push('*'),
                        MatchArrayElem::Expr(e) => deparse_expr_into(buf, e),
                    }
                }
                buf.push(']');
            }
            SubSigParam::HashDestruct(pairs) => {
                buf.push('{');
                for (j, (k, v)) in pairs.iter().enumerate() {
                    if j > 0 {
                        buf.push_str(", ");
                    }
                    let _ = write!(buf, "{} => ${}", k, v);
                }
                buf.push('}');
            }
        }
    }
}

fn deparse_expr_into(buf: &mut String, expr: &Expr) {
    match &expr.kind {
        ExprKind::Integer(n) => {
            let _ = write!(buf, "{}", n);
        }
        ExprKind::Float(f) => {
            let _ = write!(buf, "{}", f);
        }
        ExprKind::String(s) => {
            let _ = write!(buf, "\"{}\"", escape_string(s));
        }
        ExprKind::Bareword(s) => {
            buf.push_str(s);
        }
        ExprKind::Regex(pat, flags) => {
            let d = choose_delim();
            let _ = write!(buf, "qr{}{}{}{}", d, escape_regex_delim(pat, d), d, flags);
        }
        ExprKind::QW(words) => {
            buf.push_str("qw(");
            for (i, w) in words.iter().enumerate() {
                if i > 0 {
                    buf.push(' ');
                }
                buf.push_str(w);
            }
            buf.push(')');
        }
        ExprKind::Undef => {
            buf.push_str("undef");
        }
        ExprKind::MagicConst(kind) => {
            buf.push_str(match kind {
                MagicConstKind::File => "__FILE__",
                MagicConstKind::Line => "__LINE__",
                MagicConstKind::Sub => "__SUB__",
            });
        }
        ExprKind::InterpolatedString(parts) => {
            buf.push('"');
            for part in parts {
                match part {
                    StringPart::Literal(s) => buf.push_str(&escape_string(s)),
                    StringPart::ScalarVar(name) => {
                        let _ = write!(buf, "${}", name);
                    }
                    StringPart::ArrayVar(name) => {
                        let _ = write!(buf, "@{}", name);
                    }
                    StringPart::Expr(e) => {
                        buf.push_str("${");
                        deparse_expr_into(buf, e);
                        buf.push('}');
                    }
                }
            }
            buf.push('"');
        }
        ExprKind::ScalarVar(name) => {
            let _ = write!(buf, "${}", name);
        }
        ExprKind::ArrayVar(name) => {
            let _ = write!(buf, "@{}", name);
        }
        ExprKind::HashVar(name) => {
            let _ = write!(buf, "%{}", name);
        }
        ExprKind::ArrayElement { array, index } => {
            let _ = write!(buf, "${}[", array);
            deparse_expr_into(buf, index);
            buf.push(']');
        }
        ExprKind::HashElement { hash, key } => {
            let _ = write!(buf, "${}{{", hash);
            deparse_expr_into(buf, key);
            buf.push('}');
        }
        ExprKind::ArraySlice { array, indices } => {
            let _ = write!(buf, "@{}[", array);
            deparse_list(buf, indices);
            buf.push(']');
        }
        ExprKind::HashSlice { hash, keys } => {
            let _ = write!(buf, "@{}{{", hash);
            deparse_list(buf, keys);
            buf.push('}');
        }
        ExprKind::HashSliceDeref { container, keys } => {
            buf.push_str("@{");
            deparse_expr_into(buf, container);
            buf.push_str("}{");
            deparse_list(buf, keys);
            buf.push('}');
        }
        ExprKind::AnonymousListSlice { source, indices } => {
            buf.push('(');
            deparse_expr_into(buf, source);
            buf.push_str(")[");
            deparse_list(buf, indices);
            buf.push(']');
        }
        ExprKind::ScalarRef(e) => {
            buf.push('\\');
            deparse_expr_into(buf, e);
        }
        ExprKind::ArrayRef(elems) => {
            buf.push('[');
            deparse_list(buf, elems);
            buf.push(']');
        }
        ExprKind::HashRef(pairs) => {
            buf.push_str("+{");
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                deparse_expr_into(buf, k);
                buf.push_str(" => ");
                deparse_expr_into(buf, v);
            }
            buf.push('}');
        }
        ExprKind::CodeRef { params, body } => {
            buf.push_str("sub");
            if !params.is_empty() {
                buf.push_str(" (");
                deparse_params(buf, params);
                buf.push(')');
            }
            buf.push_str(" { ");
            deparse_block_into(buf, body, 0);
            buf.push_str(" }");
        }
        ExprKind::SubroutineRef(name) => {
            let _ = write!(buf, "&{}", name);
        }
        ExprKind::SubroutineCodeRef(name) => {
            let _ = write!(buf, "\\&{}", name);
        }
        ExprKind::DynamicSubCodeRef(e) => {
            buf.push_str("\\&{");
            deparse_expr_into(buf, e);
            buf.push('}');
        }
        ExprKind::Deref { expr, kind } => {
            buf.push(sigil_char(kind));
            buf.push('{');
            deparse_expr_into(buf, expr);
            buf.push('}');
        }
        ExprKind::ArrowDeref { expr, index, kind } => {
            deparse_expr_into(buf, expr);
            match kind {
                DerefKind::Array => {
                    buf.push_str("->[");
                    deparse_expr_into(buf, index);
                    buf.push(']');
                }
                DerefKind::Hash => {
                    buf.push_str("->{");
                    deparse_expr_into(buf, index);
                    buf.push('}');
                }
                DerefKind::Call => {
                    buf.push_str("->(");
                    deparse_expr_into(buf, index);
                    buf.push(')');
                }
            }
        }
        ExprKind::BinOp { left, op, right } => {
            let needs_parens = matches!(op, BinOp::LogAnd | BinOp::LogOr | BinOp::DefinedOr);
            if needs_parens {
                buf.push('(');
            }
            deparse_expr_into(buf, left);
            buf.push(' ');
            buf.push_str(binop_str(*op));
            buf.push(' ');
            deparse_expr_into(buf, right);
            if needs_parens {
                buf.push(')');
            }
        }
        ExprKind::UnaryOp { op, expr } => {
            match op {
                UnaryOp::Negate => buf.push('-'),
                UnaryOp::LogNot => buf.push('!'),
                UnaryOp::BitNot => buf.push('~'),
                UnaryOp::LogNotWord => buf.push_str("not "),
                UnaryOp::PreIncrement => buf.push_str("++"),
                UnaryOp::PreDecrement => buf.push_str("--"),
                UnaryOp::Ref => buf.push('\\'),
            }
            deparse_expr_into(buf, expr);
        }
        ExprKind::PostfixOp { expr, op } => {
            deparse_expr_into(buf, expr);
            match op {
                PostfixOp::Increment => buf.push_str("++"),
                PostfixOp::Decrement => buf.push_str("--"),
            }
        }
        ExprKind::Assign { target, value } => {
            deparse_expr_into(buf, target);
            buf.push_str(" = ");
            deparse_expr_into(buf, value);
        }
        ExprKind::CompoundAssign { target, op, value } => {
            deparse_expr_into(buf, target);
            buf.push(' ');
            buf.push_str(compound_assign_str(*op));
            buf.push_str("= ");
            deparse_expr_into(buf, value);
        }
        ExprKind::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            buf.push('(');
            deparse_expr_into(buf, condition);
            buf.push_str(" ? ");
            deparse_expr_into(buf, then_expr);
            buf.push_str(" : ");
            deparse_expr_into(buf, else_expr);
            buf.push(')');
        }
        ExprKind::Repeat { expr, count } => {
            deparse_expr_into(buf, expr);
            buf.push_str(" x ");
            deparse_expr_into(buf, count);
        }
        ExprKind::Range {
            from,
            to,
            exclusive,
        } => {
            deparse_expr_into(buf, from);
            buf.push_str(if *exclusive { " ... " } else { " .. " });
            deparse_expr_into(buf, to);
        }
        ExprKind::FuncCall { name, args } => {
            buf.push_str(name);
            if args.is_empty() {
                buf.push_str("()");
            } else {
                buf.push('(');
                deparse_list(buf, args);
                buf.push(')');
            }
        }
        ExprKind::MethodCall {
            object,
            method,
            args,
            super_call,
        } => {
            deparse_expr_into(buf, object);
            buf.push_str("->");
            if *super_call {
                buf.push_str("SUPER::");
            }
            buf.push_str(method);
            if !args.is_empty() {
                buf.push('(');
                deparse_list(buf, args);
                buf.push(')');
            }
        }
        ExprKind::IndirectCall {
            target,
            args,
            ampersand,
            pass_caller_arglist,
        } => {
            if *ampersand {
                buf.push('&');
            }
            deparse_expr_into(buf, target);
            if !*pass_caller_arglist {
                buf.push('(');
                deparse_list(buf, args);
                buf.push(')');
            }
        }
        ExprKind::Typeglob(name) => {
            let _ = write!(buf, "*{}", name);
        }
        ExprKind::TypeglobExpr(e) => {
            buf.push_str("*{");
            deparse_expr_into(buf, e);
            buf.push('}');
        }
        ExprKind::Print { handle, args } => {
            buf.push_str("print");
            if let Some(h) = handle {
                let _ = write!(buf, " {}", h);
            }
            if !args.is_empty() {
                buf.push(' ');
                deparse_list(buf, args);
            }
        }
        ExprKind::Say { handle, args } => {
            buf.push_str("say");
            if let Some(h) = handle {
                let _ = write!(buf, " {}", h);
            }
            if !args.is_empty() {
                buf.push(' ');
                deparse_list(buf, args);
            }
        }
        ExprKind::Printf { handle, args } => {
            buf.push_str("printf");
            if let Some(h) = handle {
                let _ = write!(buf, " {}", h);
            }
            if !args.is_empty() {
                buf.push(' ');
                deparse_list(buf, args);
            }
        }
        ExprKind::Die(args) => {
            buf.push_str("die");
            if !args.is_empty() {
                buf.push(' ');
                deparse_list(buf, args);
            }
        }
        ExprKind::Warn(args) => {
            buf.push_str("warn");
            if !args.is_empty() {
                buf.push(' ');
                deparse_list(buf, args);
            }
        }
        ExprKind::Match {
            expr,
            pattern,
            flags,
            ..
        } => {
            let d = choose_delim();
            deparse_expr_into(buf, expr);
            let _ = write!(
                buf,
                " =~ {}{}{}{}",
                d,
                escape_regex_delim(pattern, d),
                d,
                flags
            );
        }
        ExprKind::Substitution {
            expr,
            pattern,
            replacement,
            flags,
            delim: _,
        } => {
            let d = choose_delim();
            deparse_expr_into(buf, expr);
            let _ = write!(
                buf,
                " =~ s{}{}{}{}{}{}",
                d,
                escape_regex_delim(pattern, d),
                d,
                escape_regex_delim(replacement, d),
                d,
                flags
            );
        }
        ExprKind::Transliterate {
            expr,
            from,
            to,
            flags,
            delim: _,
        } => {
            let d = choose_delim();
            deparse_expr_into(buf, expr);
            let _ = write!(
                buf,
                " =~ tr{}{}{}{}{}{}",
                d,
                escape_tr_delim(from, d),
                d,
                escape_tr_delim(to, d),
                d,
                flags
            );
        }
        ExprKind::MapExpr {
            block,
            list,
            flatten_array_refs,
            stream,
        } => {
            let name = match (*flatten_array_refs, *stream) {
                (false, false) => "map",
                (true, false) => "flat_map",
                (false, true) => "maps",
                (true, true) => "flat_maps",
            };
            let _ = write!(buf, "{} {{ ", name);
            deparse_block_into(buf, block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
        }
        ExprKind::MapExprComma { expr, list, .. } => {
            buf.push_str("map ");
            deparse_expr_into(buf, expr);
            buf.push_str(", ");
            deparse_expr_into(buf, list);
        }
        ExprKind::GrepExpr {
            block,
            list,
            keyword,
        } => {
            let _ = write!(buf, "{} {{ ", keyword.as_str());
            deparse_block_into(buf, block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
        }
        ExprKind::GrepExprComma {
            expr,
            list,
            keyword,
        } => {
            let _ = write!(buf, "{} ", keyword.as_str());
            deparse_expr_into(buf, expr);
            buf.push_str(", ");
            deparse_expr_into(buf, list);
        }
        ExprKind::SortExpr { cmp, list } => {
            buf.push_str("sort");
            if let Some(c) = cmp {
                match c {
                    SortComparator::Block(blk) => {
                        buf.push_str(" { ");
                        deparse_block_into(buf, blk, 0);
                        buf.push_str(" } ");
                    }
                    SortComparator::Code(e) => {
                        buf.push(' ');
                        deparse_expr_into(buf, e);
                        buf.push(' ');
                    }
                }
            } else {
                buf.push(' ');
            }
            deparse_expr_into(buf, list);
        }
        ExprKind::ReverseExpr(e) => {
            buf.push_str("reverse ");
            deparse_expr_into(buf, e);
        }
        ExprKind::ScalarReverse(e) => {
            buf.push_str("rev ");
            deparse_expr_into(buf, e);
        }
        ExprKind::JoinExpr { separator, list } => {
            buf.push_str("join ");
            deparse_expr_into(buf, separator);
            buf.push_str(", ");
            deparse_expr_into(buf, list);
        }
        ExprKind::SplitExpr {
            pattern,
            string,
            limit,
        } => {
            buf.push_str("split ");
            deparse_expr_into(buf, pattern);
            buf.push_str(", ");
            deparse_expr_into(buf, string);
            if let Some(l) = limit {
                buf.push_str(", ");
                deparse_expr_into(buf, l);
            }
        }
        ExprKind::ForEachExpr { block, list } => {
            buf.push_str("each { ");
            deparse_block_into(buf, block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
        }
        ExprKind::ReduceExpr { block, list } => {
            buf.push_str("reduce { ");
            deparse_block_into(buf, block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
        }
        ExprKind::PMapExpr {
            block,
            list,
            progress,
            flat_outputs,
            on_cluster,
            stream: _,
        } => {
            if *flat_outputs {
                buf.push_str("pflat_map");
            } else {
                buf.push_str("pmap");
            }
            if let Some(cluster) = on_cluster {
                buf.push_str("_on ");
                deparse_expr_into(buf, cluster);
            }
            buf.push_str(" { ");
            deparse_block_into(buf, block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
            if let Some(p) = progress {
                buf.push_str(", progress => ");
                deparse_expr_into(buf, p);
            }
        }
        ExprKind::PGrepExpr {
            block,
            list,
            progress,
            stream: _,
        } => {
            buf.push_str("pgrep { ");
            deparse_block_into(buf, block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
            if let Some(p) = progress {
                buf.push_str(", progress => ");
                deparse_expr_into(buf, p);
            }
        }
        ExprKind::PForExpr {
            block,
            list,
            progress,
        } => {
            buf.push_str("pfor { ");
            deparse_block_into(buf, block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
            if let Some(p) = progress {
                buf.push_str(", progress => ");
                deparse_expr_into(buf, p);
            }
        }
        ExprKind::PSortExpr {
            cmp,
            list,
            progress,
        } => {
            buf.push_str("psort");
            if let Some(blk) = cmp {
                buf.push_str(" { ");
                deparse_block_into(buf, blk, 0);
                buf.push_str(" }");
            }
            buf.push(' ');
            deparse_expr_into(buf, list);
            if let Some(p) = progress {
                buf.push_str(", progress => ");
                deparse_expr_into(buf, p);
            }
        }
        ExprKind::PReduceExpr {
            block,
            list,
            progress,
        } => {
            buf.push_str("preduce { ");
            deparse_block_into(buf, block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
            if let Some(p) = progress {
                buf.push_str(", progress => ");
                deparse_expr_into(buf, p);
            }
        }
        ExprKind::PReduceInitExpr {
            init,
            block,
            list,
            progress,
        } => {
            buf.push_str("preduce_init ");
            deparse_expr_into(buf, init);
            buf.push_str(", { ");
            deparse_block_into(buf, block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
            if let Some(p) = progress {
                buf.push_str(", progress => ");
                deparse_expr_into(buf, p);
            }
        }
        ExprKind::PMapReduceExpr {
            map_block,
            reduce_block,
            list,
            progress,
        } => {
            buf.push_str("pmap_reduce { ");
            deparse_block_into(buf, map_block, 0);
            buf.push_str(" } { ");
            deparse_block_into(buf, reduce_block, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, list);
            if let Some(p) = progress {
                buf.push_str(", progress => ");
                deparse_expr_into(buf, p);
            }
        }
        ExprKind::AsyncBlock { body } => {
            buf.push_str("async { ");
            deparse_block_into(buf, body, 0);
            buf.push_str(" }");
        }
        ExprKind::SpawnBlock { body } => {
            buf.push_str("spawn { ");
            deparse_block_into(buf, body, 0);
            buf.push_str(" }");
        }
        ExprKind::Trace { body } => {
            buf.push_str("trace { ");
            deparse_block_into(buf, body, 0);
            buf.push_str(" }");
        }
        ExprKind::Timer { body } => {
            buf.push_str("timer { ");
            deparse_block_into(buf, body, 0);
            buf.push_str(" }");
        }
        ExprKind::Bench { body, times } => {
            buf.push_str("bench { ");
            deparse_block_into(buf, body, 0);
            buf.push_str(" } ");
            deparse_expr_into(buf, times);
        }
        ExprKind::Await(e) => {
            buf.push_str("await ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Slurp(e) => {
            buf.push_str("slurp ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Capture(e) => {
            buf.push_str("capture ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Qx(e) => {
            buf.push_str("qx(");
            deparse_expr_into(buf, e);
            buf.push(')');
        }
        ExprKind::FetchUrl(e) => {
            buf.push_str("fetch ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Pchannel { capacity } => {
            buf.push_str("pchannel(");
            if let Some(cap) = capacity {
                deparse_expr_into(buf, cap);
            }
            buf.push(')');
        }
        ExprKind::Push { array, values } => {
            buf.push_str("push ");
            deparse_expr_into(buf, array);
            buf.push_str(", ");
            deparse_list(buf, values);
        }
        ExprKind::Pop(e) => {
            buf.push_str("pop ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Shift(e) => {
            buf.push_str("shift ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Unshift { array, values } => {
            buf.push_str("unshift ");
            deparse_expr_into(buf, array);
            buf.push_str(", ");
            deparse_list(buf, values);
        }
        ExprKind::Splice {
            array,
            offset,
            length,
            replacement,
        } => {
            buf.push_str("splice ");
            deparse_expr_into(buf, array);
            if let Some(o) = offset {
                buf.push_str(", ");
                deparse_expr_into(buf, o);
                if let Some(l) = length {
                    buf.push_str(", ");
                    deparse_expr_into(buf, l);
                    if !replacement.is_empty() {
                        buf.push_str(", ");
                        deparse_list(buf, replacement);
                    }
                }
            }
        }
        ExprKind::Delete(e) => {
            buf.push_str("delete ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Exists(e) => {
            buf.push_str("exists ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Keys(e) => {
            buf.push_str("keys ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Values(e) => {
            buf.push_str("values ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Each(e) => {
            buf.push_str("each ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Chomp(e) => {
            buf.push_str("chomp ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Chop(e) => {
            buf.push_str("chop ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Length(e) => {
            buf.push_str("length ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Substr {
            string,
            offset,
            length,
            replacement,
        } => {
            buf.push_str("substr(");
            deparse_expr_into(buf, string);
            buf.push_str(", ");
            deparse_expr_into(buf, offset);
            if let Some(l) = length {
                buf.push_str(", ");
                deparse_expr_into(buf, l);
                if let Some(r) = replacement {
                    buf.push_str(", ");
                    deparse_expr_into(buf, r);
                }
            }
            buf.push(')');
        }
        ExprKind::Index {
            string,
            substr,
            position,
        } => {
            buf.push_str("index(");
            deparse_expr_into(buf, string);
            buf.push_str(", ");
            deparse_expr_into(buf, substr);
            if let Some(p) = position {
                buf.push_str(", ");
                deparse_expr_into(buf, p);
            }
            buf.push(')');
        }
        ExprKind::Rindex {
            string,
            substr,
            position,
        } => {
            buf.push_str("rindex(");
            deparse_expr_into(buf, string);
            buf.push_str(", ");
            deparse_expr_into(buf, substr);
            if let Some(p) = position {
                buf.push_str(", ");
                deparse_expr_into(buf, p);
            }
            buf.push(')');
        }
        ExprKind::Sprintf { format, args } => {
            buf.push_str("sprintf ");
            deparse_expr_into(buf, format);
            if !args.is_empty() {
                buf.push_str(", ");
                deparse_list(buf, args);
            }
        }
        ExprKind::Abs(e) => {
            buf.push_str("abs ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Int(e) => {
            buf.push_str("int ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Sqrt(e) => {
            buf.push_str("sqrt ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Sin(e) => {
            buf.push_str("sin ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Cos(e) => {
            buf.push_str("cos ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Exp(e) => {
            buf.push_str("exp ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Log(e) => {
            buf.push_str("log ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Hex(e) => {
            buf.push_str("hex ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Oct(e) => {
            buf.push_str("oct ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Atan2 { y, x } => {
            buf.push_str("atan2(");
            deparse_expr_into(buf, y);
            buf.push_str(", ");
            deparse_expr_into(buf, x);
            buf.push(')');
        }
        ExprKind::Rand(opt) => {
            buf.push_str("rand");
            if let Some(e) = opt {
                buf.push('(');
                deparse_expr_into(buf, e);
                buf.push(')');
            } else {
                buf.push_str("()");
            }
        }
        ExprKind::Srand(opt) => {
            buf.push_str("srand");
            if let Some(e) = opt {
                buf.push('(');
                deparse_expr_into(buf, e);
                buf.push(')');
            } else {
                buf.push_str("()");
            }
        }
        ExprKind::Lc(e) => {
            buf.push_str("lc ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Uc(e) => {
            buf.push_str("uc ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Lcfirst(e) => {
            buf.push_str("lcfirst ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Ucfirst(e) => {
            buf.push_str("ucfirst ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Fc(e) => {
            buf.push_str("fc ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Crypt { plaintext, salt } => {
            buf.push_str("crypt(");
            deparse_expr_into(buf, plaintext);
            buf.push_str(", ");
            deparse_expr_into(buf, salt);
            buf.push(')');
        }
        ExprKind::Pos(opt) => {
            buf.push_str("pos");
            if let Some(e) = opt {
                buf.push('(');
                deparse_expr_into(buf, e);
                buf.push(')');
            }
        }
        ExprKind::Study(e) => {
            buf.push_str("study ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Defined(e) => {
            buf.push_str("defined ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Ref(e) => {
            buf.push_str("ref ");
            deparse_expr_into(buf, e);
        }
        ExprKind::ScalarContext(e) => {
            buf.push_str("scalar ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Chr(e) => {
            buf.push_str("chr ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Ord(e) => {
            buf.push_str("ord ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Open { handle, mode, file } => {
            buf.push_str("open ");
            deparse_expr_into(buf, handle);
            buf.push_str(", ");
            deparse_expr_into(buf, mode);
            if let Some(f) = file {
                buf.push_str(", ");
                deparse_expr_into(buf, f);
            }
        }
        ExprKind::OpenMyHandle { name } => {
            let _ = write!(buf, "my ${}", name);
        }
        ExprKind::Close(e) => {
            buf.push_str("close ");
            deparse_expr_into(buf, e);
        }
        ExprKind::ReadLine(h) => {
            if let Some(name) = h {
                let _ = write!(buf, "<{}>", name);
            } else {
                buf.push_str("<>");
            }
        }
        ExprKind::Eof(opt) => {
            buf.push_str("eof");
            if let Some(e) = opt {
                buf.push('(');
                deparse_expr_into(buf, e);
                buf.push(')');
            }
        }
        ExprKind::FileTest { op, expr } => {
            let _ = write!(buf, "-{} ", op);
            deparse_expr_into(buf, expr);
        }
        ExprKind::System(args) => {
            buf.push_str("system ");
            deparse_list(buf, args);
        }
        ExprKind::Exec(args) => {
            buf.push_str("exec ");
            deparse_list(buf, args);
        }
        ExprKind::Eval(e) => {
            buf.push_str("eval ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Do(e) => {
            buf.push_str("do ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Require(e) => {
            buf.push_str("require ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Exit(opt) => {
            buf.push_str("exit");
            if let Some(e) = opt {
                buf.push(' ');
                deparse_expr_into(buf, e);
            }
        }
        ExprKind::Chdir(e) => {
            buf.push_str("chdir ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Mkdir { path, mode } => {
            buf.push_str("mkdir ");
            deparse_expr_into(buf, path);
            if let Some(m) = mode {
                buf.push_str(", ");
                deparse_expr_into(buf, m);
            }
        }
        ExprKind::Unlink(args) => {
            buf.push_str("unlink ");
            deparse_list(buf, args);
        }
        ExprKind::Rename { old, new } => {
            buf.push_str("rename ");
            deparse_expr_into(buf, old);
            buf.push_str(", ");
            deparse_expr_into(buf, new);
        }
        ExprKind::Chmod(args) => {
            buf.push_str("chmod ");
            deparse_list(buf, args);
        }
        ExprKind::Chown(args) => {
            buf.push_str("chown ");
            deparse_list(buf, args);
        }
        ExprKind::Stat(e) => {
            buf.push_str("stat ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Lstat(e) => {
            buf.push_str("lstat ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Link { old, new } => {
            buf.push_str("link ");
            deparse_expr_into(buf, old);
            buf.push_str(", ");
            deparse_expr_into(buf, new);
        }
        ExprKind::Symlink { old, new } => {
            buf.push_str("symlink ");
            deparse_expr_into(buf, old);
            buf.push_str(", ");
            deparse_expr_into(buf, new);
        }
        ExprKind::Readlink(e) => {
            buf.push_str("readlink ");
            deparse_expr_into(buf, e);
        }
        ExprKind::Files(args)
        | ExprKind::Filesf(args)
        | ExprKind::FilesfRecursive(args)
        | ExprKind::Dirs(args)
        | ExprKind::DirsRecursive(args)
        | ExprKind::SymLinks(args)
        | ExprKind::Sockets(args)
        | ExprKind::Pipes(args)
        | ExprKind::BlockDevices(args)
        | ExprKind::CharDevices(args)
        | ExprKind::Glob(args) => {
            let name = match &expr.kind {
                ExprKind::Files(_) => "files",
                ExprKind::Filesf(_) => "filesf",
                ExprKind::FilesfRecursive(_) => "fr",
                ExprKind::Dirs(_) => "dirs",
                ExprKind::DirsRecursive(_) => "dr",
                ExprKind::SymLinks(_) => "sym_links",
                ExprKind::Sockets(_) => "sockets",
                ExprKind::Pipes(_) => "pipes",
                ExprKind::BlockDevices(_) => "block_devices",
                ExprKind::CharDevices(_) => "char_devices",
                ExprKind::Glob(_) => "glob",
                _ => unreachable!(),
            };
            buf.push_str(name);
            if !args.is_empty() {
                buf.push(' ');
                deparse_list(buf, args);
            }
        }
        ExprKind::Bless { ref_expr, class } => {
            buf.push_str("bless ");
            deparse_expr_into(buf, ref_expr);
            if let Some(c) = class {
                buf.push_str(", ");
                deparse_expr_into(buf, c);
            }
        }
        ExprKind::Caller(opt) => {
            buf.push_str("caller");
            if let Some(e) = opt {
                buf.push('(');
                deparse_expr_into(buf, e);
                buf.push(')');
            }
        }
        ExprKind::Wantarray => {
            buf.push_str("wantarray");
        }
        ExprKind::List(elems) => {
            buf.push('(');
            deparse_list(buf, elems);
            buf.push(')');
        }
        ExprKind::PostfixIf { expr, condition } => {
            deparse_expr_into(buf, expr);
            buf.push_str(" if ");
            deparse_expr_into(buf, condition);
        }
        ExprKind::PostfixUnless { expr, condition } => {
            deparse_expr_into(buf, expr);
            buf.push_str(" unless ");
            deparse_expr_into(buf, condition);
        }
        ExprKind::PostfixWhile { expr, condition } => {
            deparse_expr_into(buf, expr);
            buf.push_str(" while ");
            deparse_expr_into(buf, condition);
        }
        ExprKind::PostfixUntil { expr, condition } => {
            deparse_expr_into(buf, expr);
            buf.push_str(" until ");
            deparse_expr_into(buf, condition);
        }
        ExprKind::PostfixForeach { expr, list } => {
            deparse_expr_into(buf, expr);
            buf.push_str(" for ");
            deparse_expr_into(buf, list);
        }
        ExprKind::GenBlock { body } => {
            buf.push_str("gen { ");
            deparse_block_into(buf, body, 0);
            buf.push_str(" }");
        }
        ExprKind::Yield(e) => {
            buf.push_str("yield ");
            deparse_expr_into(buf, e);
        }
        ExprKind::RetryBlock {
            body,
            times,
            backoff,
        } => {
            buf.push_str("retry { ");
            deparse_block_into(buf, body, 0);
            buf.push_str(" } times => ");
            deparse_expr_into(buf, times);
            match backoff {
                RetryBackoff::None => {}
                RetryBackoff::Linear => buf.push_str(", backoff => linear"),
                RetryBackoff::Exponential => buf.push_str(", backoff => exponential"),
            }
        }
        ExprKind::EveryBlock { interval, body } => {
            buf.push_str("every(");
            deparse_expr_into(buf, interval);
            buf.push_str(") { ");
            deparse_block_into(buf, body, 0);
            buf.push_str(" }");
        }
        ExprKind::AlgebraicMatch { subject, arms } => {
            buf.push_str("match (");
            deparse_expr_into(buf, subject);
            buf.push_str(") { ");
            for (i, arm) in arms.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                deparse_match_pattern(buf, &arm.pattern);
                if let Some(guard) = &arm.guard {
                    buf.push_str(" if ");
                    deparse_expr_into(buf, guard);
                }
                buf.push_str(" => ");
                deparse_expr_into(buf, &arm.body);
            }
            buf.push_str(" }");
        }
        _ => {
            buf.push_str("...");
        }
    }
}

fn deparse_match_pattern(buf: &mut String, pat: &MatchPattern) {
    match pat {
        MatchPattern::Any => buf.push('_'),
        MatchPattern::Value(e) => deparse_expr_into(buf, e),
        MatchPattern::Regex { pattern, flags } => {
            let d = choose_delim();
            let _ = write!(buf, "{}{}{}{}", d, escape_regex_delim(pattern, d), d, flags);
        }
        MatchPattern::Array(elems) => {
            buf.push('[');
            for (i, elem) in elems.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                match elem {
                    MatchArrayElem::CaptureScalar(n) => {
                        let _ = write!(buf, "${}", n);
                    }
                    MatchArrayElem::RestBind(n) => {
                        let _ = write!(buf, "@{}", n);
                    }
                    MatchArrayElem::Rest => buf.push('*'),
                    MatchArrayElem::Expr(e) => deparse_expr_into(buf, e),
                }
            }
            buf.push(']');
        }
        MatchPattern::Hash(pairs) => {
            buf.push('{');
            for (i, pair) in pairs.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                match pair {
                    MatchHashPair::KeyOnly { key } => {
                        deparse_expr_into(buf, key);
                        buf.push_str(" => _");
                    }
                    MatchHashPair::Capture { key, name } => {
                        deparse_expr_into(buf, key);
                        let _ = write!(buf, " => ${}", name);
                    }
                }
            }
            buf.push('}');
        }
        MatchPattern::OptionSome(name) => {
            let _ = write!(buf, "Some(${})", name);
        }
    }
}

fn deparse_list(buf: &mut String, exprs: &[Expr]) {
    for (i, e) in exprs.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        deparse_expr_into(buf, e);
    }
}

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '$' => out.push_str("\\$"),
            '@' => out.push_str("\\@"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_regex_delim(s: &str, delim: char) -> String {
    let delim_str = delim.to_string();
    let escaped = format!("\\{}", delim);
    s.replace(&delim_str, &escaped)
}

fn escape_tr_delim(s: &str, delim: char) -> String {
    let delim_str = delim.to_string();
    let escaped = format!("\\{}", delim);
    s.replace(&delim_str, &escaped)
}

fn binop_str(op: BinOp) -> &'static str {
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

fn compound_assign_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Concat => ".",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::ShiftLeft => "<<",
        BinOp::ShiftRight => ">>",
        BinOp::LogAnd => "&&",
        BinOp::LogOr => "||",
        BinOp::DefinedOr => "//",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn roundtrip(code: &str) -> String {
        let prog = parse(code).expect("parse");
        let mut buf = String::new();
        deparse_block_into(&mut buf, &prog.statements, 0);
        buf
    }

    #[test]
    fn deparse_simple_expr() {
        let s = roundtrip("1 + 2;");
        assert!(s.contains("1 + 2"));
    }

    #[test]
    fn deparse_sub_decl() {
        let s = roundtrip("sub foo { 42 }");
        assert!(s.contains("sub foo"));
        assert!(s.contains("42"));
    }

    #[test]
    fn deparse_anon_sub() {
        let s = roundtrip("my $f = sub { $_ + 1 };");
        assert!(s.contains("sub {"));
        assert!(s.contains("$_ + 1"));
    }
}
