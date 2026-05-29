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
/// `SymbolKind` — see variants.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    /// `Local` variant.
    Local,
    /// `Our` variant.
    Our,
    /// `State` variant.
    State,
    /// `Param` variant.
    Param,
    /// `Sub` variant.
    Sub,
    /// `Type` variant.
    Type,
    /// `Package` variant.
    Package,
    /// `format Foo = ... .` — Perl report templates referenced by `write FOO`.
    /// Package-scoped, behaves like Sub for rename / goto-def.
    Format,
    /// Loop label (`LOOP:`) referenced by `last LOOP` / `next LOOP` /
    /// `redo LOOP` / `goto LOOP`. File-local (lexical).
    Label,
    /// Struct / class / enum-variant field. Indexed by bare name with
    /// the struct's decl line; multiple structs sharing a field name
    /// will all match — goto-def returns the first found.
    Field,
}
/// `Symbol` — see fields for layout.

#[derive(Clone, Debug)]
pub struct Symbol {
    /// `id` field.
    pub id: SymbolId,
    /// `name` field.
    pub name: String,
    /// `kind` field.
    pub kind: SymbolKind,
    /// `package` field.
    pub package: String,
    /// 0-based source line of the declaration.
    pub decl_line: u32,
}
/// `SymbolRef` — see fields for layout.

#[derive(Clone, Debug)]
pub struct SymbolRef {
    /// `symbol` field.
    pub symbol: SymbolId,
    /// `line` field.
    pub line: u32,
    /// `name` field.
    pub name: String,
}
/// `SymbolTable` — see fields for layout.

pub struct SymbolTable {
    /// `symbols` field.
    pub symbols: Vec<Symbol>,
    /// `refs` field.
    pub refs: Vec<SymbolRef>,
    /// Source line text, indexed by 0-based line number — used to map
    /// `(line, name)` pairs back to byte/UTF-16 character offsets when
    /// emitting LSP edits.
    line_text: Vec<String>,
}

impl SymbolTable {
    /// `build` — see implementation.
    pub fn build(text: &str, path: &str) -> Option<Self> {
        Self::build_with_extra_types(
            text,
            path,
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
        )
    }

