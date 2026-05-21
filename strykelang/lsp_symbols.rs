//! Scope-aware symbol table for the LSP server. Powers rename, find-references,
//! and go-to-declaration with proper lexical-scope resolution instead of the
//! textual-occurrence fallback in [`crate::lsp::highlights_for_identifier`].
//!
//! Architecture:
//!
//! * [`SymbolTable::build`] parses the source via [`crate::parse_with_file`],
//!   walks the resulting AST, and produces:
//!     - one [`Symbol`] per declared name (vars, params, fns, types, packages)
//!     - one [`SymbolRef`] per textual occurrence pointing to a [`SymbolId`]
//! * Resolution: at a cursor `(line, character)` the table finds the closest
//!   ref/decl, returns its `SymbolId`; from there, every ref+decl sharing the
//!   id is the rename set.
//!
//! AST nodes only carry line numbers, not byte offsets. To turn a `(line, name)`
//! pair into an exact LSP `Range`, we re-scan the line text for the name —
//! the same trick used by [`crate::lsp::highlights_for_identifier`]. Ambiguity
//! when the same name appears multiple times on one line is resolved by
//! consuming positions in AST traversal order.
//!
//! Scoping rules implemented:
//!
//! * Every `Block` opens a new lexical scope. `my` / `state` / `local` decls
//!   bind into the innermost open block. `our` binds into the enclosing
//!   package namespace (visible across files but resolved per package).
//! * Sub bodies open a fresh scope; signature params bind there.
//! * `for my $x (LIST) { … }` binds `$x` into the loop body's scope only.
//! * Class / struct / enum decls bind a type symbol in the enclosing package.
//! * `package Foo::Bar` switches the current-package context until end-of-file
//!   or the next `package`.
//! * Reference resolution walks scopes inside-out, then falls back to the
//!   package namespace and the global/main namespace.

use std::collections::HashMap;

use lsp_types::{Position, Range};

use crate::ast::{
    Block, Expr, ExprKind, Program, Sigil, Statement, StmtKind, SubSigParam, VarDecl,
};

/// Stable handle to a unique declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ScopeId(u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Local,
    Our,
    State,
    Param,
    Sub,
    Type,
    Package,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub package: String,
    /// 0-based source line of the declaration.
    pub decl_line: u32,
}

#[derive(Clone, Debug)]
pub struct SymbolRef {
    pub symbol: SymbolId,
    pub line: u32,
    pub name: String,
}

pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub refs: Vec<SymbolRef>,
    /// Source line text, indexed by 0-based line number — used to map
    /// `(line, name)` pairs back to byte/UTF-16 character offsets when
    /// emitting LSP edits.
    line_text: Vec<String>,
}

impl SymbolTable {
    pub fn build(text: &str, path: &str) -> Option<Self> {
        let program = crate::parse_with_file(text, path).ok()?;
        let line_text: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        let mut builder = Builder::new(line_text);
        builder.walk_program(&program);
        Some(builder.finish())
    }

    /// Find the symbol referenced/declared at the given 0-based line. If
    /// `needle` is supplied, prefer refs/decls matching that name on the line;
    /// otherwise return the first symbol on the line.
    pub fn symbol_at(&self, line: u32, needle: Option<&str>) -> Option<SymbolId> {
        // Prefer references (they're at the cursor position more often than
        // declarations).
        let mut best: Option<SymbolId> = None;
        for r in &self.refs {
            if r.line != line {
                continue;
            }
            if let Some(n) = needle {
                if name_matches(&r.name, n) {
                    return Some(r.symbol);
                }
            } else if best.is_none() {
                best = Some(r.symbol);
            }
        }
        for d in &self.symbols {
            if d.decl_line != line {
                continue;
            }
            if let Some(n) = needle {
                if name_matches(&d.name, n) {
                    return Some(d.id);
                }
            } else if best.is_none() {
                best = Some(d.id);
            }
        }
        best
    }

