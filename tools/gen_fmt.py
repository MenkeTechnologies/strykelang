#!/usr/bin/env python3
"""One-shot generator for src/fmt.rs from src/ast.rs. Run from repo root: python3 tools/gen_fmt.py"""
from pathlib import Path


def main() -> None:
    ast = Path("src/ast.rs").read_text()

    def extract_enum(name: str) -> str:
        i = ast.index(f"pub enum {name} {{")
        j = ast.index("{", i) + 1
        depth = 1
        k = j
        while depth:
            c = ast[k]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
            k += 1
        return ast[j : k - 1]

    def split_top_level_variants(body: str) -> list[str]:
        variants: list[str] = []
        buf: list[str] = []
        depth = 0
        for line in body.splitlines():
            s = line.strip()
            if not s or s.startswith("//"):
                continue
            if depth == 0 and s and (s[0].isupper() or s.startswith("///")):
                if buf and not buf[0].strip().startswith("///"):
                    variants.append("\n".join(buf))
                    buf = []
            buf.append(line)
            depth += line.count("{") - line.count("}")
        if buf:
            variants.append("\n".join(buf))
        return variants

    def parse_variant(v: str):
        lines = [l for l in v.splitlines() if l.strip() and not l.strip().startswith("//")]
        if not lines:
            return None, None
        first = lines[0].strip().rstrip(",")
        if "(" in first:
            name, rest = first.split("(", 1)
            name = name.strip()
            inner = rest.rstrip(")").rstrip(",")
            return "tuple", (name, inner)
        if "{" in first:
            name = first.split("{")[0].strip()
            return "struct", name
        return "unit", first.rstrip(",").strip()

    stmt_body = extract_enum("StmtKind")
    expr_body = extract_enum("ExprKind")
    stmt_variants = split_top_level_variants(stmt_body)
    expr_variants = split_top_level_variants(expr_body)

    def format_stmt_arm(v: str) -> str | None:
        kind, data = parse_variant(v)
        if kind is None:
            return None
        if kind == "unit":
            name = data
            if name == "Empty":
                return '        StmtKind::Empty => ";".to_string(),'
            return f'        StmtKind::{name} => "{name.lower().replace("_", " ")}".to_string(),'
        if kind == "tuple":
            name, _inner = data
            if name == "Expression":
                return '        StmtKind::Expression(e) => format!("{};", format_expr(e)),'
            if name == "Return":
                return """        StmtKind::Return(e) => e
                .as_ref()
                .map(|x| format!("return {};", format_expr(x)))
                .unwrap_or_else(|| "return;".to_string()),"""
            if name == "Begin":
                return '        StmtKind::Begin(b) => format!("BEGIN {{\\n{}\\n}}", format_block(b)),'
            if name == "UnitCheck":
                return '        StmtKind::UnitCheck(b) => format!("UNITCHECK {{\\n{}\\n}}", format_block(b)),'
            if name == "Check":
                return '        StmtKind::Check(b) => format!("CHECK {{\\n{}\\n}}", format_block(b)),'
            if name == "Init":
                return '        StmtKind::Init(b) => format!("INIT {{\\n{}\\n}}", format_block(b)),'
            if name == "End":
                return '        StmtKind::End(b) => format!("END {{\\n{}\\n}}", format_block(b)),'
            if name == "Continue":
                return '        StmtKind::Continue(b) => format!("continue {{\\n{}\\n}}", format_block(b)),'
            if name == "Last":
                return """        StmtKind::Last(l) => l
                .as_ref()
                .map(|x| format!("last {};", x))
                .unwrap_or_else(|| "last;".to_string()),"""
            if name == "Next":
                return """        StmtKind::Next(l) => l
                .as_ref()
                .map(|x| format!("next {};", x))
                .unwrap_or_else(|| "next;".to_string()),"""
            if name == "Redo":
                return """        StmtKind::Redo(l) => l
                .as_ref()
                .map(|x| format!("redo {};", x))
                .unwrap_or_else(|| "redo;".to_string()),"""
            if name == "My":
                return '        StmtKind::My(decls) => format!("my {};", format_var_decls(decls)),'
            if name == "Our":
                return '        StmtKind::Our(decls) => format!("our {};", format_var_decls(decls)),'
            if name == "Local":
                return '        StmtKind::Local(decls) => format!("local {};", format_var_decls(decls)),'
            if name == "MySync":
                return '        StmtKind::MySync(decls) => format!("mysync {};", format_var_decls(decls)),'
            if name == "Block":
                return '        StmtKind::Block(b) => format!("{{\\n{}\\n}}", format_block(b)),'
            return f'        StmtKind::{name}(_) => format!("/* unsupported StmtKind::{name} */"),'
        if kind == "struct":
            name = data
            if name == "If":
                return """        StmtKind::If {
                condition,
                body,
                elsifs,
                else_block,
            } => {
                let mut s = format!("if ({}) {{\\n{}\\n}}", format_expr(condition), format_block(body));
                for (c, b) in elsifs {
                    s.push_str(&format!(" elsif ({}) {{\\n{}\\n}}", format_expr(c), format_block(b)));
                }
                if let Some(eb) = else_block {
                    s.push_str(&format!(" else {{\\n{}\\n}}", format_block(eb)));
                }
                s
            },"""
            if name == "Unless":
                return """        StmtKind::Unless {
                condition,
                body,
                else_block,
            } => {
                let mut s = format!("unless ({}) {{\\n{}\\n}}", format_expr(condition), format_block(body));
                if let Some(eb) = else_block {
                    s.push_str(&format!(" else {{\\n{}\\n}}", format_block(eb)));
                }
                s
            },"""
            if name == "While":
                return """        StmtKind::While {
                condition,
                body,
                label,
                continue_block,
            } => {
                let lb = label.as_ref().map(|l| format!("{}: ", l)).unwrap_or_default();
                let mut s = format!("{}while ({}) {{\\n{}\\n}}", lb, format_expr(condition), format_block(body));
                if let Some(cb) = continue_block {
                    s.push_str(&format!(" continue {{\\n{}\\n}}", format_block(cb)));
                }
                s
            },"""
            if name == "Until":
                return """        StmtKind::Until {
                condition,
                body,
                label,
                continue_block,
            } => {
                let lb = label.as_ref().map(|l| format!("{}: ", l)).unwrap_or_default();
                let mut s = format!("{}until ({}) {{\\n{}\\n}}", lb, format_expr(condition), format_block(body));
                if let Some(cb) = continue_block {
                    s.push_str(&format!(" continue {{\\n{}\\n}}", format_block(cb)));
                }
                s
            },"""
            if name == "DoWhile":
                return """        StmtKind::DoWhile { body, condition } => {
                format!("do {{\\n{}\\n}} while ({})", format_block(body), format_expr(condition))
            },"""
            if name == "For":
                return """        StmtKind::For {
                init,
                condition,
                step,
                body,
                label,
                continue_block,
            } => {
                let lb = label.as_ref().map(|l| format!("{}: ", l)).unwrap_or_default();
                let ini = init.as_ref().map(|s| format_statement(s)).unwrap_or_default();
                let cond = condition.as_ref().map(|e| format_expr(e)).unwrap_or_default();
                let st = step.as_ref().map(|e| format_expr(e)).unwrap_or_default();
                let mut s = format!(
                    "{}for ({}; {}; {}) {{\\n{}\\n}}",
                    lb, ini, cond, st, format_block(body)
                );
                if let Some(cb) = continue_block {
                    s.push_str(&format!(" continue {{\\n{}\\n}}", format_block(cb)));
                }
                s
            },"""
            if name == "Foreach":
                return """        StmtKind::Foreach {
                var,
                list,
                body,
                label,
                continue_block,
            } => {
                let lb = label.as_ref().map(|l| format!("{}: ", l)).unwrap_or_default();
                let mut s = format!(
                    "{}foreach ${} ({}) {{\\n{}\\n}}",
                    lb, var, format_expr(list), format_block(body)
                );
                if let Some(cb) = continue_block {
                    s.push_str(&format!(" continue {{\\n{}\\n}}", format_block(cb)));
                }
                s
            },"""
            if name == "SubDecl":
                return """        StmtKind::SubDecl {
                name,
                params: _params,
                body,
                prototype,
            } => {
                let proto = prototype.as_ref().map(|p| format!(" ({})", p)).unwrap_or_default();
                format!("fn {}{} {{\\n{}\\n}}", name, proto, format_block(body))
            },"""
            if name == "Package":
                return '        StmtKind::Package { name } => format!("package {};", name),'
            if name == "Use":
                return """        StmtKind::Use { module, imports } => {
                if imports.is_empty() {
                    format!("use {};", module)
                } else {
                    format!("use {} {};", module, format_expr_list(imports))
                }
            },"""
            if name == "No":
                return """        StmtKind::No { module, imports } => {
                if imports.is_empty() {
                    format!("no {};", module)
                } else {
                    format!("no {} {};", module, format_expr_list(imports))
                }
            },"""
            if name == "Last":
                return """        StmtKind::Last(l) => l
                .as_ref()
                .map(|x| format!("last {};", x))
                .unwrap_or_else(|| "last;".to_string()),"""
            if name == "Next":
                return """        StmtKind::Next(l) => l
                .as_ref()
                .map(|x| format!("next {};", x))
                .unwrap_or_else(|| "next;".to_string()),"""
            if name == "Redo":
                return """        StmtKind::Redo(l) => l
                .as_ref()
                .map(|x| format!("redo {};", x))
                .unwrap_or_else(|| "redo;".to_string()),"""
            if name == "My":
                return '        StmtKind::My(decls) => format!("my {};", format_var_decls(decls)),'
            if name == "Our":
                return '        StmtKind::Our(decls) => format!("our {};", format_var_decls(decls)),'
            if name == "Local":
                return '        StmtKind::Local(decls) => format!("local {};", format_var_decls(decls)),'
            if name == "MySync":
                return '        StmtKind::MySync(decls) => format!("mysync {};", format_var_decls(decls)),'
            if name == "Block":
                return '        StmtKind::Block(b) => format!("{{\\n{}\\n}}", format_block(b)),'
            if name == "Goto":
                return '        StmtKind::Goto { target } => format!("goto {};", format_expr(target)),'
            if name == "StructDecl":
                return """        StmtKind::StructDecl { def } => {
                let fields = def
                    .fields
                    .iter()
                    .map(|(n, t)| format!("{} => {:?}", n, t))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("struct {} {{ {} }}", def.name, fields)
            },"""
            if name == "EvalTimeout":
                return """        StmtKind::EvalTimeout { timeout, body } => {
                format!("eval_timeout {} {{\\n{}\\n}}", format_expr(timeout), format_block(body))
            },"""
            if name == "TryCatch":
                return """        StmtKind::TryCatch {
                try_block,
                catch_var,
                catch_block,
                finally_block,
            } => {
                let fin = finally_block
                    .as_ref()
                    .map(|b| format!("\\nfinally {{\\n{}\\n}}", format_block(b)))
                    .unwrap_or_default();
                format!(
                    "try {{\\n{}\\n}} catch (${}) {{\\n{}\\n}}{}",
                    format_block(try_block),
                    catch_var,
                    format_block(catch_block),
                    fin
                )
            },"""
            if name == "Given":
                return """        StmtKind::Given { topic, body } => {
                format!("given ({}) {{\\n{}\\n}}", format_expr(topic), format_block(body))
            },"""
            if name == "When":
                return """        StmtKind::When { cond, body } => {
                format!("when ({}) {{\\n{}\\n}}", format_expr(cond), format_block(body))
            },"""
            if name == "DefaultCase":
                return '        StmtKind::DefaultCase { body } => format!("default {{\\n{}\\n}}", format_block(body)),'
            return f'        StmtKind::{name} {{ .. }} => format!("/* StmtKind::{name} */"),'

    out: list[str] = []
    out.append("//! Pretty-print parsed Perl back to source (`st --fmt`).")
    out.append("//! Regenerate with `python3 tools/gen_fmt.py` after `ast.rs` changes.")
    out.append("")
    out.append("#![allow(unused_variables)] // generated `match` arms name fields not always used")
    out.append("")
    out.append("use crate::ast::*;")
    out.append("")
    out.append("/// Format a whole program as Perl-like source.")
    out.append("pub fn format_program(p: &Program) -> String {")
    out.append("    p.statements")
    out.append("        .iter()")
    out.append("        .map(|s| format_statement(s))")
    out.append("        .collect::<Vec<_>>()")
    out.append('        .join("\\n")')
    out.append("}")
    out.append("")
    out.append("fn format_statement(s: &Statement) -> String {")
    out.append('    let lab = s.label.as_ref().map(|l| format!("{}: ", l)).unwrap_or_default();')
    out.append("    let body = match &s.kind {")
    for v in stmt_variants:
        arm = format_stmt_arm(v)
        if arm:
            out.append(arm)
    out.append("    };")
    out.append('    format!("{}{}", lab, body)')
    out.append("}")
    out.append("")
    out.append("fn format_block(b: &Block) -> String {")
    out.append('    b.iter().map(format_statement).collect::<Vec<_>>().join("\\n")')
    out.append("}")
    out.append("")
    out.append("fn format_var_decls(decls: &[VarDecl]) -> String {")
    out.append("    decls")
    out.append("        .iter()")
    out.append("        .map(|d| {")
    out.append("            let sig = match d.sigil {")
    out.append('                Sigil::Scalar => "$",')
    out.append('                Sigil::Array => "@",')
    out.append('                Sigil::Hash => "%",')
    out.append("            };")
    out.append('            let mut s = format!("{}{}", sig, d.name);')
    out.append("            if let Some(t) = d.type_annotation {")
    out.append('                s.push_str(&format!(" : {:?}", t));')
    out.append("            }")
    out.append("            if let Some(ref init) = d.initializer {")
    out.append('                s.push_str(&format!(" = {}", format_expr(init)));')
    out.append("            }")
    out.append("            s")
    out.append("        })")
    out.append('        .collect::<Vec<_>>()')
    out.append('        .join(", ")')
    out.append("}")
    out.append("")
    out.append("fn format_expr_list(es: &[Expr]) -> String {")
    out.append("    es.iter().map(format_expr).collect::<Vec<_>>().join(\", \")")
    out.append("}")
    out.append("")

    def binop_rust_to_perl(name: str) -> str:
        m = {
            "Add": "+",
            "Sub": "-",
            "Mul": "*",
            "Div": "/",
            "Mod": "%",
            "Pow": "**",
            "Concat": ".",
            "NumEq": "==",
            "NumNe": "!=",
            "NumLt": "<",
            "NumGt": ">",
            "NumLe": "<=",
            "NumGe": ">=",
            "Spaceship": "<=>",
            "StrEq": "eq",
            "StrNe": "ne",
            "StrLt": "lt",
            "StrGt": "gt",
            "StrLe": "le",
            "StrGe": "ge",
            "StrCmp": "cmp",
            "LogAnd": "&&",
            "LogOr": "||",
            "DefinedOr": "//",
            "BitAnd": "&",
            "BitOr": "|",
            "BitXor": "^",
            "ShiftLeft": "<<",
            "ShiftRight": ">>",
            "LogAndWord": "and",
            "LogOrWord": "or",
            "BindMatch": "=~",
            "BindNotMatch": "!~",
        }
        return m.get(name, "?")

    bin_out = ["fn format_binop(op: BinOp) -> &'static str {", "    match op {"]
    for line in extract_enum("BinOp").splitlines():
        t = line.strip().rstrip(",")
        if not t or t.startswith("//") or "(" in t:
            continue
        name = t.strip()
        bin_out.append(f'        BinOp::{name} => "{binop_rust_to_perl(name)}",')
    bin_out += ["    }", "}"]
    out.extend(bin_out)
    out.append("")

    un_out = ["fn format_unary(op: UnaryOp) -> &'static str {", "    match op {"]
    um = {
        "Negate": "-",
        "LogNot": "!",
        "BitNot": "~",
        "LogNotWord": "not",
        "PreIncrement": "++",
        "PreDecrement": "--",
    }
    for line in extract_enum("UnaryOp").splitlines():
        t = line.strip().rstrip(",")
        if not t or t.startswith("//") or "(" in t:
            continue
        name = t.strip()
        if name == "Ref":
            un_out.append('        UnaryOp::Ref => "\\\\",')
        else:
            un_out.append(f'        UnaryOp::{name} => "{um.get(name, "?")}",')
    un_out += ["    }", "}"]
    out.extend(un_out)
    out.append("")

    po_out = ["fn format_postfix(op: PostfixOp) -> &'static str {", "    match op {"]
    pm = {"Increment": "++", "Decrement": "--"}
    for line in extract_enum("PostfixOp").splitlines():
        t = line.strip().rstrip(",")
        if not t or t.startswith("//") or "(" in t:
            continue
        name = t.strip()
        po_out.append(f'        PostfixOp::{name} => "{pm.get(name, "?")}",')
    po_out += ["    }", "}"]
    out.extend(po_out)
    out.append("")

    # Rust format! patterns: `$` must not start a Python `{{` escape; build with chr(36) + pieces.
    _fmt_hash_el = (
        'format!("'
        + chr(36)
        + "{}"
        + "{{"
        + "{}"
        + "}}"
        + '", hash, format_expr(key))'
    )
    _fmt_deref_scalar = (
        'format!("' + chr(36) + "{{" + "{}" + "}}" + '", format_expr(expr))'
    )
    _fmt_deref_arr = 'format!("@{{${}}}", format_expr(expr))'
    _fmt_deref_hash = 'format!("%{{${}}}", format_expr(expr))'
    _fmt_arrow_hash = (
        'format!("({})->' + "{{" + "{}" + "}}" + '", format_expr(expr), format_expr(index))'
    )

    out += [
        "fn format_string_part(p: &StringPart) -> String {",
        "    match p {",
        "        StringPart::Literal(s) => s.clone(),",
        "        StringPart::ScalarVar(n) => format!(\"${{{}}}\", n),",
        "        StringPart::ArrayVar(n) => format!(\"@{{{}}}\", n),",
        "        StringPart::Expr(e) => format_expr(e),",
        "    }",
        "}",
        "",
        "fn format_string_literal(s: &str) -> String {",
        "    let mut out = String::new();",
        "    out.push('\"');",
        "    for c in s.chars() {",
        "        match c {",
        "            '\\\\' => out.push_str(\"\\\\\\\\\"),",
        "            '\"' => out.push_str(\"\\\\\\\"\"),",
        "            '\\n' => out.push_str(\"\\\\n\"),",
        "            '\\r' => out.push_str(\"\\\\r\"),",
        "            '\\t' => out.push_str(\"\\\\t\"),",
        "            _ => out.push(c),",
        "        }",
        "    }",
        "    out.push('\"');",
        "    out",
        "}",
        "",
        "/// Format an expression; aims for readable Perl-like output.",
        "pub fn format_expr(e: &Expr) -> String {",
        "    match &e.kind {",
    ]

    struct_exprs = {
        "ArrayElement": 'format!("${}[{}]", array, format_expr(index))',
        "HashElement": _fmt_hash_el,
        "ArraySlice": 'format!("@{}[{}]", array, indices.iter().map(format_expr).collect::<Vec<_>>().join(", "))',
        "HashSlice": 'format!("@{}{{{}}}", hash, keys.iter().map(format_expr).collect::<Vec<_>>().join(", "))',
        "ScalarRef": 'format!("\\\\{}", format_expr(expr))',
        "ArrayRef": 'format!("[{}]", es.iter().map(format_expr).collect::<Vec<_>>().join(", "))',
        "HashRef": 'format!("({})", pairs.iter().map(|(k,v)| format!("{} => {}", format_expr(k), format_expr(v))).collect::<Vec<_>>().join(", "))',
        "CodeRef": 'format!("sub {{\\n{}\\n}}", format_block(body))',
        "Deref": f"""match kind {{
                Sigil::Scalar => {_fmt_deref_scalar},
                Sigil::Array => {_fmt_deref_arr},
                Sigil::Hash => {_fmt_deref_hash},
            }}""",
        "ArrowDeref": f"""match kind {{
                DerefKind::Array => format!("({{}})->[{{}}]", format_expr(expr), format_expr(index)),
                DerefKind::Hash => {_fmt_arrow_hash},
                DerefKind::Call => format!("({{}})->({{}})", format_expr(expr), format_expr(index)),
            }}""",
        "BinOp": 'format!("({} {} {})", format_expr(left), format_binop(*op), format_expr(right))',
        "UnaryOp": 'format!("({}{})", format_unary(*op), format_expr(expr))',
        "PostfixOp": 'format!("({}{})", format_expr(expr), format_postfix(*op))',
        "Assign": 'format!("{} = {}", format_expr(target), format_expr(value))',
        "CompoundAssign": 'format!("{} {}= {}", format_expr(target), format_binop(*op), format_expr(value))',
        "Ternary": 'format!("({} ? {} : {})", format_expr(condition), format_expr(then_expr), format_expr(else_expr))',
        "Repeat": 'format!("({} x {})", format_expr(expr), format_expr(count))',
        "Range": 'format!("({} .. {})", format_expr(from), format_expr(to))',
        "FuncCall": 'format!("{}({})", name, args.iter().map(format_expr).collect::<Vec<_>>().join(", "))',
        "MethodCall": 'format!("{}->{}({})", format_expr(object), method, args.iter().map(format_expr).collect::<Vec<_>>().join(", "))',
        "Print": """{
                let mut s = String::new();
                if let Some(h) = handle {
                    s.push_str(h);
                    s.push_str(": ");
                }
                s.push_str("print ");
                s.push_str(&format_expr_list(args));
                s
            }""",
        "Say": """{
                let mut s = String::new();
                if let Some(h) = handle {
                    s.push_str(h);
                    s.push_str(": ");
                }
                s.push_str("say ");
                s.push_str(&format_expr_list(args));
                s
            }""",
        "Printf": """{
                let mut s = String::new();
                if let Some(h) = handle {
                    s.push_str(h);
                    s.push_str(": ");
                }
                s.push_str("printf ");
                s.push_str(&format_expr_list(args));
                s
            }""",
        "Die": 'format!("die({})", format_expr_list(args))',
        "Warn": 'format!("warn({})", format_expr_list(args))',
        "Match": 'format!("({} =~ /{}/{})", format_expr(expr), pattern, flags)',
        "Substitution": 'format!("({} =~ s/{}/{}/{})", format_expr(expr), pattern, replacement, flags)',
        "Transliterate": 'format!("({} =~ tr/{}/{}/{})", format_expr(expr), from, to, flags)',
        "MapExpr": """{
            let kw = if *flatten_array_refs { "flat_map" } else { "map" };
            format!("{} {{\\n{}\\n}} {}", kw, format_block(block), format_expr(list))
        }""",
        "MapExprComma": """{
            let kw = if *flatten_array_refs { "flat_map" } else { "map" };
            format!("{}, {}, {}", kw, format_expr(expr), format_expr(list))
        }""",
        "GrepExpr": 'format!("grep {{\\n{}\\n}} {}", format_block(block), format_expr(list))',
        "SortExpr": """match cmp {
                Some(crate::ast::SortComparator::Block(b)) => {
                    format!("sort {{\\n{}\\n}} {}", format_block(b), format_expr(list))
                }
                Some(crate::ast::SortComparator::Code(e)) => {
                    format!("sort {} {}", format_expr(e), format_expr(list))
                }
                None => format!("sort {}", format_expr(list)),
            }""",
        "JoinExpr": 'format!("join({}, {})", format_expr(separator), format_expr(list))',
        "SplitExpr": """match limit {
                Some(l) => format!("split({}, {}, {})", format_expr(pattern), format_expr(string), format_expr(l)),
                None => format!("split({}, {})", format_expr(pattern), format_expr(string)),
            }""",
        "PMapExpr": """{
                let kw = if *flat_outputs { "pflat_map" } else { "pmap" };
                let base = format!(
                    "{}{{\\n{{}}\\n}} {{}}",
                    kw,
                    format_block(block),
                    format_expr(list)
                );
                match progress {
                    Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                    None => base,
                }
            }""",
        "PMapChunkedExpr": """match progress {
                Some(p) => format!(
                    "pmap_chunked {} {{\\n{}\\n}} {}, progress => {}",
                    format_expr(chunk_size),
                    format_block(block),
                    format_expr(list),
                    format_expr(p)
                ),
                None => format!(
                    "pmap_chunked {} {{\\n{}\\n}} {}",
                    format_expr(chunk_size),
                    format_block(block),
                    format_expr(list)
                ),
            }""",
        "PGrepExpr": """match progress {
                Some(p) => format!(
                    "pgrep {{\\n{}\\n}} {}, progress => {}",
                    format_block(block),
                    format_expr(list),
                    format_expr(p)
                ),
                None => format!("pgrep {{\\n{}\\n}} {}", format_block(block), format_expr(list)),
            }""",
        "PForExpr": """match progress {
                Some(p) => format!(
                    "pfor {{\\n{}\\n}} {}, progress => {}",
                    format_block(block),
                    format_expr(list),
                    format_expr(p)
                ),
                None => format!("pfor {{\\n{}\\n}} {}", format_block(block), format_expr(list)),
            }""",
        "ParLinesExpr": """match progress {
                Some(p) => format!(
                    "par_lines({}, {}, progress => {})",
                    format_expr(path),
                    format_expr(callback),
                    format_expr(p)
                ),
                None => format!("par_lines({}, {})", format_expr(path), format_expr(callback)),
            }""",
        "ParWalkExpr": """match progress {
                Some(p) => format!(
                    "par_walk({}, {}, progress => {})",
                    format_expr(path),
                    format_expr(callback),
                    format_expr(p)
                ),
                None => format!("par_walk({}, {})", format_expr(path), format_expr(callback)),
            }""",
        "PwatchExpr": 'format!("pwatch({}, {})", format_expr(path), format_expr(callback))',
        "PSortExpr": """match (cmp, progress) {
                (Some(b), Some(p)) => format!(
                    "psort {{\\n{}\\n}} {}, progress => {}",
                    format_block(b),
                    format_expr(list),
                    format_expr(p)
                ),
                (Some(b), None) => format!("psort {{\\n{}\\n}} {}", format_block(b), format_expr(list)),
                (None, Some(p)) => format!("psort {}, progress => {}", format_expr(list), format_expr(p)),
                (None, None) => format!("psort {}", format_expr(list)),
            }""",
        "PcacheExpr": """match progress {
                Some(p) => format!(
                    "pcache {{\\n{}\\n}} {}, progress => {}",
                    format_block(block),
                    format_expr(list),
                    format_expr(p)
                ),
                None => format!("pcache {{\\n{}\\n}} {}", format_block(block), format_expr(list)),
            }""",
        "ReduceExpr": 'format!("reduce {{\\n{}\\n}} {}", format_block(block), format_expr(list))',
        "PReduceExpr": 'format!("preduce {{\\n{}\\n}} {}", format_block(block), format_expr(list))',
        "PReduceInitExpr": 'format!("preduce_init {}, {{\\n{}\\n}} {}", format_expr(init), format_block(block), format_expr(list))',
        "FanExpr": """{
            let kw = if *capture { "fan_cap" } else { "fan" };
            let base = match count {
                Some(c) => format!("{} {} {{\\n{}\\n}}", kw, format_expr(c), format_block(block)),
                None => format!("{} {{\\n{}\\n}}", kw, format_block(block)),
            };
            match progress {
                Some(p) => format!("{}, progress => {}", base, format_expr(p)),
                None => base,
            }
        }""",
        "AsyncBlock": 'format!("async {{\\n{}\\n}}", format_block(body))',
        "SpawnBlock": 'format!("spawn {{\\n{}\\n}}", format_block(body))',
        "Trace": 'format!("trace {{\\n{}\\n}}", format_block(body))',
        "Timer": 'format!("timer {{\\n{}\\n}}", format_block(body))',
        "Push": 'format!("push({}, {})", format_expr(array), format_expr_list(values))',
        "Unshift": 'format!("unshift({}, {})", format_expr(array), format_expr_list(values))',
        "Splice": 'format!("splice({}, ...)", format_expr(array))',
        "Substr": 'format!("substr({}, ...)", format_expr(string))',
        "Index": 'format!("index({}, {})", format_expr(string), format_expr(substr))',
        "Rindex": 'format!("rindex({}, {})", format_expr(string), format_expr(substr))',
        "Sprintf": 'format!("sprintf({}, {})", format_expr(format), format_expr_list(args))',
        "Atan2": 'format!("atan2({}, {})", format_expr(y), format_expr(x))',
        "Rand": """match rand {
                Some(e) => format!("rand({})", format_expr(e)),
                None => "rand()".to_string(),
            }""",
        "Srand": """match srand {
                Some(e) => format!("srand({})", format_expr(e)),
                None => "srand()".to_string(),
            }""",
        "Crypt": 'format!("crypt({}, {})", format_expr(plaintext), format_expr(salt))',
        "Pos": """match pos {
                Some(e) => format!("pos({})", format_expr(e)),
                None => "pos".to_string(),
            }""",
        "Open": 'format!("open({}, {}, ...)", format_expr(handle), format_expr(mode))',
        "ReadLine": """match handle {
                Some(h) => format!("<{}>", h),
                None => "<>".to_string(),
            }""",
        "Eof": """match expr {
                Some(e) => format!("eof({})", format_expr(e)),
                None => "eof()".to_string(),
            }""",
        "Opendir": 'format!("opendir({}, {})", format_expr(handle), format_expr(path))',
        "Seekdir": 'format!("seekdir({}, {})", format_expr(handle), format_expr(position))',
        "FileTest": 'format!("-{}{}", op, format_expr(expr))',
        "System": 'format!("system({})", format_expr_list(args))',
        "Exec": 'format!("exec({})", format_expr_list(args))',
        "Eval": 'format!("eval({})", format_expr(expr))',
        "Do": 'format!("do {}", format_expr(expr))',
        "Require": 'format!("require {}", format_expr(expr))',
        "Exit": """match exit {
                Some(e) => format!("exit({})", format_expr(e)),
                None => "exit()".to_string(),
            }""",
        "Chdir": 'format!("chdir {}", format_expr(expr))',
        "Mkdir": 'format!("mkdir({}, ...)", format_expr(path))',
        "Unlink": 'format!("unlink({})", format_expr_list(args))',
        "Rename": 'format!("rename({}, {})", format_expr(old), format_expr(new))',
        "Chmod": 'format!("chmod(...)", )',
        "Chown": 'format!("chown(...)", )',
        "Link": 'format!("link({}, {})", format_expr(old), format_expr(new))',
        "Symlink": 'format!("symlink({}, {})", format_expr(old), format_expr(new))',
        "Glob": 'format!("glob({})", format_expr_list(args))',
        "GlobPar": """match progress {
                Some(p) => format!("glob_par({}), progress => {}", format_expr_list(args), format_expr(p)),
                None => format!("glob_par({})", format_expr_list(args)),
            }""",
        "ParSed": """match progress {
                Some(p) => format!("par_sed({}), progress => {}", format_expr_list(args), format_expr(p)),
                None => format!("par_sed({})", format_expr_list(args)),
            }""",
        "Bless": """match class {
                Some(c) => format!("bless({}, {})", format_expr(ref_expr), format_expr(c)),
                None => format!("bless({})", format_expr(ref_expr)),
            }""",
        "Caller": """match expr {
                Some(e) => format!("caller({})", format_expr(e)),
                None => "caller()".to_string(),
            }""",
        "List": 'format!("({})", es.iter().map(format_expr).collect::<Vec<_>>().join(", "))',
        "PostfixIf": 'format!("{} if {}", format_expr(expr), format_expr(condition))',
        "PostfixUnless": 'format!("{} unless {}", format_expr(expr), format_expr(condition))',
        "PostfixWhile": 'format!("{} while {}", format_expr(expr), format_expr(condition))',
        "PostfixUntil": 'format!("{} until {}", format_expr(expr), format_expr(condition))',
        "PostfixForeach": 'format!("{} foreach {}", format_expr(expr), format_expr(list))',
    }

    for v in expr_variants:
        kind, data = parse_variant(v)
        if kind == "unit":
            name = data
            if name == "Pchannel":
                out.append('        ExprKind::Pchannel => "pchannel()".to_string(),')
            elif name == "Wantarray":
                out.append('        ExprKind::Wantarray => "wantarray".to_string(),')
            else:
                out.append(f'        ExprKind::{name} => "{name.lower()}".to_string(),')
        elif kind == "tuple":
            name, _inner = data
            simple = {
                "Integer": '        ExprKind::Integer(n) => n.to_string(),',
                "Float": '        ExprKind::Float(f) => format!("{}", f),',
                "String": '        ExprKind::String(s) => format_string_literal(s),',
                "Bareword": '        ExprKind::Bareword(s) => s.clone(),',
                "Regex": '        ExprKind::Regex(p, fl) => format!("/{}/{}/", p, fl),',
                "QW": '        ExprKind::QW(ws) => format!("qw({})", ws.join(" ")),',
                "Undef": '        ExprKind::Undef => "undef".to_string(),',
                "ReverseExpr": '        ExprKind::ReverseExpr(e) => format!("reverse {}", format_expr(e)),',
                "Pop": '        ExprKind::Pop(e) => format!("pop {}", format_expr(e)),',
                "Shift": '        ExprKind::Shift(e) => format!("shift {}", format_expr(e)),',
                "Delete": '        ExprKind::Delete(e) => format!("delete {}", format_expr(e)),',
                "Exists": '        ExprKind::Exists(e) => format!("exists {}", format_expr(e)),',
                "Keys": '        ExprKind::Keys(e) => format!("keys {}", format_expr(e)),',
                "Values": '        ExprKind::Values(e) => format!("values {}", format_expr(e)),',
                "Each": '        ExprKind::Each(e) => format!("each {}", format_expr(e)),',
                "Chomp": '        ExprKind::Chomp(e) => format!("chomp {}", format_expr(e)),',
                "Chop": '        ExprKind::Chop(e) => format!("chop {}", format_expr(e)),',
                "Length": '        ExprKind::Length(e) => format!("length {}", format_expr(e)),',
                "Abs": '        ExprKind::Abs(e) => format!("abs {}", format_expr(e)),',
                "Int": '        ExprKind::Int(e) => format!("int {}", format_expr(e)),',
                "Sqrt": '        ExprKind::Sqrt(e) => format!("sqrt {}", format_expr(e)),',
                "Sin": '        ExprKind::Sin(e) => format!("sin {}", format_expr(e)),',
                "Cos": '        ExprKind::Cos(e) => format!("cos {}", format_expr(e)),',
                "Exp": '        ExprKind::Exp(e) => format!("exp {}", format_expr(e)),',
                "Log": '        ExprKind::Log(e) => format!("log {}", format_expr(e)),',
                "Hex": '        ExprKind::Hex(e) => format!("hex {}", format_expr(e)),',
                "Oct": '        ExprKind::Oct(e) => format!("oct {}", format_expr(e)),',
                "Lc": '        ExprKind::Lc(e) => format!("lc {}", format_expr(e)),',
                "Uc": '        ExprKind::Uc(e) => format!("uc {}", format_expr(e)),',
                "Lcfirst": '        ExprKind::Lcfirst(e) => format!("lcfirst {}", format_expr(e)),',
                "Ucfirst": '        ExprKind::Ucfirst(e) => format!("ucfirst {}", format_expr(e)),',
                "Fc": '        ExprKind::Fc(e) => format!("fc {}", format_expr(e)),',
                "Defined": '        ExprKind::Defined(e) => format!("defined {}", format_expr(e)),',
                "Ref": '        ExprKind::Ref(e) => format!("ref {}", format_expr(e)),',
                "ScalarContext": '        ExprKind::ScalarContext(e) => format!("scalar {}", format_expr(e)),',
                "Chr": '        ExprKind::Chr(e) => format!("chr {}", format_expr(e)),',
                "Ord": '        ExprKind::Ord(e) => format!("ord {}", format_expr(e)),',
                "Close": '        ExprKind::Close(e) => format!("close {}", format_expr(e)),',
                "Readdir": '        ExprKind::Readdir(e) => format!("readdir {}", format_expr(e)),',
                "Closedir": '        ExprKind::Closedir(e) => format!("closedir {}", format_expr(e)),',
                "Rewinddir": '        ExprKind::Rewinddir(e) => format!("rewinddir {}", format_expr(e)),',
                "Telldir": '        ExprKind::Telldir(e) => format!("telldir {}", format_expr(e)),',
                "Readlink": '        ExprKind::Readlink(e) => format!("readlink {}", format_expr(e)),',
                "Stat": '        ExprKind::Stat(e) => format!("stat {}", format_expr(e)),',
                "Lstat": '        ExprKind::Lstat(e) => format!("lstat {}", format_expr(e)),',
                "Slurp": '        ExprKind::Slurp(e) => format!("slurp {}", format_expr(e)),',
                "Capture": '        ExprKind::Capture(e) => format!("capture {}", format_expr(e)),',
                "FetchUrl": '        ExprKind::FetchUrl(e) => format!("fetch_url {}", format_expr(e)),',
                "Await": '        ExprKind::Await(e) => format!("await {}", format_expr(e)),',
                "Study": '        ExprKind::Study(e) => format!("study {}", format_expr(e)),',
                "ScalarVar": '        ExprKind::ScalarVar(name) => format!("${}", name),',
                "ArrayVar": '        ExprKind::ArrayVar(name) => format!("@{}", name),',
                "HashVar": '        ExprKind::HashVar(name) => format!("%{}", name),',
                "InterpolatedString": "        ExprKind::InterpolatedString(parts) => parts.iter().map(format_string_part).collect::<String>(),",
            }
            if name in simple:
                out.append(simple[name])
            else:
                out.append(f'        ExprKind::{name}(_) => format!("/* ExprKind::{name} */"),')
        elif kind == "struct":
            name = data
            if name not in struct_exprs:
                raise SystemExit(f"Missing struct_exprs for {name}")
            raw_inner = v[v.index("{") + 1 : v.rindex("}")].strip()
            field_lines = []
            for line in raw_inner.splitlines():
                line = line.strip().rstrip(",")
                if not line or line.startswith("//"):
                    continue
                field_lines.append(line.split(":")[0].strip())
            inner = ",\n            ".join(field_lines)
            out.append(f"        ExprKind::{name} {{")
            out.append(f"            {inner}")
            out.append(f"        }} => {struct_exprs[name]},")

    out.append("    }")
    out.append("}")
    out.append("")

    Path("src/fmt.rs").write_text("\n".join(out))
    print("Wrote src/fmt.rs", len(out), "lines")


if __name__ == "__main__":
    main()