    /// Like [`Self::build`] but injects additional Type / Field names
    /// the file should be aware of — typically gathered from a
    /// `require`d lib so cross-file Field rename can detect that
    /// `Project::Geom::Point(x => 1)` in the active file is a
    /// constructor call (and therefore `x => 1` is a Field ref).
    pub fn build_with_extra_types(
        text: &str,
        path: &str,
        extra_types: &std::collections::HashSet<String>,
        extra_fields: &std::collections::HashSet<String>,
    ) -> Option<Self> {
        let program = crate::parse_with_file(text, path).ok()?;
        let line_text: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        let mut builder = Builder::new(line_text);
        for t in extra_types {
            builder.known_type_names.insert(t.clone());
        }
        for f in extra_fields {
            builder.known_field_names.insert(f.clone());
        }
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

    /// Like [`Self::ranges_for`] but also returns the matched spelling
    /// for each range. Callers that need sigil-aware substitution
    /// (rename for `%foo` referenced via `$foo{k}` element access) use
    /// the matched name to compute the correct new spelling.
    pub fn ranges_and_names_for(&self, id: SymbolId) -> Vec<(Range, String)> {
        let mut out: Vec<(Range, String)> = Vec::new();
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
                    let prev = lt[..start_byte].chars().next_back();
                    let after_slice = &lt[start_byte + name.len()..];
                    let next = after_slice.chars().next();
                    let next_next = after_slice.chars().nth(1);
                    if !is_ident_boundary_before(prev)
                        || !is_ident_boundary_after_pair(next, next_next, name)
                    {
                        continue;
                    }
                    let end_byte = start_byte + name.len();
                    let c0 = lt[..start_byte].encode_utf16().count() as u32;
                    let c1 = lt[..end_byte].encode_utf16().count() as u32;
                    out.push((
                        Range {
                            start: Position {
                                line,
                                character: c0,
                            },
                            end: Position {
                                line,
                                character: c1,
                            },
                        },
                        name.to_string(),
                    ));
                }
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
                    let after_slice = &lt[start_byte + name.len()..];
                    let next = after_slice.chars().next();
                    let next_next = after_slice.chars().nth(1);
                    if !is_ident_boundary_before(prev)
                        || !is_ident_boundary_after_pair(next, next_next, name)
                    {
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

/// Pull the constant's name out of a `use constant NAME =>` slot. The
/// parser usually delivers it as `String(_)` via fat-arrow auto-quoting,
/// but bare-identifier keys in a hashref-block come through as
/// `Bareword(_)`. Mirrors `vm_helper::use_constant_name_from_expr`.
fn constant_name_from_expr(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::String(s) => Some(s.clone()),
        ExprKind::Bareword(s) => Some(s.clone()),
        _ => None,
    }
}

fn is_ident_boundary_before(c: Option<char>) -> bool {
    match c {
        None => true,
        Some(c) => !c.is_alphanumeric() && c != '_' && c != ':',
    }
}

#[cfg(test)]
fn is_ident_boundary_after(c: Option<char>, name: &str) -> bool {
    is_ident_boundary_after_pair(c, None, name)
}

/// Boundary check with one-char lookahead. The lookahead is required to
/// distinguish a label-decl colon (`OUTER:` — single colon followed by
/// space/newline) from a package-path separator (`Foo::Bar` — double
/// colon extending the bareword). With single-char input we can't tell
/// the two apart and would reject every label-decl span.
fn is_ident_boundary_after_pair(c: Option<char>, c2: Option<char>, name: &str) -> bool {
    let trailing_pair_ok = !name.contains("::");
    match c {
        None => true,
        Some(c) => {
            if c.is_alphanumeric() || c == '_' {
                return false;
            }
            if c == ':' && trailing_pair_ok {
                // Single `:` (label-decl) is a boundary; `::` (path
                // continuation) is not.
                return c2 != Some(':');
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
    /// Bare Type names (struct/class/enum/trait) gathered in a
    /// pre-pass so the FuncCall walker can decide whether a call
    /// like `Rectangle(width => 1)` is a struct constructor (and
    /// therefore its fat-comma keys are Field refs) vs an arbitrary
    /// function call.
    known_type_names: std::collections::HashSet<String>,
    /// Bare Field names (struct/class fields, enum variants) gathered
    /// in the same pre-pass. Used to filter qualified-Bareword refs
    /// like `TrafficLight::Red` in match arms — only treat the suffix
    /// as a Field ref when it's actually declared as a Field.
    known_field_names: std::collections::HashSet<String>,
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
            known_type_names: std::collections::HashSet::new(),
            known_field_names: std::collections::HashSet::new(),
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

    /// Resolve a bare name against Field symbols (struct/class fields
    /// and enum variants). Used by the MethodCall + struct-constructor
    /// walkers to record refs that the regular `resolve` misses —
    /// Field symbols don't live in scope_bindings or
    /// package_namespace because field names overlap freely with
    /// other identifiers.
    fn record_method_or_field_ref(&mut self, name: &str, line: u32) {
        // Sub / Type / Our / Format / Local — go through resolve.
        if let Some(id) = self.resolve(name) {
            self.refs.push(SymbolRef {
                symbol: id,
                line,
                name: name.to_string(),
            });
            return;
        }
        // Field fallback — scan declared Fields by bare name.
        if let Some(id) = self
            .symbols
            .iter()
            .find(|s| s.name == name && matches!(s.kind, SymbolKind::Field))
            .map(|s| s.id)
        {
            self.refs.push(SymbolRef {
                symbol: id,
                line,
                name: name.to_string(),
            });
        }
    }

    /// Record a ref to the symbol resolved under `canonical` but tagged
    /// with the actually-appearing-in-source spelling `as_spelled`.
    /// Used for sigil-aliased access (`$h{k}` reads from `%h`, `@h{k1,k2}`
    /// slices from `%h`): resolution must use the canonical sigil so the
    /// SymbolTable's scope_bindings hit, but the recorded ref carries the
    /// source spelling so [`Self::ranges_and_names_for`] can find it in
    /// the line text and the renamer can substitute sigil-preserving.
    fn record_aliased_ref(&mut self, canonical: &str, as_spelled: &str, line: u32) {
        if let Some(id) = self.resolve(canonical) {
            self.refs.push(SymbolRef {
                symbol: id,
                line,
                name: as_spelled.to_string(),
            });
        }
    }

    // ── Walker ───────────────────────────────────────────────────────────

    fn walk_program(&mut self, p: &Program) {
        let _root = self.push_scope();
        // Pre-pass: collect Type + Field bare names so usage-before-
        // decl ordering doesn't lose refs. Without this, a FuncCall
        // on line 2 walking `Rectangle(width => 1)` wouldn't know
        // that `Rectangle` is a Type and `width` is a Field if the
        // struct decl is on line 10.
        for stmt in &p.statements {
            self.precollect_types_and_fields(stmt);
        }
        for stmt in &p.statements {
            self.walk_stmt(stmt);
        }
        self.pop_scope();
    }

    fn precollect_types_and_fields(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StmtKind::StructDecl { def } => {
                self.known_type_names.insert(def.name.clone());
                for f in &def.fields {
                    self.known_field_names.insert(f.name.clone());
                }
                // Methods participate in the `$self->X` / `$obj->X`
                // rename + goto-def pipeline alongside fields.
                for m in &def.methods {
                    self.known_field_names.insert(m.name.clone());
                }
            }
            StmtKind::EnumDecl { def } => {
                self.known_type_names.insert(def.name.clone());
                for v in &def.variants {
                    self.known_field_names.insert(v.name.clone());
                }
            }
            StmtKind::ClassDecl { def, .. } => {
                self.known_type_names.insert(def.name.clone());
                for f in &def.fields {
                    self.known_field_names.insert(f.name.clone());
                }
                for m in &def.methods {
                    self.known_field_names.insert(m.name.clone());
                }
            }
            StmtKind::TraitDecl { def } => {
                self.known_type_names.insert(def.name.clone());
                for m in &def.methods {
                    self.known_field_names.insert(m.name.clone());
                }
            }
            // Recurse into nested blocks so types declared inside
            // BEGIN/END/INIT phasers + nested packages are visible.
            StmtKind::Block(b)
            | StmtKind::StmtGroup(b)
            | StmtKind::Begin(b)
            | StmtKind::End(b)
            | StmtKind::UnitCheck(b)
            | StmtKind::Check(b)
            | StmtKind::Init(b)
            | StmtKind::Continue(b) => {
                for s in b {
                    self.precollect_types_and_fields(s);
                }
            }
            _ => {}
        }
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
                for f in &def.fields {
                    self.declare_field(&f.name, line0);
                }
                // Methods registered as Field symbols too — they
                // share the same access shape (`$self->method` /
                // `$obj->method`) as fields, so rename / goto-def
                // / find-usages all go through the same code path.
                for m in &def.methods {
                    self.declare_field(&m.name, line0);
                }
                self.walk_generic_stmt(stmt);
            }
            StmtKind::EnumDecl { def } => {
                self.declare_package_symbol(&def.name, line0, SymbolKind::Type);
                for v in &def.variants {
                    self.declare_field(&v.name, line0);
                }
                self.walk_generic_stmt(stmt);
            }
            StmtKind::ClassDecl { def, .. } => {
                self.declare_package_symbol(&def.name, line0, SymbolKind::Type);
                for f in &def.fields {
                    self.declare_field(&f.name, line0);
                }
                for m in &def.methods {
                    self.declare_field(&m.name, line0);
                }
                self.walk_generic_stmt(stmt);
            }
            StmtKind::TraitDecl { def } => {
                self.declare_package_symbol(&def.name, line0, SymbolKind::Type);
                // Register trait method names as Field symbols so rename
                // at the trait declaration site works. Without this,
                // F2 on `state` / `transition` inside `trait Stateful
                // { fn state; fn transition }` had nothing to rename
                // at the decl — only the call sites got rewritten.
                for m in &def.methods {
                    self.declare_field(&m.name, line0);
                }
                self.walk_generic_stmt(stmt);
            }
            // `use constant NAME => value` / `use constant { A => 1, B => 2 }` /
            // `use constant (A => 1, B => 2)` — each constant name compiles
            // to a `Sub` at runtime, so register it as such in the symbol
            // table. Rename, hover, goto-def then work uniformly.
            StmtKind::Use { module, imports } if module == "constant" => {
                for imp in imports {
                    self.declare_constants_from_import(imp, line0);
                }
            }
            // `format NAME = ... .` — Perl report template. Referenced by
            // `write NAME` / `select NAME`.
            StmtKind::FormatDecl { name, .. } => {
                self.declare_package_symbol(name, line0, SymbolKind::Format);
            }
            // Loop labels: `LOOP: while (...) { ... }`. Declared at the
            // labeled-loop's line (file-local lexical scope), referenced
            // by `last LOOP` / `next LOOP` / `redo LOOP`.
            StmtKind::While { label, body, .. } | StmtKind::Until { label, body, .. } => {
                if let Some(lab) = label {
                    self.declare_label(lab, line0);
                }
                self.walk_block(body);
            }
            StmtKind::For {
                label,
                init,
                condition,
                step,
                body,
                continue_block,
            } => {
                if let Some(lab) = label {
                    self.declare_label(lab, line0);
                }
                if let Some(init) = init {
                    self.walk_stmt(init);
                }
                if let Some(c) = condition {
                    self.walk_expr(c);
                }
                if let Some(s) = step {
                    self.walk_expr(s);
                }
                self.walk_block(body);
                if let Some(b) = continue_block {
                    self.walk_block(b);
                }
            }
            StmtKind::Foreach {
                var,
                label,
                list,
                body,
                ..
            } => {
                if let Some(lab) = label {
                    self.declare_label(lab, line0);
                }
                self.walk_expr(list);
                // Push a fresh scope for the loop var + body, then
                // declare the loop var inside it. Without this,
                // `for my $cb (@cbs) { … }` left `$cb` undeclared —
                // rename / hover at `$cb` failed to find any symbol.
                let _scope = self.push_scope();
                // Foreach loop var is always scalar (`for my $cb (…)`)
                // and the parser stores the bare name without the `$`
                // sigil.
                if !var.is_empty() {
                    self.declare_local(Sigil::Scalar, var, line0, SymbolKind::Local);
                }
                for stmt in body {
                    self.walk_stmt(stmt);
                }
                self.pop_scope();
            }
            StmtKind::Last(Some(lab)) | StmtKind::Next(Some(lab)) | StmtKind::Redo(Some(lab)) => {
                self.record_ref(lab, line0);
            }
            StmtKind::Goto { target } => {
                if let ExprKind::Bareword(n) = &target.kind {
                    self.record_ref(n, line0);
                } else {
                    self.walk_expr(target);
                }
            }
            _ => {
                // Fall through to a generic walk via serde reflection.
                // Catches edge AST variants (try/catch, given/when, etc.)
                // without enumerating all 123 up-front.
                self.walk_generic_stmt(stmt);
            }
        }
    }

    /// Declare a struct / class field (or enum variant) at the parent
    /// type's `line`. Stored bare-name so goto-def on a constructor
    /// arg (`Rectangle(width => -1, …)`) lands on the struct decl.
    fn declare_field(&mut self, name: &str, line: u32) -> SymbolId {
        let id = self.fresh_sym();
        self.symbols.push(Symbol {
            id,
            name: name.to_string(),
            kind: SymbolKind::Field,
            package: self.current_package(),
            decl_line: line,
        });
        id
    }

    /// Declare a loop label at `line` in the current scope. Labels are
    /// file-local (lexical), so we use [`declare_local`] with no sigil
    /// to keep them out of the package namespace.
    fn declare_label(&mut self, name: &str, line: u32) -> SymbolId {
        let id = self.fresh_sym();
        let scope = self.current_scope();
        self.symbols.push(Symbol {
            id,
            name: name.to_string(),
            kind: SymbolKind::Label,
            package: self.current_package(),
            decl_line: line,
        });
        self.scope_bindings[scope.0 as usize].insert(name.to_string(), id);
        id
    }

    /// Walk one `use constant`-import argument and register each named
    /// constant. Mirrors `vm_helper::apply_use_constant` for shape:
    ///   - `List(items)`: even-length NAME, VALUE pairs.
    ///   - `HashRef(pairs)`: `{ NAME => VALUE, ... }`.
    /// Names arrive as `String(_)` (fat-arrow auto-quote) or `Bareword(_)`.
    fn declare_constants_from_import(&mut self, imp: &Expr, line: u32) {
        match &imp.kind {
            ExprKind::List(items) => {
                let mut i = 0;
                while i + 1 < items.len() {
                    if let Some(name) = constant_name_from_expr(&items[i]) {
                        self.declare_package_symbol(&name, line, SymbolKind::Sub);
                    }
                    i += 2;
                }
            }
            ExprKind::HashRef(pairs) => {
                for (k, _v) in pairs {
                    if let Some(name) = constant_name_from_expr(k) {
                        self.declare_package_symbol(&name, line, SymbolKind::Sub);
                    }
                }
            }
            // Single bare `use constant FOO => 1` may also surface as a
            // direct `String`/`Bareword` if the parser doesn't wrap it.
            _ => {
                if let Some(name) = constant_name_from_expr(imp) {
                    self.declare_package_symbol(&name, line, SymbolKind::Sub);
                }
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
                // Struct-constructor sugar: `Rectangle(width => 1,
                // height => 2)` parses as a FuncCall with args
                // alternating [String/Bareword, value, ...]. ONLY
                // treat the keys as Field refs when the function
                // name is a known Type — otherwise we'd false-
                // positive on arbitrary hash literals or function
                // calls that happen to have a fat-comma arg list.
                let bare_tail = name.rsplit("::").next().unwrap_or(name.as_str());
                if self.known_type_names.contains(name.as_str())
                    || self.known_type_names.contains(bare_tail)
                {
                    self.walk_fat_comma_args_for_fields(args, line);
                }
                // Qualified-name as a zero-arg call — `Color::Red`
                // parses as FuncCall{name: "Color::Red", args:[]}
                // when used as a value (enum-variant access without
                // explicit constructor parens). Record the suffix as
                // a Field ref if it's a known variant of a known
                // Type. Same AST-only rule as the Bareword arm.
                if let Some(idx) = name.rfind("::") {
                    let suffix = &name[idx + 2..];
                    let prefix = &name[..idx];
                    let prefix_tail = prefix.rsplit("::").next().unwrap_or(prefix);
                    if !suffix.is_empty()
                        && self.known_field_names.contains(suffix)
                        && (self.known_type_names.contains(prefix)
                            || self.known_type_names.contains(prefix_tail))
                    {
                        self.record_method_or_field_ref(suffix, line);
                    }
                }
                for a in args {
                    self.walk_expr(a);
                }
            }
            // Method-call receivers and package-qualified paths show up as
            // `Bareword("Point")` in `Point->new(...)`, `Foo::bar(...)`, etc.
            // Treat each as a reference so renaming a struct / class / enum
            // / package picks up the usage sites, not just the declaration.
            ExprKind::Bareword(n) => {
                self.record_ref(n, line);
                // Qualified path `Pkg::Variant` or `Type::field` —
                // additionally record refs for the suffix if it's a
                // known Field name (catches `TrafficLight::Red` in
                // match arms, `Op::Add` as a value, etc.). The
                // prefix is already covered by `record_ref(n)` which
                // tries the Type by name fallback.
                if let Some(idx) = n.rfind("::") {
                    let suffix = &n[idx + 2..];
                    if !suffix.is_empty() && self.known_field_names.contains(suffix) {
                        self.record_method_or_field_ref(suffix, line);
                    }
                    // Also record a ref for the prefix as a Type if
                    // it matches a known Type name.
                    let prefix = &n[..idx];
                    let bare = prefix.rsplit("::").next().unwrap_or(prefix);
                    if !bare.is_empty()
                        && (self.known_type_names.contains(prefix)
                            || self.known_type_names.contains(bare))
                    {
                        // record_ref tries resolve first; for a Type
                        // declared with package "main" the qualified
                        // form is just the bare name, so this works.
                        self.record_ref(prefix, line);
                    }
                }
            }
            // `\&name` — coderef to a named sub. The name is a regular
            // sub identifier; rename / goto-def need to treat it as a
            // ref so the workspace edit catches every `\&named_one`
            // site when `named_one` is renamed.
            ExprKind::SubroutineCodeRef(n) => self.record_ref(n, line),
            // `$obj->method(args)` / `$obj->field` — record a ref to
            // the method name so rename on a Sub or Field declared
            // inside a struct/class body finds every accessor call
            // site. The MethodCall name is bare; resolve walks the
            // package_namespace to attach it to the right symbol if
            // one exists, otherwise the ref is dropped (no false
            // positives).
            ExprKind::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                self.walk_expr(object);
                self.record_method_or_field_ref(method, line);
                // Only walk fat-comma keys as Field refs when the
                // receiver is a known Type (`Rectangle->new(width
                // => 1)`). Otherwise an arbitrary method call with
                // hash-like args would false-positive.
                let receiver_is_type = match &object.kind {
                    ExprKind::Bareword(n) => {
                        let tail = n.rsplit("::").next().unwrap_or(n.as_str());
                        self.known_type_names.contains(n.as_str())
                            || self.known_type_names.contains(tail)
                    }
                    _ => false,
                };
                if receiver_is_type {
                    self.walk_fat_comma_args_for_fields(args, line);
                }
                for a in args {
                    self.walk_expr(a);
                }
            }
            // Element / slice access: `$arr[i]`, `$h{k}`, `@arr[1,2]`,
            // `@h{k1,k2}`, `%h{k1,k2}` — the container name lives in a
            // bare `String` field, NOT a wrapping `ArrayVar`/`HashVar`,
            // so the serde reflection fallback doesn't see it. Record
            // the container as a ref explicitly so rename / goto-def
            // catch every element-access site.
            ExprKind::ArrayElement { array, index } => {
                // `$arr[i]` reads a scalar from `@arr` — resolve under
                // the canonical `@arr`, then also record the access-form
                // spelling (`$arr`) so `ranges_and_names_for` finds it
                // on the line for sigil-aware rename substitution.
                self.record_aliased_ref(&format!("@{array}"), &format!("${array}"), line);
                self.walk_expr(index);
            }
            ExprKind::HashElement { hash, key } => {
                // `$h{k}` reads a scalar from `%h` — same pattern.
                self.record_aliased_ref(&format!("%{hash}"), &format!("${hash}"), line);
                self.walk_expr(key);
            }
            ExprKind::ArraySlice { array, indices } => {
                // `@arr[i,j]` is still spelled with `@` — record canonical only.
                self.record_ref(&format!("@{array}"), line);
                for i in indices {
                    self.walk_expr(i);
                }
            }
            ExprKind::HashSlice { hash, keys } => {
                // `@h{k1,k2}` slice on a hash uses `@` sigil in source.
                self.record_aliased_ref(&format!("%{hash}"), &format!("@{hash}"), line);
                for k in keys {
                    self.walk_expr(k);
                }
            }
            ExprKind::HashKvSlice { hash, keys } => {
                // `%h{k1,k2}` keeps the `%` sigil — canonical alone.
                self.record_ref(&format!("%{hash}"), line);
                for k in keys {
                    self.walk_expr(k);
                }
            }
            ExprKind::Do(inner) => self.walk_expr(inner),
            _ => self.walk_generic_expr(e),
        }
    }

    /// Walk a FuncCall / MethodCall arg list looking for fat-comma
    /// pairs: alternating String/Bareword keys followed by values.
    /// Each key that matches a declared Field name is recorded as a
    /// Field ref so rename catches `Rectangle(width => 1)` call sites.
    fn walk_fat_comma_args_for_fields(&mut self, args: &[Expr], default_line: u32) {
        let mut i = 0;
        while i < args.len() {
            let key_name = match &args[i].kind {
                ExprKind::String(s) => Some(s.clone()),
                ExprKind::Bareword(s) => Some(s.clone()),
                _ => None,
            };
            if let Some(name) = key_name {
                let line = ast_line_to_lsp(args[i].line);
                // record_method_or_field_ref is the right entry —
                // it'll fall through silently if `name` doesn't match
                // any Field, so we don't create false refs for
                // arbitrary hash keys.
                let line = if line == 0 { default_line } else { line };
                self.record_method_or_field_ref(&name, line);
            }
            i += 2;
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
                    // Qualified `Type::Field` form — record the suffix
                    // as a Field ref if it matches a known Field.
                    if let Some(idx) = s.rfind("::") {
                        let suffix = &s[idx + 2..];
                        if !suffix.is_empty() && self.known_field_names.contains(suffix) {
                            self.record_method_or_field_ref(suffix, line);
                        }
                    }
                }
                // String values that LOOK like `Type::Field` —
                // match-arm patterns auto-quote `TrafficLight::Red`
                // into a String literal. Only treat as a ref when
                // BOTH the prefix is a known Type AND the suffix is
                // a known Field, otherwise a literal user-typed
                // string like `"Foo::bar"` would false-positive.
                if let Some(s) = map.get("String").and_then(|x| x.as_str()) {
                    if let Some(idx) = s.rfind("::") {
                        let suffix = &s[idx + 2..];
                        let prefix = &s[..idx];
                        if !suffix.is_empty() && !prefix.is_empty() {
                            let prefix_tail = prefix.rsplit("::").next().unwrap_or(prefix);
                            let prefix_is_type = self.known_type_names.contains(prefix)
                                || self.known_type_names.contains(prefix_tail);
                            let suffix_is_field = self.known_field_names.contains(suffix);
                            if prefix_is_type && suffix_is_field {
                                self.record_method_or_field_ref(suffix, line);
                            }
                        }
                    }
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
                // MethodCall — `$obj->method(args)`. The method name
                // is a Field/method symbol; record it so rename /
                // goto-def on a class method works from any caller
                // even when reached via a wrapping expression
                // (Print, BinOp, etc.) that the generic walker
                // dispatches through walk_json.
                if let Some(mc) = map.get("MethodCall").and_then(|x| x.as_object()) {
                    if let Some(method) = mc.get("method").and_then(|x| x.as_str()) {
                        self.record_method_or_field_ref(method, line);
                    }
                }
                // Loop-label refs: `last LOOP`/`next LOOP`/`redo LOOP`
                // serialize as `{"Last": "LOOP"}` etc. (None as `null`).
                // Catches cases where the Last is wrapped in a postfix
                // `if`/`unless` (`last LOOP if cond` → `If { body: [Last("LOOP")] }`),
                // where the explicit walk_stmt arm never sees it.
                for key in ["Last", "Next", "Redo"] {
                    if let Some(name) = map.get(key).and_then(|x| x.as_str()) {
                        self.record_ref(name, line);
                    }
                }
                // `\&named_one` — coderef-of operator.
                if let Some(s) = map.get("SubroutineCodeRef").and_then(|x| x.as_str()) {
                    self.record_ref(s, line);
                }
                // Element / slice access where the container is in a
                // bare `String` field (not a `HashVar`/`ArrayVar`):
                // record refs sigil-aliased so the renamer can rewrite
                // `$h{k}` / `@h{k1,k2}` / `$arr[i]` while still
                // resolving to the canonical `%h` / `@arr` symbol.
                if let Some(e) = map.get("HashElement").and_then(|x| x.as_object()) {
                    if let Some(s) = e.get("hash").and_then(|x| x.as_str()) {
                        self.record_aliased_ref(&format!("%{s}"), &format!("${s}"), line);
                    }
                }
                if let Some(e) = map.get("ArrayElement").and_then(|x| x.as_object()) {
                    if let Some(s) = e.get("array").and_then(|x| x.as_str()) {
                        self.record_aliased_ref(&format!("@{s}"), &format!("${s}"), line);
                    }
                }
                if let Some(e) = map.get("HashSlice").and_then(|x| x.as_object()) {
                    if let Some(s) = e.get("hash").and_then(|x| x.as_str()) {
                        self.record_aliased_ref(&format!("%{s}"), &format!("@{s}"), line);
                    }
                }
                if let Some(e) = map.get("ArraySlice").and_then(|x| x.as_object()) {
                    if let Some(s) = e.get("array").and_then(|x| x.as_str()) {
                        self.record_ref(&format!("@{s}"), line);
                    }
                }
                if let Some(e) = map.get("HashKvSlice").and_then(|x| x.as_object()) {
                    if let Some(s) = e.get("hash").and_then(|x| x.as_str()) {
                        self.record_ref(&format!("%{s}"), line);
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
        // Single `:` with no lookahead → could be label-decl form
        // (`LABEL:`) which IS a boundary. The pair-aware variant
        // (`is_ident_boundary_after_pair`) is what callers actually
        // use to distinguish `::` (rejected) from `:` (accepted).
        assert!(is_ident_boundary_after(Some(':'), "foo"));
        // `::` lookahead — rejected because the bare name would
        // extend into a qualified path.
        assert!(!is_ident_boundary_after_pair(Some(':'), Some(':'), "foo"));
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
    fn build_collects_qualified_enum_variant_as_field_ref() {
        // `enum Color { Red }; my $c = Color::Red` — `Color::Red`
        // should record a Field-ref on "Red" at the `my $c` line.
        let src = "enum Color { Red }\nmy $c = Color::Red\n";
        let t = build(src);
        let red_id = t
            .symbols
            .iter()
            .find(|s| s.name == "Red" && s.kind == SymbolKind::Field)
            .map(|s| s.id)
            .expect("expected Field symbol `Red`");
        let line2_refs: Vec<&SymbolRef> = t
            .refs
            .iter()
            .filter(|r| r.line == 1 && r.symbol == red_id)
            .collect();
        assert!(
            !line2_refs.is_empty(),
            "expected ref to `Red` recorded at the `my $c = Color::Red` line — refs: {:#?}",
            t.refs
        );
    }

    #[test]
    fn build_collects_loop_label_declaration_and_refs() {
        let src = "OUTER: for my $i (1..3) {\n    last OUTER if $i == 2\n}\n";
        let t = build(src);
        let labels: Vec<_> = t
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Label)
            .collect();
        assert_eq!(
            labels.len(),
            1,
            "expected 1 Label symbol, got {:?}",
            t.symbols
        );
        let lbl = &labels[0];
        assert_eq!(lbl.name, "OUTER");
        assert_eq!(lbl.decl_line, 0);
        let refs: Vec<_> = t.refs.iter().filter(|r| r.name == "OUTER").collect();
        assert!(
            !refs.is_empty(),
            "expected at least one `last OUTER` ref, got refs={:?}",
            t.refs
        );
    }

    #[test]
    fn build_collects_use_constant_declarations() {
        let t = build("use constant FOO => 1\nuse constant { A => 2, B => 3 }\np FOO\np A\n");
        let names: Vec<_> = t
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Sub)
            .map(|s| s.name.as_str())
            .collect();
        assert!(names.iter().any(|n| n.ends_with("::FOO") || *n == "FOO"));
        assert!(names.iter().any(|n| n.ends_with("::A") || *n == "A"));
        assert!(names.iter().any(|n| n.ends_with("::B") || *n == "B"));
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