    /// Every (line, name) pair belonging to `id` — declaration + references.
    ///
    /// Symbols declared inside a `package` are stored with the package-
    /// qualified name (`Util::greet`), but the declaration site in the
    /// source text uses the bare name (`fn greet { … }`). Emit BOTH the
    /// qualified and the bare-tail name at the decl line so
    /// [`Self::ranges_for`] can locate either spelling.
    pub fn occurrences(&self, id: SymbolId) -> Vec<(u32, String)> {
        let mut out: Vec<(u32, String)> = Vec::new();
        if let Some(sym) = self.symbols.iter().find(|s| s.id == id) {
            out.push((sym.decl_line, sym.name.clone()));
            if let Some(idx) = sym.name.rfind("::") {
                let tail = &sym.name[idx + 2..];
                if !tail.is_empty() && tail != sym.name {
                    out.push((sym.decl_line, tail.to_string()));
                }
            }
        }
        for r in &self.refs {
            if r.symbol == id {
                out.push((r.line, r.name.clone()));
            }
        }
        out
    }

    /// LSP-ready ranges for every occurrence of `id` in this file. Re-scans
    /// the line text for the name to produce exact UTF-16 columns.
    pub fn ranges_for(&self, id: SymbolId) -> Vec<Range> {
        let mut out: Vec<Range> = Vec::new();
        let mut per_line: HashMap<u32, Vec<&str>> = HashMap::new();
        for (line, name) in self.occurrences(id) {
            per_line
                .entry(line)
                .or_default()
                .push(Box::leak(name.into_boxed_str()));
        }
        for (line, names) in per_line {
            let Some(lt) = self.line_text.get(line as usize) else {
                continue;
            };
            for name in names {
                for (start_byte, _) in lt.match_indices(name) {
                    // Reject substring matches that aren't bounded by non-
                    // identifier chars (so `foo` doesn't match inside `foobar`).
                    let prev = lt[..start_byte].chars().next_back();
                    let next = lt[start_byte + name.len()..].chars().next();
                    if !is_ident_boundary_before(prev) || !is_ident_boundary_after(next, name) {
                        continue;
                    }
                    let end_byte = start_byte + name.len();
                    let c0 = lt[..start_byte].encode_utf16().count() as u32;
                    let c1 = lt[..end_byte].encode_utf16().count() as u32;
                    out.push(Range {
                        start: Position {
                            line,
                            character: c0,
                        },
                        end: Position {
                            line,
                            character: c1,
                        },
                    });
                }
            }
        }
        out
    }
}

fn is_ident_boundary_before(c: Option<char>) -> bool {
    match c {
        None => true,
        Some(c) => !c.is_alphanumeric() && c != '_' && c != ':',
    }
}

fn is_ident_boundary_after(c: Option<char>, name: &str) -> bool {
    // For namespaced names ending in `::name`, allow trailing `(` etc.
    // For bare names, reject if followed by `::` (would extend the path).
    let trailing_pair_ok = !name.contains("::");
    match c {
        None => true,
        Some(c) => {
            if c.is_alphanumeric() || c == '_' {
                return false;
            }
            if c == ':' && trailing_pair_ok {
                return false;
            }
            true
        }
    }
}

// ── Builder ──────────────────────────────────────────────────────────────────

struct Builder {
    symbols: Vec<Symbol>,
    refs: Vec<SymbolRef>,
    line_text: Vec<String>,
    next_sym: u32,
    next_scope: u32,
    /// Stack of open lexical scopes; innermost last.
    scope_stack: Vec<ScopeId>,
    /// Per-scope: name → SymbolId. `Vec` indexed by ScopeId.0.
    scope_bindings: Vec<HashMap<String, SymbolId>>,
    /// Current package context. `package Foo::Bar` mutates this.
    package_stack: Vec<String>,
    /// Names → ids for declarations at package scope (functions, types, our-vars).
    package_namespace: HashMap<(String, String), SymbolId>,
}

impl Builder {
    fn new(line_text: Vec<String>) -> Self {
        Self {
            symbols: Vec::new(),
            refs: Vec::new(),
            line_text,
            next_sym: 0,
            next_scope: 0,
            scope_stack: Vec::new(),
            scope_bindings: Vec::new(),
            package_stack: vec!["main".to_string()],
            package_namespace: HashMap::new(),
        }
    }

    fn finish(self) -> SymbolTable {
        SymbolTable {
            symbols: self.symbols,
            refs: self.refs,
            line_text: self.line_text,
        }
    }

