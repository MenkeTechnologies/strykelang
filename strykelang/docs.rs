//! Module-doc generator — produces Markdown documentation from a
//! parsed stryke source file by pairing `## doc comments` with the
//! top-level declaration immediately below them.
//!
//! Driven by the project-wide CLI subcommand `stryke gen-docs
//! [PATH] [--out DIR]`, which walks a directory tree and calls
//! [`generate_markdown`] once per source file.

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expr, ExprKind, Statement, StmtKind};

    fn expr(kind: ExprKind) -> Expr {
        Expr { kind, line: 1 }
    }

    fn pkg_stmt(name: &str) -> Statement {
        Statement::new(
            StmtKind::Package {
                name: name.to_string(),
            },
            1,
        )
    }

    // ─── format_sub_signature ────────────────────────────────────────────

    #[test]
    fn format_sub_signature_no_params_returns_bare_name() {
        assert_eq!(format_sub_signature("foo", &[]), "foo");
    }

    #[test]
    fn format_sub_signature_scalar_array_hash_sigils() {
        let params = vec![
            SubSigParam::Scalar("a".into(), None, None),
            SubSigParam::Array("xs".into(), None),
            SubSigParam::Hash("h".into(), None),
        ];
        assert_eq!(format_sub_signature("f", &params), "f($a, @xs, %h)");
    }

    #[test]
    fn format_sub_signature_destructure_placeholders() {
        let params = vec![
            SubSigParam::ArrayDestruct(vec![]),
            SubSigParam::HashDestruct(vec![]),
        ];
        // Destructure params are rendered as ellipsis placeholders.
        assert_eq!(format_sub_signature("g", &params), "g([…], {…})");
    }

    // ─── derive_module_title ─────────────────────────────────────────────

    #[test]
    fn derive_module_title_prefers_first_package_declaration() {
        let prog = Program {
            statements: vec![pkg_stmt("My::Mod"), pkg_stmt("Other")],
        };
        assert_eq!(derive_module_title("/tmp/x.stk", &prog), "My::Mod");
    }

    #[test]
    fn derive_module_title_falls_back_to_file_stem() {
        let prog = Program { statements: vec![] };
        assert_eq!(derive_module_title("/tmp/my_file.stk", &prog), "my_file");
    }

    #[test]
    fn derive_module_title_no_extension_uses_full_basename() {
        let prog = Program { statements: vec![] };
        assert_eq!(derive_module_title("/tmp/Makefile", &prog), "Makefile");
    }

    // ─── extract_doc_above ───────────────────────────────────────────────

    #[test]
    fn extract_doc_above_picks_up_consecutive_hash_hash_lines() {
        let src = vec!["## line one", "## line two", "fn foo {}"];
        // decl is on line 3 (1-based) → walks up from line 2.
        let r = extract_doc_above(&src, 3);
        assert_eq!(r, "line one\nline two");
    }

    #[test]
    fn extract_doc_above_stops_at_non_doc_line() {
        let src = vec!["## kept", "fn bar {}", "## not for next", "fn foo {}"];
        let r = extract_doc_above(&src, 4);
        // Line 3 is "## not for next", line 2 is `fn bar {}` → only line 3 collected.
        assert_eq!(r, "not for next");
    }

    #[test]
    fn extract_doc_above_returns_empty_when_decl_is_line_one() {
        // Nothing above line 1; helper guards against decl_line < 2.
        assert_eq!(extract_doc_above(&["fn foo {}"], 1), "");
    }

    #[test]
    fn extract_doc_above_bare_double_hash_yields_blank_line() {
        let src = vec!["## first", "##", "## third", "fn foo {}"];
        let r = extract_doc_above(&src, 4);
        assert_eq!(r, "first\n\nthird");
    }

    // ─── leading_module_doc ──────────────────────────────────────────────

    #[test]
    fn leading_module_doc_collects_top_block() {
        let src = vec!["## module doc", "## second line", "", "fn x {}"];
        assert_eq!(
            leading_module_doc(&src),
            Some("module doc\nsecond line".into())
        );
    }

    #[test]
    fn leading_module_doc_skips_shebang() {
        let src = vec!["#!/usr/bin/env stryke", "## after shebang", "", "fn x {}"];
        assert_eq!(leading_module_doc(&src), Some("after shebang".into()));
    }

    #[test]
    fn leading_module_doc_returns_none_if_starts_with_code() {
        let src = vec!["fn x {}", "## not module doc"];
        assert!(leading_module_doc(&src).is_none());
    }

    // ─── extract_constant_names / constant_name_of ───────────────────────

    #[test]
    fn extract_constant_names_from_list_takes_keys_only() {
        // `use constant ( FOO => 1, BAR => 2 )` → ["FOO", "BAR"]
        let imports = vec![expr(ExprKind::List(vec![
            expr(ExprKind::Bareword("FOO".into())),
            expr(ExprKind::Integer(1)),
            expr(ExprKind::Bareword("BAR".into())),
            expr(ExprKind::Integer(2)),
        ]))];
        assert_eq!(extract_constant_names(&imports), vec!["FOO", "BAR"]);
    }

    #[test]
    fn extract_constant_names_from_hashref_takes_keys() {
        let imports = vec![expr(ExprKind::HashRef(vec![
            (
                expr(ExprKind::String("PI".into())),
                expr(ExprKind::Float(3.14)),
            ),
            (
                expr(ExprKind::Bareword("E".into())),
                expr(ExprKind::Float(2.71)),
            ),
        ]))];
        assert_eq!(extract_constant_names(&imports), vec!["PI", "E"]);
    }

    #[test]
    fn constant_name_of_only_accepts_string_or_bareword() {
        assert_eq!(
            constant_name_of(&expr(ExprKind::String("X".into()))),
            Some("X".into())
        );
        assert_eq!(
            constant_name_of(&expr(ExprKind::Bareword("Y".into()))),
            Some("Y".into())
        );
        // Integer is not a name → None.
        assert_eq!(constant_name_of(&expr(ExprKind::Integer(7))), None);
    }

    // ─── generate_markdown (integration) ─────────────────────────────────

    #[test]
    fn generate_markdown_emits_header_with_module_title() {
        let prog = Program { statements: vec![] };
        let md = generate_markdown("/some/path/foo.stk", "", &prog);
        assert!(md.starts_with("# Module: foo\n\n"), "got: {md:?}");
    }

    #[test]
    fn generate_markdown_includes_packages_section_when_present() {
        let prog = Program {
            statements: vec![pkg_stmt("My::Pkg")],
        };
        let md = generate_markdown("anon.stk", "", &prog);
        // Title pulled from first package; Packages section also rendered.
        assert!(md.contains("# Module: My::Pkg"));
        assert!(md.contains("## Packages"));
        assert!(md.contains("### `package My::Pkg`"));
    }

    #[test]
    fn generate_markdown_no_subs_skips_subroutines_section() {
        let prog = Program { statements: vec![] };
        let md = generate_markdown("x.stk", "", &prog);
        assert!(!md.contains("## Subroutines"));
    }
}
