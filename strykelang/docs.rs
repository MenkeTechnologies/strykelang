//! Module-doc generator — produces Markdown documentation from a
//! parsed stryke source file by pairing `## doc comments` with the
//! top-level declaration immediately below them.
//!
//! Invoked from the CLI as `stryke --docs FILE`. Public so other
//! tooling (e.g. a future workspace-wide doc generator) can reuse the
//! same logic.

use crate::ast::{Program, Statement, StmtKind, SubSigParam};

/// Emit Markdown for every top-level declaration in `program` that's
/// considered a public API surface (fn / struct / enum / class /
/// trait / package / `use constant`). Doc comments are extracted from
/// the SOURCE — consecutive `##` lines immediately above the
/// declaration line — so the parser/AST doesn't need to track them.
pub fn generate_markdown(filename: &str, source: &str, program: &Program) -> String {
    let source_lines: Vec<&str> = source.lines().collect();
    let module_title = derive_module_title(filename, program);

    let mut out = String::new();
    out.push_str("# Module: ");
    out.push_str(&module_title);
    out.push_str("\n\n");

    // Module-level header doc: any `##` block at the very top of the
    // file, before the first non-blank non-comment line.
    if let Some(header) = leading_module_doc(&source_lines) {
        out.push_str(&header);
        out.push_str("\n\n");
    }

    // Walk top-level statements, bucketing by category.
    let mut subs: Vec<&Statement> = Vec::new();
    let mut structs: Vec<&Statement> = Vec::new();
    let mut enums: Vec<&Statement> = Vec::new();
    let mut classes: Vec<&Statement> = Vec::new();
    let mut traits: Vec<&Statement> = Vec::new();
    let mut consts: Vec<&Statement> = Vec::new();
    let mut packages: Vec<&Statement> = Vec::new();

    for stmt in &program.statements {
        match &stmt.kind {
            StmtKind::SubDecl { .. } => subs.push(stmt),
            StmtKind::StructDecl { .. } => structs.push(stmt),
            StmtKind::EnumDecl { .. } => enums.push(stmt),
            StmtKind::ClassDecl { .. } => classes.push(stmt),
            StmtKind::TraitDecl { .. } => traits.push(stmt),
            StmtKind::Use { module, .. } if module == "constant" => consts.push(stmt),
            StmtKind::Package { .. } => packages.push(stmt),
            _ => {}
        }
    }

    if !packages.is_empty() {
        out.push_str("## Packages\n\n");
        for stmt in &packages {
            if let StmtKind::Package { name } = &stmt.kind {
                let doc = extract_doc_above(&source_lines, stmt.line);
                out.push_str(&format!("### `package {}`\n\n", name));
                if !doc.is_empty() {
                    out.push_str(&doc);
                    out.push_str("\n\n");
                }
            }
        }
    }

    if !consts.is_empty() {
        out.push_str("## Constants\n\n");
        for stmt in &consts {
            if let StmtKind::Use { imports, .. } = &stmt.kind {
                let doc = extract_doc_above(&source_lines, stmt.line);
                for name in extract_constant_names(imports) {
                    out.push_str(&format!("### `{}`\n\n", name));
                    if !doc.is_empty() {
                        out.push_str(&doc);
                        out.push_str("\n\n");
                    }
                }
            }
        }
    }

    if !traits.is_empty() {
        out.push_str("## Traits\n\n");
        for stmt in &traits {
            if let StmtKind::TraitDecl { def } = &stmt.kind {
                let doc = extract_doc_above(&source_lines, stmt.line);
                out.push_str(&format!("### `trait {}`\n\n", def.name));
                if !doc.is_empty() {
                    out.push_str(&doc);
                    out.push_str("\n\n");
                }
            }
        }
    }

    if !structs.is_empty() {
        out.push_str("## Structs\n\n");
        for stmt in &structs {
            if let StmtKind::StructDecl { def } = &stmt.kind {
                let doc = extract_doc_above(&source_lines, stmt.line);
                out.push_str(&format!("### `struct {}`\n\n", def.name));
                if !doc.is_empty() {
                    out.push_str(&doc);
                    out.push_str("\n\n");
                }
                if !def.fields.is_empty() {
                    out.push_str("Fields:\n");
                    for f in &def.fields {
                        out.push_str(&format!("- `{}`\n", f.name));
                    }
                    out.push('\n');
                }
            }
        }
    }

    if !enums.is_empty() {
        out.push_str("## Enums\n\n");
        for stmt in &enums {
            if let StmtKind::EnumDecl { def } = &stmt.kind {
                let doc = extract_doc_above(&source_lines, stmt.line);
                out.push_str(&format!("### `enum {}`\n\n", def.name));
                if !doc.is_empty() {
                    out.push_str(&doc);
                    out.push_str("\n\n");
                }
                if !def.variants.is_empty() {
                    out.push_str("Variants:\n");
                    for v in &def.variants {
                        out.push_str(&format!("- `{}`\n", v.name));
                    }
                    out.push('\n');
                }
            }
        }
    }

    if !classes.is_empty() {
        out.push_str("## Classes\n\n");
        for stmt in &classes {
            if let StmtKind::ClassDecl { def } = &stmt.kind {
                let doc = extract_doc_above(&source_lines, stmt.line);
                out.push_str(&format!("### `class {}`\n\n", def.name));
                if !doc.is_empty() {
                    out.push_str(&doc);
                    out.push_str("\n\n");
                }
                if !def.fields.is_empty() {
                    out.push_str("Fields:\n");
                    for f in &def.fields {
                        out.push_str(&format!("- `{}`\n", f.name));
                    }
                    out.push('\n');
                }
            }
        }
    }

    if !subs.is_empty() {
        out.push_str("## Subroutines\n\n");
        for stmt in &subs {
            if let StmtKind::SubDecl { name, params, .. } = &stmt.kind {
                let doc = extract_doc_above(&source_lines, stmt.line);
                let sig = format_sub_signature(name, params);
                out.push_str(&format!("### `fn {}`\n\n", sig));
                if !doc.is_empty() {
                    out.push_str(&doc);
                    out.push_str("\n\n");
                }
            }
        }
    }

    out
}