    fn fresh_sym(&mut self) -> SymbolId {
        let id = SymbolId(self.next_sym);
        self.next_sym += 1;
        id
    }

    fn fresh_scope(&mut self) -> ScopeId {
        let id = ScopeId(self.next_scope);
        self.next_scope += 1;
        self.scope_bindings.push(HashMap::new());
        id
    }

    fn push_scope(&mut self) -> ScopeId {
        let id = self.fresh_scope();
        self.scope_stack.push(id);
        id
    }

    fn pop_scope(&mut self) {
        self.scope_stack.pop();
    }

    fn current_scope(&self) -> ScopeId {
        *self.scope_stack.last().expect("scope stack underflow")
    }

    fn current_package(&self) -> String {
        self.package_stack
            .last()
            .cloned()
            .unwrap_or_else(|| "main".to_string())
    }

    fn declare_local(&mut self, sigil: Sigil, name: &str, line: u32, kind: SymbolKind) -> SymbolId {
        let id = self.fresh_sym();
        let scope = self.current_scope();
        let display = sigil_prefix(sigil) + name;
        self.symbols.push(Symbol {
            id,
            name: display.clone(),
            kind,
            package: self.current_package(),
            decl_line: line,
        });
        self.scope_bindings[scope.0 as usize].insert(display, id);
        id
    }

    fn declare_package_symbol(&mut self, name: &str, line: u32, kind: SymbolKind) -> SymbolId {
        let id = self.fresh_sym();
        let scope = self.current_scope();
        let pkg = self.current_package();
        // Store under the qualified name (Pkg::name) AND the bare name in
        // the current scope so references can resolve either form.
        // Both `contains("::")` and `pkg == "main"` skip the prefix —
        // collapsed into one branch.
        let qualified = if name.contains("::") || pkg == "main" {
            name.to_string()
        } else {
            format!("{pkg}::{name}")
        };
        self.symbols.push(Symbol {
            id,
            name: qualified.clone(),
            kind,
            package: pkg.clone(),
            decl_line: line,
        });
        self.package_namespace.insert((pkg, qualified.clone()), id);
        // Also register the bare name in the current lexical scope so an
        // unqualified call inside the same file finds it.
        self.scope_bindings[scope.0 as usize].insert(qualified, id);
        id
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // Walk scopes inside-out.
        for scope in self.scope_stack.iter().rev() {
            if let Some(&id) = self.scope_bindings[scope.0 as usize].get(name) {
                return Some(id);
            }
        }
        // Try current-package qualified lookup.
        let pkg = self.current_package();
        if let Some(&id) = self.package_namespace.get(&(pkg.clone(), name.to_string())) {
            return Some(id);
        }
        // Strip current-package prefix and retry.
        if let Some(stripped) = name.strip_prefix(&format!("{pkg}::")) {
            if let Some(&id) = self.package_namespace.get(&(pkg, stripped.to_string())) {
                return Some(id);
            }
        }
        // Final fallback: any package's qualified hit.
        for ((_p, n), &id) in &self.package_namespace {
            if n == name {
                return Some(id);
            }
        }
        None
    }

    fn record_ref(&mut self, name: &str, line: u32) {
        if let Some(id) = self.resolve(name) {
            self.refs.push(SymbolRef {
                symbol: id,
                line,
                name: name.to_string(),
            });
        }
    }

    // ── Walker ───────────────────────────────────────────────────────────

    fn walk_program(&mut self, p: &Program) {
        let _root = self.push_scope();
        for stmt in &p.statements {
            self.walk_stmt(stmt);
        }
        self.pop_scope();
    }

    fn walk_block(&mut self, block: &Block) {
        let _scope = self.push_scope();
        for stmt in block {
            self.walk_stmt(stmt);
        }
        self.pop_scope();
    }

    fn walk_stmt(&mut self, stmt: &Statement) {
        let line0 = ast_line_to_lsp(stmt.line);
        match &stmt.kind {
            StmtKind::Expression(e) => self.walk_expr(e),
            StmtKind::My(decls) | StmtKind::State(decls) | StmtKind::Local(decls) => {
                let kind = match &stmt.kind {
                    StmtKind::My(_) => SymbolKind::Local,
                    StmtKind::State(_) => SymbolKind::State,
                    StmtKind::Local(_) => SymbolKind::Local,
                    _ => SymbolKind::Local,
                };
                for d in decls {
                    self.walk_var_decl(d, line0, kind.clone());
                }
            }
            StmtKind::Our(decls) | StmtKind::OurSync(decls) => {
                for d in decls {
                    let sym = self.declare_local(d.sigil, &d.name, line0, SymbolKind::Our);
                    let display = sigil_prefix(d.sigil) + &d.name;
                    let pkg = self.current_package();
                    self.package_namespace.insert((pkg, display), sym);
                    if let Some(init) = &d.initializer {
                        self.walk_expr(init);
                    }
                }
            }
            StmtKind::MySync(decls) => {
                for d in decls {
                    self.walk_var_decl(d, line0, SymbolKind::Local);
                }
            }
            StmtKind::Block(b) | StmtKind::StmtGroup(b) => self.walk_block(b),
            StmtKind::SubDecl {
                name, params, body, ..
            } => {
                self.declare_package_symbol(name, line0, SymbolKind::Sub);
                let _scope = self.push_scope();
                for p in params {
                    self.walk_sub_param(p, line0);
                }
                for s in body {
                    self.walk_stmt(s);
                }
                self.pop_scope();
            }
            StmtKind::Package { name } => {
                self.package_stack.push(name.clone());
                self.declare_package_symbol(name, line0, SymbolKind::Package);
            }
            StmtKind::StructDecl { def } => {
                self.declare_package_symbol(&def.name, line0, SymbolKind::Type);
                self.walk_generic_stmt(stmt);
            }
            StmtKind::EnumDecl { def } => {
                self.declare_package_symbol(&def.name, line0, SymbolKind::Type);
                self.walk_generic_stmt(stmt);
            }
            StmtKind::ClassDecl { def, .. } => {
                self.declare_package_symbol(&def.name, line0, SymbolKind::Type);
                self.walk_generic_stmt(stmt);
            }
            StmtKind::TraitDecl { def } => {
                self.declare_package_symbol(&def.name, line0, SymbolKind::Type);
                self.walk_generic_stmt(stmt);
            }
            _ => {
                // Fall through to a generic walk via serde reflection.
                // Catches edge AST variants (try/catch, given/when, etc.)
                // without enumerating all 123 up-front.
                self.walk_generic_stmt(stmt);
            }
        }
    }

    fn walk_var_decl(&mut self, d: &VarDecl, line: u32, kind: SymbolKind) {
        self.declare_local(d.sigil, &d.name, line, kind);
        if let Some(init) = &d.initializer {
            self.walk_expr(init);
        }
    }

    fn walk_expr(&mut self, e: &Expr) {
        let line = ast_line_to_lsp(e.line);
        match &e.kind {
            ExprKind::ScalarVar(n) => self.record_ref(&format!("${n}"), line),
            ExprKind::ArrayVar(n) => self.record_ref(&format!("@{n}"), line),
            ExprKind::HashVar(n) => self.record_ref(&format!("%{n}"), line),
            ExprKind::FuncCall { name, args, .. } => {
                self.record_ref(name, line);
                for a in args {
                    self.walk_expr(a);
                }
            }
            // Method-call receivers and package-qualified paths show up as
            // `Bareword("Point")` in `Point->new(...)`, `Foo::bar(...)`, etc.
            // Treat each as a reference so renaming a struct / class / enum
            // / package picks up the usage sites, not just the declaration.
            ExprKind::Bareword(n) => self.record_ref(n, line),
            ExprKind::Do(inner) => self.walk_expr(inner),
            _ => self.walk_generic_expr(e),
        }
    }

    fn walk_sub_param(&mut self, p: &SubSigParam, line: u32) {
        match p {
            SubSigParam::Scalar(name, _ty, default) => {
                self.declare_local(Sigil::Scalar, name, line, SymbolKind::Param);
                if let Some(d) = default {
                    self.walk_expr(d);
                }
            }
            SubSigParam::Array(name, default) => {
                self.declare_local(Sigil::Array, name, line, SymbolKind::Param);
                if let Some(d) = default {
                    self.walk_expr(d);
                }
            }
            SubSigParam::Hash(name, default) => {
                self.declare_local(Sigil::Hash, name, line, SymbolKind::Param);
                if let Some(d) = default {
                    self.walk_expr(d);
                }
            }
            _ => {}
        }
    }