/// Pick a reasonable module title — the first `package Foo::Bar;`
/// declaration, or fall back to the file's basename.
fn derive_module_title(filename: &str, program: &Program) -> String {
    for stmt in &program.statements {
        if let StmtKind::Package { name } = &stmt.kind {
            return name.clone();
        }
    }
    std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_string()
}

/// Collect doc-comment text immediately above an AST line. Walks
/// upward from `decl_line - 1` (AST is 1-based; the line ABOVE the
/// decl is `decl_line - 1` in 1-based, or `decl_line - 2` in 0-based
/// indexing into `source_lines`). Stops at the first non-`##` line.
fn extract_doc_above(source_lines: &[&str], decl_line_1based: usize) -> String {
    if decl_line_1based < 2 {
        return String::new();
    }
    let mut collected: Vec<String> = Vec::new();
    let mut i = decl_line_1based.saturating_sub(2); // 0-based line above
    loop {
        let line = source_lines.get(i).copied().unwrap_or("");
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            collected.push(rest.to_string());
        } else if trimmed == "##" {
            collected.push(String::new());
        } else {
            break;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    collected.reverse();
    collected.join("\n")
}

/// Doc block at the very top of the file (before any code), used as
/// the module-level description.
fn leading_module_doc(source_lines: &[&str]) -> Option<String> {
    let mut collected: Vec<String> = Vec::new();
    let mut i = 0usize;
    // Skip shebang if present.
    if let Some(line) = source_lines.first() {
        if line.starts_with("#!") {
            i = 1;
        }
    }
    while i < source_lines.len() {
        let line = source_lines[i];
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            collected.push(rest.to_string());
        } else if trimmed == "##" {
            collected.push(String::new());
        } else if trimmed.is_empty() {
            // Blank line between leading doc and code — stop.
            if !collected.is_empty() {
                break;
            }
        } else {
            break;
        }
        i += 1;
    }
    if collected.is_empty() {
        None
    } else {
        Some(collected.join("\n"))
    }
}

/// Format a sub signature like `name($a, $b)` for the Markdown
/// heading. Falls back to bare `name` when there are no signature
/// params.
fn format_sub_signature(name: &str, params: &[SubSigParam]) -> String {
    if params.is_empty() {
        return name.to_string();
    }
    let parts: Vec<String> = params
        .iter()
        .map(|p| match p {
            SubSigParam::Scalar(n, _, _) => format!("${}", n),
            SubSigParam::Array(n, _) => format!("@{}", n),
            SubSigParam::Hash(n, _) => format!("%{}", n),
            SubSigParam::ArrayDestruct(_) => "[…]".to_string(),
            SubSigParam::HashDestruct(_) => "{…}".to_string(),
        })
        .collect();
    format!("{}({})", name, parts.join(", "))
}

/// Pull constant names out of `use constant`'s imports list — mirrors
/// `vm_helper::apply_use_constant`'s shape detection.
fn extract_constant_names(imports: &[crate::ast::Expr]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for imp in imports {
        match &imp.kind {
            crate::ast::ExprKind::List(items) => {
                let mut i = 0;
                while i + 1 < items.len() {
                    if let Some(n) = constant_name_of(&items[i]) {
                        names.push(n);
                    }
                    i += 2;
                }
            }
            crate::ast::ExprKind::HashRef(pairs) => {
                for (k, _) in pairs {
                    if let Some(n) = constant_name_of(k) {
                        names.push(n);
                    }
                }
            }
            _ => {
                if let Some(n) = constant_name_of(imp) {
                    names.push(n);
                }
            }
        }
    }
    names
}

fn constant_name_of(e: &crate::ast::Expr) -> Option<String> {
    match &e.kind {
        crate::ast::ExprKind::String(s) => Some(s.clone()),
        crate::ast::ExprKind::Bareword(s) => Some(s.clone()),
        _ => None,
    }
}