    /// Best-effort recursion via reflection on the AST `Debug` shape. We
    /// extract anything that looks like an Expr or a Block by JSON-walking
    /// the serde representation. Keeps the walker complete across new AST
    /// variants without enumerating every one.
    fn walk_generic_stmt(&mut self, stmt: &Statement) {
        // Serialize to JSON, walk for nested expr / block / vardecl nodes.
        let v = match serde_json::to_value(stmt) {
            Ok(v) => v,
            Err(_) => return,
        };
        self.walk_json(&v, ast_line_to_lsp(stmt.line));
    }

    fn walk_generic_expr(&mut self, e: &Expr) {
        let v = match serde_json::to_value(e) {
            Ok(v) => v,
            Err(_) => return,
        };
        self.walk_json(&v, ast_line_to_lsp(e.line));
    }

    /// Walk a JSON value looking for the recognizable shapes we care about:
    /// `ScalarVar`/`ArrayVar`/`HashVar`/`FuncCall.name` for references. Best-
    /// effort fallback for AST variants not explicitly handled above.
    ///
    /// Line numbers carried by AST nodes are 1-based; LSP positions are
    /// 0-based. Every JSON `"line"` field is run through
    /// [`ast_line_to_lsp`] before being recorded — the parent `line` arg is
    /// already 0-based by contract from the callers above.
    fn walk_json(&mut self, v: &serde_json::Value, line: u32) {
        match v {
            serde_json::Value::Object(map) => {
                if let Some(s) = map.get("ScalarVar").and_then(|x| x.as_str()) {
                    self.record_ref(&format!("${s}"), line);
                }
                if let Some(s) = map.get("ArrayVar").and_then(|x| x.as_str()) {
                    self.record_ref(&format!("@{s}"), line);
                }
                if let Some(s) = map.get("HashVar").and_then(|x| x.as_str()) {
                    self.record_ref(&format!("%{s}"), line);
                }
                if let Some(s) = map.get("Bareword").and_then(|x| x.as_str()) {
                    // `Point->new(...)`, `Foo::Bar->method()`, qualified
                    // bareword calls — pick up the receiver as a ref so
                    // class/struct/enum/package rename catches usages.
                    self.record_ref(s, line);
                }
                if let Some(call) = map.get("FuncCall").and_then(|x| x.as_object()) {
                    if let Some(name) = call.get("name").and_then(|x| x.as_str()) {
                        let used_line = call
                            .get("line")
                            .and_then(|x| x.as_u64())
                            .map(|n| ast_line_to_lsp(n as usize))
                            .unwrap_or(line);
                        self.record_ref(name, used_line);
                    }
                }
                let next_line = map
                    .get("line")
                    .and_then(|x| x.as_u64())
                    .map(|n| ast_line_to_lsp(n as usize))
                    .unwrap_or(line);
                for (_k, vv) in map {
                    self.walk_json(vv, next_line);
                }
            }
            serde_json::Value::Array(items) => {
                for vv in items {
                    self.walk_json(vv, line);
                }
            }
            _ => {}
        }
    }
}

/// AST line numbers are 1-based (column-1 of the source). LSP `Position`
/// lines are 0-based. Convert exactly once at the boundary so every line
/// stored on a [`Symbol`] / [`SymbolRef`] is in LSP space and matches the
/// `(line, character)` the editor sends back.
fn ast_line_to_lsp(line1: usize) -> u32 {
    (line1.saturating_sub(1)) as u32
}

/// `stored` is the symbol/ref's canonical name (possibly package-qualified
/// like `Util::greet`); `needle` is what the editor extracted at the cursor.
/// A bare-name cursor (`greet`) should still resolve to a qualified
/// declaration in the same package, so accept either an exact match or a
/// `::needle` tail. This is what lets `fn greet` declared inside
/// `package Util` answer cursor hits on the bare `greet` token.
fn name_matches(stored: &str, needle: &str) -> bool {
    stored == needle || stored.ends_with(&format!("::{needle}"))
}

fn sigil_prefix(s: Sigil) -> String {
    match s {
        Sigil::Scalar => "$".to_string(),
        Sigil::Array => "@".to_string(),
        Sigil::Hash => "%".to_string(),
        _ => "*".to_string(), // Typeglob and any future sigils
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ast_line_to_lsp_zero_indexes() {
        assert_eq!(ast_line_to_lsp(1), 0);
        assert_eq!(ast_line_to_lsp(42), 41);
        // `0` is impossible in well-formed AST but the helper must not
        // wrap around — `saturating_sub` keeps us at zero.
        assert_eq!(ast_line_to_lsp(0), 0);
    }

    #[test]
    fn name_matches_exact_and_tail() {
        assert!(name_matches("greet", "greet"));
        assert!(name_matches("Util::greet", "greet"));
        assert!(!name_matches("Util::greeter", "greet"));
        assert!(!name_matches("greet", "Util::greet"));
        assert!(!name_matches("", "greet"));
    }

    #[test]
    fn sigil_prefix_maps_known_sigils() {
        assert_eq!(sigil_prefix(Sigil::Scalar), "$");
        assert_eq!(sigil_prefix(Sigil::Array), "@");
        assert_eq!(sigil_prefix(Sigil::Hash), "%");
    }

    #[test]
    fn is_ident_boundary_rejects_alnum_and_underscore() {
        assert!(is_ident_boundary_before(None));
        assert!(is_ident_boundary_before(Some(' ')));
        assert!(is_ident_boundary_before(Some('(')));
        assert!(!is_ident_boundary_before(Some('a')));
        assert!(!is_ident_boundary_before(Some('_')));
        assert!(!is_ident_boundary_before(Some(':')));
    }

    #[test]
    fn is_ident_boundary_after_rejects_continuation() {
        // bare name: trailing `::` would extend the path → rejected
        assert!(!is_ident_boundary_after(Some(':'), "foo"));
        // already-namespaced name: trailing punctuation OK
        assert!(is_ident_boundary_after(Some('('), "Util::greet"));
        // alphanumerics never form a boundary
        assert!(!is_ident_boundary_after(Some('x'), "foo"));
        // end-of-line/string is always a boundary
        assert!(is_ident_boundary_after(None, "foo"));
    }

    fn build(src: &str) -> SymbolTable {
        SymbolTable::build(src, "test.stk").expect("source parses")
    }

    #[test]
    fn build_collects_simple_my_declaration() {
        let t = build("my $x = 1\np $x\n");
        // One Symbol for `$x`, at least one SymbolRef on use line.
        let xs: Vec<_> = t.symbols.iter().filter(|s| s.name == "$x").collect();
        assert_eq!(
            xs.len(),
            1,
            "expected one symbol for $x, got {:?}",
            t.symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
        assert_eq!(xs[0].kind, SymbolKind::Local);
    }

    #[test]
    fn occurrences_include_decl_and_refs_for_local() {
        // Use an explicit expression form so $x is unambiguously a ScalarVar
        // reference (not the first arg of a print-like statement, which the
        // parser may consume specially).
        let t = build("my $x = 1\nmy $y = $x + 1\nmy $z = $x * 2\n");
        let id = t.symbols.iter().find(|s| s.name == "$x").unwrap().id;
        let occ = t.occurrences(id);
        let lines: Vec<u32> = occ.iter().map(|(l, _)| *l).collect();
        // decl line 0, refs on lines 1 and 2 (0-indexed)
        assert!(lines.contains(&0), "missing decl line 0 in {lines:?}");
        assert!(lines.contains(&1), "missing ref on line 1 in {lines:?}");
        assert!(lines.contains(&2), "missing ref on line 2 in {lines:?}");
    }

    #[test]
    fn ranges_for_filters_substring_matches() {
        // `$x` appears inside `$xy` — ranges_for must skip the substring hit.
        let t = build("my $x = 1\nmy $xy = $x + 2\n");
        let id = t.symbols.iter().find(|s| s.name == "$x").unwrap().id;
        let ranges = t.ranges_for(id);
        // Expect: decl on line 0, one ref on line 1 — not 2 ranges on line 1.
        let line1 = ranges.iter().filter(|r| r.start.line == 1).count();
        assert_eq!(
            line1, 1,
            "expected single $x range on line 1 (not the $xy substring), got {ranges:?}"
        );
    }
}
