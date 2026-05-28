//! Static analysis pass for detecting undefined variables and subroutines.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::ast::{
    Block, DerefKind, Expr, ExprKind, MatchArrayElem, Program, Sigil, Statement, StmtKind,
    StringPart, SubSigParam,
};
use crate::error::{ErrorKind, StrykeError, StrykeResult};

static BUILTINS: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn builtins() -> &'static HashSet<&'static str> {
    BUILTINS.get_or_init(|| {
        include_str!("lsp_completion_words.txt")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    })
}

#[derive(Default)]
struct Scope {
    scalars: HashSet<String>,
    arrays: HashSet<String>,
    hashes: HashSet<String>,
    subs: HashSet<String>,
    /// Scalar → Type name when the scalar's initializer is a known
    /// constructor expression (`Point(x=>1)`, `Point->new(x=>1)`).
    /// Drives the `$obj->method` typo-catch in MethodCall analysis.
    scalar_types: HashMap<String, String>,
}

impl Scope {
    fn declare_scalar(&mut self, name: &str) {
        self.scalars.insert(name.to_string());
    }
    fn declare_array(&mut self, name: &str) {
        self.arrays.insert(name.to_string());
    }
    fn declare_hash(&mut self, name: &str) {
        self.hashes.insert(name.to_string());
    }
    fn declare_sub(&mut self, name: &str) {
        self.subs.insert(name.to_string());
    }
}

pub struct StaticAnalyzer {
    scopes: Vec<Scope>,
    errors: Vec<StrykeError>,
    file: String,
    current_package: String,
    /// When `false` (the `stryke check` default), strict-vars-style
    /// "Global symbol \"$x\" requires explicit package name" errors are
    /// suppressed — `stryke check` is a parse / compile gate, not a
    /// strict-vars enforcer. Set to `true` only when the source itself
    /// has `use strict;` (or `use strict 'vars';`), in which case we
    /// emit so the analyzer surfaces the same diagnostics the runtime
    /// would. Topic vars (`$_0`, `@_1`, …) and special vars (`$_`,
    /// `@ARGV`, `%ENV`, …) stay exempt regardless.
    strict_vars: bool,
    /// Canonicalized paths of files already walked for `require` so the
    /// declaration sweep doesn't loop on `require A` / `require B` /
    /// `require A` cycles.
    seen_required_files: HashSet<PathBuf>,
    /// Per-type field-name sets. Populated during the declaration
    /// sweep so that calls like `Point->new(x => 10, yyg => 20)` can
    /// be checked against the known fields of `Point` — `yyg` is
    /// not a field, so the linter emits a diagnostic.
    type_fields: HashMap<String, HashSet<String>>,
    /// Per-type method-name sets — used together with `type_fields`
    /// for the `$self->X` check inside class/struct method bodies.
    /// A `$self->method_or_field` access is valid only when the name
    /// is either a field of the enclosing class or a method on it.
    type_methods: HashMap<String, HashSet<String>>,
    /// Per-type parent list — `class Dog extends Animal, Trainable`
    /// records `Dog → [Animal, Trainable]`. Drives the inherited-
    /// method lookup so `$self->trail` resolves to `Animal::trail`
    /// instead of being flagged as unknown on `Dog`.
    type_parents: HashMap<String, Vec<String>>,
    /// The class/struct being analyzed inside a method body. `None`
    /// outside any type's body. Used to resolve `$self->X` against
    /// the right type's fields + methods.
    current_class: Option<String>,
}

impl StaticAnalyzer {
    pub fn new(file: &str) -> Self {
        Self::with_strict_vars(file, false)
    }

    pub fn with_strict_vars(file: &str, strict_vars: bool) -> Self {
        let mut global = Scope::default();
        for name in ["_", "a", "b", "ARGV", "ENV", "SIG", "INC"] {
            global.declare_array(name);
        }
        for name in ["ENV", "SIG", "INC"] {
            global.declare_hash(name);
        }
        for name in [
            "_", "a", "b", "!", "$", "@", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "&",
            "`", "'", "+", ".", "/", "\\", "|", "%", "=", "-", "~", "^", "*", "?", "\"",
        ] {
            global.declare_scalar(name);
        }
        Self {
            scopes: vec![global],
            errors: Vec::new(),
            file: file.to_string(),
            current_package: "main".to_string(),
            strict_vars,
            seen_required_files: HashSet::new(),
            type_fields: HashMap::new(),
            type_methods: HashMap::new(),
            type_parents: HashMap::new(),
            current_class: None,
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn declare_scalar(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.declare_scalar(name);
        }
    }

    fn declare_array(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.declare_array(name);
        }
    }

    fn declare_hash(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.declare_hash(name);
        }
    }

    fn declare_sub(&mut self, name: &str) {
        if let Some(scope) = self.scopes.first_mut() {
            scope.declare_sub(name);
        }
    }

    /// Record that scalar `$name` was bound to an instance of `type`.
    /// Stored on the innermost active scope so nested blocks shadow
    /// outer bindings the same way variable scoping already works.
    fn declare_scalar_type(&mut self, name: &str, ty: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.scalar_types.insert(name.to_string(), ty.to_string());
        }
    }

    /// Walk scopes outer-to-inner. Returns the Type name `$name` was
    /// last bound to via a known constructor, if any.
    fn resolve_scalar_type(&self, name: &str) -> Option<&str> {
        for s in self.scopes.iter().rev() {
            if let Some(t) = s.scalar_types.get(name) {
                return Some(t.as_str());
            }
        }
        None
    }

    /// If `init` is `Type(...)` or `Type->new(...)` AND the bare tail
    /// resolves to a declared Type, return that type name. The lint
    /// only fires when the file declares the Type — same gating rule
    /// already used by `check_constructor_keys` callers.
    fn infer_constructor_type(&self, init: &Expr) -> Option<String> {
        match &init.kind {
            ExprKind::FuncCall { name, .. } => {
                let bare = name.rsplit("::").next().unwrap_or(name);
                if self.type_fields.contains_key(name) {
                    return Some(name.clone());
                }
                if self.type_fields.contains_key(bare) {
                    return Some(bare.to_string());
                }
                None
            }
            ExprKind::MethodCall { object, method, .. } if method == "new" => {
                if let ExprKind::Bareword(n) = &object.kind {
                    let bare = n.rsplit("::").next().unwrap_or(n);
                    if self.type_fields.contains_key(n) {
                        return Some(n.clone());
                    }
                    if self.type_fields.contains_key(bare) {
                        return Some(bare.to_string());
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn is_scalar_defined(&self, name: &str) -> bool {
        if is_special_var(name) || is_topic_var(name) {
            return true;
        }
        self.scopes.iter().rev().any(|s| s.scalars.contains(name))
    }

    fn is_array_defined(&self, name: &str) -> bool {
        if is_special_var(name) || is_topic_var(name) {
            return true;
        }
        self.scopes.iter().rev().any(|s| s.arrays.contains(name))
    }

    fn is_hash_defined(&self, name: &str) -> bool {
        if is_special_var(name) || is_topic_var(name) {
            return true;
        }
        self.scopes.iter().rev().any(|s| s.hashes.contains(name))
    }

    fn is_sub_defined(&self, name: &str) -> bool {
        // Late static binding: static::method() is always valid (runtime-resolved)
        if name.starts_with("static::") {
            return true;
        }
        // Compiler-generated calls emitted by parser desugaring — not
        // in the user-facing `%b` builtin set but always valid. Keep
        // this list literal — every entry must correspond to a real
        // dispatch arm in `builtins.rs`.
        if matches!(
            name,
            "_thread_par_run"
                | "__stryke_rust_compile"
                | "defer__internal"
                // Parser-level constructor specials — handled by
                // compiler.rs / vm_helper.rs as if they were built-in,
                // but don't register through the normal `%b` path.
                | "deque",
        ) {
            return true;
        }
        let base = name.rsplit("::").next().unwrap_or(name);
        if builtins().contains(base) {
            return true;
        }
        self.scopes
            .iter()
            .rev()
            .any(|s| s.subs.contains(name) || s.subs.contains(base))
    }

    fn error(&mut self, kind: ErrorKind, msg: String, line: usize) {
        self.errors
            .push(StrykeError::new(kind, msg, line, &self.file));
    }

    pub fn analyze(mut self, program: &Program) -> StrykeResult<()> {
        for stmt in &program.statements {
            self.collect_declarations_stmt(stmt);
        }
        for stmt in &program.statements {
            self.analyze_stmt(stmt);
        }
        if let Some(e) = self.errors.into_iter().next() {
            Err(e)
        } else {
            Ok(())
        }
    }

    fn collect_declarations_stmt(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StmtKind::Package { name } => {
                self.current_package = name.clone();
            }
            StmtKind::SubDecl { name, .. } => {
                let fqn = if name.contains("::") {
                    name.clone()
                } else {
                    format!("{}::{}", self.current_package, name)
                };
                self.declare_sub(name);
                self.declare_sub(&fqn);
            }
            StmtKind::Use { module, imports } => {
                self.declare_sub(module);
                // `use constant NAME => …`, `use constant { A => 1, B => 2 }`,
                // and `use constant NAME => (1, 2, 3)` install one sub per
                // NAME. Recognize those name slots so the linter can resolve
                // the constants the program references later.
                if module == "constant" {
                    self.collect_use_constant_names(imports);
                }
            }
            // `require "./lib/foo.stk"` / `require Foo::Bar` — parse the
            // pulled-in file and register its sub declarations so callers
            // like `Project::Foo::bar()` are not flagged as undefined.
            // Postfix-modifier `require … if COND;` lowers to a
            // `PostfixIf`-wrapped expression inside an Expression statement,
            // so this walker hits the inner Require either way.
            StmtKind::Expression(e) => self.collect_required_subs_from_expr(e),
            StmtKind::Block(b)
            | StmtKind::StmtGroup(b)
            | StmtKind::Begin(b)
            | StmtKind::End(b)
            | StmtKind::UnitCheck(b)
            | StmtKind::Check(b)
            | StmtKind::Init(b) => {
                for s in b {
                    self.collect_declarations_stmt(s);
                }
            }
            StmtKind::If {
                body,
                elsifs,
                else_block,
                ..
            } => {
                for s in body {
                    self.collect_declarations_stmt(s);
                }
                for (_, b) in elsifs {
                    for s in b {
                        self.collect_declarations_stmt(s);
                    }
                }
                if let Some(b) = else_block {
                    for s in b {
                        self.collect_declarations_stmt(s);
                    }
                }
            }
            StmtKind::ClassDecl { def } => {
                self.declare_sub(&def.name);
                for m in &def.methods {
                    if m.is_static {
                        self.declare_sub(&format!("{}::{}", def.name, m.name));
                    }
                }
                for sf in &def.static_fields {
                    self.declare_sub(&format!("{}::{}", def.name, sf.name));
                }
                let mut fields: HashSet<String> = HashSet::new();
                for f in &def.fields {
                    fields.insert(f.name.clone());
                }
                self.type_fields.insert(def.name.clone(), fields);
                // Instance + static methods for `$self->X` diagnostics.
                let mut methods: HashSet<String> = HashSet::new();
                for m in &def.methods {
                    methods.insert(m.name.clone());
                }
                self.type_methods.insert(def.name.clone(), methods);
                // Parent classes + implemented traits feed the
                // inheritance/trait walker so `$self->X` resolves via
                // any reachable type. `class Dog extends Animal impl
                // Trainable` — both Animal AND Trainable contribute
                // methods.
                let mut parents = def.extends.clone();
                parents.extend(def.implements.iter().cloned());
                if !parents.is_empty() {
                    self.type_parents.insert(def.name.clone(), parents);
                }
            }
            StmtKind::StructDecl { def } => {
                self.declare_sub(&def.name);
                let mut fields: HashSet<String> = HashSet::new();
                for f in &def.fields {
                    fields.insert(f.name.clone());
                }
                self.type_fields.insert(def.name.clone(), fields);
                let mut methods: HashSet<String> = HashSet::new();
                for m in &def.methods {
                    methods.insert(m.name.clone());
                }
                self.type_methods.insert(def.name.clone(), methods);
            }
            StmtKind::EnumDecl { def } => {
                self.declare_sub(&def.name);
                for v in &def.variants {
                    self.declare_sub(&format!("{}::{}", def.name, v.name));
                }
                // Variants form the "field" set for diagnostic purposes
                // (enums don't have fat-comma constructor calls, but
                // record anyway for completeness).
                let mut fields: HashSet<String> = HashSet::new();
                for v in &def.variants {
                    fields.insert(v.name.clone());
                }
                self.type_fields.insert(def.name.clone(), fields);
            }
            // Trait declarations contribute their method set so classes
            // that `impl Trait` can inherit them through the parent
            // chain. Without this, `Person impl Greetable` with a
            // default `fn greeting { ... }` on the trait wouldn't
            // resolve `$p->greeting`.
            StmtKind::TraitDecl { def } => {
                self.declare_sub(&def.name);
                let mut methods: HashSet<String> = HashSet::new();
                for m in &def.methods {
                    methods.insert(m.name.clone());
                }
                self.type_methods.insert(def.name.clone(), methods);
                // Empty field set so the type_fields key exists (drives
                // the constructor-key check + hierarchy walker entry).
                self.type_fields.entry(def.name.clone()).or_default();
            }
            _ => {}
        }
    }

    /// Register every NAME slot from a `use constant ...` import list so
    /// later references parse as defined subs. Handles all three documented
    /// shapes:
    ///   use constant NAME => VALUE
    ///   use constant NAME => (V1, V2, ...)
    ///   use constant { N1 => V1, N2 => V2, ... }
    fn collect_use_constant_names(&mut self, imports: &[Expr]) {
        for imp in imports {
            match &imp.kind {
                ExprKind::List(items) => {
                    let mut i = 0;
                    while i + 1 < items.len() {
                        if let Some(name) = static_string_value(&items[i]) {
                            let fqn = format!("{}::{}", self.current_package, name);
                            self.declare_sub(&name);
                            self.declare_sub(&fqn);
                        }
                        i += 2;
                    }
                }
                ExprKind::HashRef(pairs) => {
                    for (k, _) in pairs {
                        if let Some(name) = static_string_value(k) {
                            let fqn = format!("{}::{}", self.current_package, name);
                            self.declare_sub(&name);
                            self.declare_sub(&fqn);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Pull sub declarations out of a `require "./path"` / `require Module`
    /// inside an expression. The analyzer scans the required file's AST so
    /// callers of imported subs aren't flagged as undefined.
    ///
    /// Only static string-literal paths (`require "./lib/foo.stk"`) and
    /// bareword module specs (`require Foo::Bar`) are followed. Dynamic
    /// `require $var` is skipped — the analyzer can't know the target.
    fn collect_required_subs_from_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Require(inner) => {
                let Some(spec) = static_string_value(inner) else {
                    return;
                };
                self.follow_require(&spec);
            }
            ExprKind::PostfixIf { expr: inner, .. }
            | ExprKind::PostfixUnless { expr: inner, .. } => {
                self.collect_required_subs_from_expr(inner);
            }
            _ => {}
        }
    }

    /// Parse the file named by `spec` (relative to the current analyzed
    /// file when prefixed with `./` / `../`, otherwise treated as a Perl
    /// module specifier `Foo::Bar` → `Foo/Bar.pm`) and merge every sub
    /// declaration it contains into the analyzer's scope. Recurses into
    /// chained `require`s. Silently skips any path that fails to resolve
    /// or parse — the runtime will surface the same error if it matters.
    fn follow_require(&mut self, spec: &str) {
        let spec = spec.trim();
        if spec.is_empty() {
            return;
        }
        // Pragma-style requires (`require strict;`) install nothing
        // user-visible; skip cheaply.
        if matches!(
            spec,
            "strict"
                | "warnings"
                | "utf8"
                | "feature"
                | "v5"
                | "threads"
                | "Thread::Pool"
                | "Parallel::ForkManager"
        ) {
            return;
        }
        let Some(target) = self.resolve_require_path(spec) else {
            return;
        };
        let canon = target.canonicalize().unwrap_or(target.clone());
        if !self.seen_required_files.insert(canon.clone()) {
            return; // already walked
        }
        let Ok(src) = std::fs::read_to_string(&target) else {
            return;
        };
        let file_str = target.to_string_lossy().into_owned();
        let Ok(program) = crate::parse_module_with_file(&src, &file_str) else {
            return;
        };
        // Save and restore the caller's package — required files routinely
        // contain their own `package …;` declarations that shouldn't leak.
        let saved_pkg = std::mem::replace(&mut self.current_package, "main".to_string());
        for stmt in &program.statements {
            self.collect_declarations_stmt(stmt);
        }
        self.current_package = saved_pkg;
    }

    fn resolve_require_path(&self, spec: &str) -> Option<PathBuf> {
        resolve_require_path_from_file(&self.file, spec)
    }

    fn analyze_stmt(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StmtKind::Package { name } => {
                self.current_package = name.clone();
            }
            StmtKind::My(decls)
            | StmtKind::Our(decls)
            | StmtKind::Local(decls)
            | StmtKind::State(decls)
            | StmtKind::MySync(decls)
            | StmtKind::OurSync(decls) => {
                // `our` / `oursync` inside `package Pkg` declare a
                // package-global. References from outside the package
                // use the qualified form `$Pkg::name` — record both
                // spellings so strict-vars accepts either.
                let is_package_global =
                    matches!(stmt.kind, StmtKind::Our(_) | StmtKind::OurSync(_));
                for d in decls {
                    match d.sigil {
                        Sigil::Scalar => {
                            self.declare_scalar(&d.name);
                            if is_package_global {
                                let q = format!("{}::{}", self.current_package, d.name);
                                self.declare_scalar(&q);
                            }
                        }
                        Sigil::Array => {
                            self.declare_array(&d.name);
                            if is_package_global {
                                let q = format!("{}::{}", self.current_package, d.name);
                                self.declare_array(&q);
                            }
                        }
                        Sigil::Hash => {
                            self.declare_hash(&d.name);
                            if is_package_global {
                                let q = format!("{}::{}", self.current_package, d.name);
                                self.declare_hash(&q);
                            }
                        }
                        Sigil::Typeglob => {}
                    }
                    if let Some(init) = &d.initializer {
                        if matches!(d.sigil, Sigil::Scalar) {
                            if let Some(ty) = self.infer_constructor_type(init) {
                                self.declare_scalar_type(&d.name, &ty);
                            }
                        }
                        self.analyze_expr(init);
                    }
                }
            }
            StmtKind::Expression(e) => self.analyze_expr(e),
            StmtKind::Return(Some(e)) => self.analyze_expr(e),
            StmtKind::Return(None) => {}
            StmtKind::If {
                condition,
                body,
                elsifs,
                else_block,
            } => {
                self.analyze_expr(condition);
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
                for (cond, b) in elsifs {
                    self.analyze_expr(cond);
                    self.push_scope();
                    self.analyze_block(b);
                    self.pop_scope();
                }
                if let Some(b) = else_block {
                    self.push_scope();
                    self.analyze_block(b);
                    self.pop_scope();
                }
            }
            StmtKind::Unless {
                condition,
                body,
                else_block,
            } => {
                self.analyze_expr(condition);
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
                if let Some(b) = else_block {
                    self.push_scope();
                    self.analyze_block(b);
                    self.pop_scope();
                }
            }
            StmtKind::While {
                condition,
                body,
                continue_block,
                ..
            }
            | StmtKind::Until {
                condition,
                body,
                continue_block,
                ..
            } => {
                self.analyze_expr(condition);
                self.push_scope();
                self.analyze_block(body);
                if let Some(cb) = continue_block {
                    self.analyze_block(cb);
                }
                self.pop_scope();
            }
            StmtKind::DoWhile { body, condition } => {
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
                self.analyze_expr(condition);
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
                continue_block,
                ..
            } => {
                self.push_scope();
                if let Some(i) = init {
                    self.analyze_stmt(i);
                }
                if let Some(c) = condition {
                    self.analyze_expr(c);
                }
                if let Some(s) = step {
                    self.analyze_expr(s);
                }
                self.analyze_block(body);
                if let Some(cb) = continue_block {
                    self.analyze_block(cb);
                }
                self.pop_scope();
            }
            StmtKind::Foreach {
                var,
                list,
                body,
                continue_block,
                ..
            } => {
                self.analyze_expr(list);
                self.push_scope();
                self.declare_scalar(var);
                self.analyze_block(body);
                if let Some(cb) = continue_block {
                    self.analyze_block(cb);
                }
                self.pop_scope();
            }
            StmtKind::SubDecl {
                name, params, body, ..
            } => {
                let fqn = if name.contains("::") {
                    name.clone()
                } else {
                    format!("{}::{}", self.current_package, name)
                };
                self.declare_sub(name);
                self.declare_sub(&fqn);
                self.push_scope();
                for p in params {
                    self.declare_param(p);
                }
                self.analyze_block(body);
                self.pop_scope();
            }
            StmtKind::Block(b)
            | StmtKind::StmtGroup(b)
            | StmtKind::Begin(b)
            | StmtKind::End(b)
            | StmtKind::UnitCheck(b)
            | StmtKind::Check(b)
            | StmtKind::Init(b)
            | StmtKind::Continue(b) => {
                self.push_scope();
                self.analyze_block(b);
                self.pop_scope();
            }
            StmtKind::TryCatch {
                try_block,
                catch_var,
                catch_block,
                finally_block,
            } => {
                self.push_scope();
                self.analyze_block(try_block);
                self.pop_scope();
                self.push_scope();
                self.declare_scalar(catch_var);
                self.analyze_block(catch_block);
                self.pop_scope();
                if let Some(fb) = finally_block {
                    self.push_scope();
                    self.analyze_block(fb);
                    self.pop_scope();
                }
            }
            StmtKind::EvalTimeout { body, .. } => {
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
            }
            StmtKind::Given { topic, body } => {
                self.analyze_expr(topic);
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
            }
            StmtKind::When { cond, body } => {
                self.analyze_expr(cond);
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
            }
            StmtKind::DefaultCase { body } => {
                self.push_scope();
                self.analyze_block(body);
                self.pop_scope();
            }
            StmtKind::LocalExpr {
                target,
                initializer,
            } => {
                self.analyze_expr(target);
                if let Some(init) = initializer {
                    self.analyze_expr(init);
                }
            }
            StmtKind::Goto { target } => {
                self.analyze_expr(target);
            }
            StmtKind::Tie { class, args, .. } => {
                self.analyze_expr(class);
                for a in args {
                    self.analyze_expr(a);
                }
            }
            StmtKind::Use { imports, .. } | StmtKind::No { imports, .. } => {
                for e in imports {
                    self.analyze_expr(e);
                }
            }
            StmtKind::StructDecl { def } => {
                // Walk struct methods with `current_class` set so
                // `$self->X` body references resolve against this
                // struct's fields + methods.
                let prev = self.current_class.take();
                self.current_class = Some(def.name.clone());
                for m in &def.methods {
                    self.push_scope();
                    self.declare_scalar("self");
                    for p in &m.params {
                        self.declare_param(p);
                    }
                    self.analyze_block(&m.body);
                    self.pop_scope();
                }
                self.current_class = prev;
            }
            StmtKind::ClassDecl { def } => {
                let prev = self.current_class.take();
                self.current_class = Some(def.name.clone());
                for m in &def.methods {
                    if let Some(body) = &m.body {
                        self.push_scope();
                        self.declare_scalar("self");
                        for p in &m.params {
                            self.declare_param(p);
                        }
                        self.analyze_block(body);
                        self.pop_scope();
                    }
                }
                self.current_class = prev;
            }
            StmtKind::EnumDecl { .. }
            | StmtKind::TraitDecl { .. }
            | StmtKind::FormatDecl { .. }
            | StmtKind::AdviceDecl { .. }
            | StmtKind::UsePerlVersion { .. }
            | StmtKind::UseOverload { .. }
            | StmtKind::Last(_)
            | StmtKind::Next(_)
            | StmtKind::Redo(_)
            | StmtKind::Empty => {}
        }
    }

    fn declare_param(&mut self, param: &SubSigParam) {
        match param {
            SubSigParam::Scalar(name, _, _) => self.declare_scalar(name),
            SubSigParam::Array(name, _) => self.declare_array(name),
            SubSigParam::Hash(name, _) => self.declare_hash(name),
            SubSigParam::ArrayDestruct(elems) => {
                for e in elems {
                    match e {
                        MatchArrayElem::CaptureScalar(n) => self.declare_scalar(n),
                        MatchArrayElem::RestBind(n) => self.declare_array(n),
                        _ => {}
                    }
                }
            }
            SubSigParam::HashDestruct(pairs) => {
                for (_, name) in pairs {
                    self.declare_scalar(name);
                }
            }
        }
    }

    fn analyze_block(&mut self, block: &Block) {
        for stmt in block {
            self.analyze_stmt(stmt);
        }
    }

    /// Check `Type(field => value, …)` / `Type->new(field => value, …)`
    /// constructor calls: emit a diagnostic for each fat-comma key
    /// that doesn't match a declared field of `type_name`. No-op when
    /// `type_name` isn't a known Type (lets ordinary FuncCalls
    /// through untouched).
    fn check_constructor_keys(&mut self, type_name: &str, args: &[Expr], call_line: usize) {
        let bare_tail = type_name.rsplit("::").next().unwrap_or(type_name);
        // Pick the resolved class name (qualified or bare). Bail if the
        // file doesn't declare any Type by this name.
        let resolved = if self.type_fields.contains_key(type_name) {
            type_name.to_string()
        } else if self.type_fields.contains_key(bare_tail) {
            bare_tail.to_string()
        } else {
            return;
        };
        // Walk class + parents via `extends`, unioning every declared
        // field into one set. `Dog extends Animal { name }` + `Dog
        // { breed }` accepts `Dog(name => ..., breed => ...)`.
        let mut all_fields: HashSet<String> = HashSet::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue: Vec<String> = vec![resolved.clone()];
        while let Some(c) = queue.pop() {
            if !seen.insert(c.clone()) {
                continue;
            }
            if let Some(fs) = self.type_fields.get(&c) {
                all_fields.extend(fs.iter().cloned());
            }
            if let Some(parents) = self.type_parents.get(&c) {
                queue.extend(parents.iter().cloned());
            }
        }
        // Detect call style: fat-comma keyed (`Type(k => v, k => v)`)
        // vs positional (`Type(1, "title", Priority::High)`). Stryke
        // collapses fat-comma to plain args at parse time, so the AST
        // shapes are identical. Disambiguate by checking whether the
        // FIRST arg matches a declared field name — if not, the call
        // is positional and the key check would only generate noise.
        let first_arg_is_field = args.first().is_some_and(|a| match &a.kind {
            ExprKind::String(s) | ExprKind::Bareword(s) => all_fields.contains(s),
            _ => false,
        });
        let looks_keyed = args.len().is_multiple_of(2)
            && first_arg_is_field
            && (0..args.len()).step_by(2).all(|i| {
                matches!(&args[i].kind, ExprKind::String(_))
                    || matches!(&args[i].kind, ExprKind::Bareword(s) if !s.contains("::"))
            });
        if !looks_keyed {
            return;
        }
        let mut i = 0;
        while i < args.len() {
            let key_name = match &args[i].kind {
                ExprKind::String(s) => Some(s.clone()),
                ExprKind::Bareword(s) => Some(s.clone()),
                _ => None,
            };
            if let Some(name) = key_name {
                if !all_fields.contains(&name) {
                    let line = if args[i].line > 0 {
                        args[i].line
                    } else {
                        call_line
                    };
                    let bare_for_msg = type_name.rsplit("::").next().unwrap_or(type_name);
                    self.error(
                        ErrorKind::UndefinedSubroutine,
                        format!(
                            "Unknown field `{name}` in constructor call to `{bare_for_msg}` — \
                             declared fields: {}",
                            {
                                let mut v: Vec<&String> = all_fields.iter().collect();
                                v.sort();
                                v.into_iter()
                                    .map(String::as_str)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            }
                        ),
                        line,
                    );
                }
            }
            i += 2;
        }
    }

    /// True when `method` is a field OR method declared on `class_name`
    /// OR any ancestor in the `extends` chain. Cycle-guarded so a
    /// pathological `A extends B; B extends A` doesn't recur forever.
    fn method_resolves_in_hierarchy(&self, class_name: &str, method: &str) -> bool {
        // Universal methods inherited from `UNIVERSAL` (Perl) / every
        // stryke object. Sourced from `vm_helper.rs` built-in class +
        // struct method dispatch — keep in sync when new universal
        // methods are added.
        //
        // - `isa` / `can` / `DOES` / `does` / `VERSION` — Perl UNIVERSAL.
        // - `new` / `BUILD` / `DESTROY` / `destroy` — lifecycle hooks.
        // - `clone` — deep copy via per-field deep_clone.
        // - `with` — functional update returning a new instance with
        //   the named fields changed.
        // - `to_hash` / `to_hash_rec` / `to_hash_deep` — serialize.
        // - `fields` / `methods` / `superclass` — runtime introspection.
        if matches!(
            method,
            "isa"
                | "can"
                | "DOES"
                | "does"
                | "VERSION"
                | "new"
                | "BUILD"
                | "DESTROY"
                | "destroy"
                | "clone"
                | "with"
                | "to_hash"
                | "to_hash_rec"
                | "to_hash_deep"
                | "fields"
                | "methods"
                | "superclass"
        ) {
            return true;
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue: Vec<String> = vec![class_name.to_string()];
        while let Some(c) = queue.pop() {
            if !seen.insert(c.clone()) {
                continue;
            }
            if self.type_fields.get(&c).is_some_and(|s| s.contains(method)) {
                return true;
            }
            if self
                .type_methods
                .get(&c)
                .is_some_and(|s| s.contains(method))
            {
                return true;
            }
            if let Some(parents) = self.type_parents.get(&c) {
                queue.extend(parents.iter().cloned());
            }
        }
        false
    }

    /// Gather every field + method name visible on `class_name` and its
    /// ancestors (BFS through `extends`). Used to render the "available:
    /// …" suggestion list when a `$self->X` / `$obj->X` lookup fails.
    fn collect_hierarchy_members(&self, class_name: &str) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut out: HashSet<String> = HashSet::new();
        let mut queue: Vec<String> = vec![class_name.to_string()];
        while let Some(c) = queue.pop() {
            if !seen.insert(c.clone()) {
                continue;
            }
            if let Some(fs) = self.type_fields.get(&c) {
                out.extend(fs.iter().cloned());
            }
            if let Some(ms) = self.type_methods.get(&c) {
                out.extend(ms.iter().cloned());
            }
            if let Some(parents) = self.type_parents.get(&c) {
                queue.extend(parents.iter().cloned());
            }
        }
        let mut v: Vec<String> = out.into_iter().collect();
        v.sort();
        v
    }

    fn analyze_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            // `$#name` — the Perl last-index-of-array form. The parser
            // surfaces it as `ScalarVar("#name")`; resolve to the
            // underlying `@name` so a defined array satisfies the
            // check. Bare `$#` (no name) is the magic "last index of
            // $_" form — always defined.
            ExprKind::ScalarVar(name)
                if self.strict_vars
                    && name.len() > 1
                    && name.starts_with('#')
                    && !self.is_array_defined(&name[1..]) =>
            {
                self.error(
                    ErrorKind::UndefinedVariable,
                    format!(
                        "Global symbol \"@{}\" requires explicit package name",
                        &name[1..]
                    ),
                    expr.line,
                );
            }
            ExprKind::ScalarVar(name) if name.starts_with('#') => {
                // `$#name` with @name defined OR bare `$#` — no-op.
            }
            ExprKind::ScalarVar(name) if self.strict_vars && !self.is_scalar_defined(name) => {
                self.error(
                    ErrorKind::UndefinedVariable,
                    format!("Global symbol \"${}\" requires explicit package name", name),
                    expr.line,
                );
            }
            ExprKind::ArrayVar(name) if self.strict_vars && !self.is_array_defined(name) => {
                self.error(
                    ErrorKind::UndefinedVariable,
                    format!("Global symbol \"@{}\" requires explicit package name", name),
                    expr.line,
                );
            }
            ExprKind::HashVar(name) if self.strict_vars && !self.is_hash_defined(name) => {
                self.error(
                    ErrorKind::UndefinedVariable,
                    format!("Global symbol \"%{}\" requires explicit package name", name),
                    expr.line,
                );
            }
            ExprKind::ArrayElement { array, index } => {
                if self.strict_vars
                    && !array.starts_with("__topicstr__")
                    && !self.is_array_defined(array)
                    && !self.is_scalar_defined(array)
                {
                    self.error(
                        ErrorKind::UndefinedVariable,
                        format!(
                            "Global symbol \"@{}\" requires explicit package name",
                            array
                        ),
                        expr.line,
                    );
                }
                self.analyze_expr(index);
            }
            ExprKind::HashElement { hash, key } => {
                if self.strict_vars && !self.is_hash_defined(hash) && !self.is_scalar_defined(hash)
                {
                    self.error(
                        ErrorKind::UndefinedVariable,
                        format!("Global symbol \"%{}\" requires explicit package name", hash),
                        expr.line,
                    );
                }
                self.analyze_expr(key);
            }
            ExprKind::ArraySlice { array, indices } => {
                if self.strict_vars && !self.is_array_defined(array) {
                    self.error(
                        ErrorKind::UndefinedVariable,
                        format!(
                            "Global symbol \"@{}\" requires explicit package name",
                            array
                        ),
                        expr.line,
                    );
                }
                for i in indices {
                    self.analyze_expr(i);
                }
            }
            ExprKind::HashSlice { hash, keys } => {
                if self.strict_vars && !self.is_hash_defined(hash) {
                    self.error(
                        ErrorKind::UndefinedVariable,
                        format!("Global symbol \"%{}\" requires explicit package name", hash),
                        expr.line,
                    );
                }
                for k in keys {
                    self.analyze_expr(k);
                }
            }
            ExprKind::FuncCall { name, args } => {
                if !self.is_sub_defined(name) {
                    self.error(
                        ErrorKind::UndefinedSubroutine,
                        format!("Undefined subroutine &{}", name),
                        expr.line,
                    );
                }
                // Constructor-call form `Type(field => value)` — check
                // each fat-comma key against the Type's known fields.
                self.check_constructor_keys(name, args, expr.line);
                for a in args {
                    self.analyze_expr(a);
                }
            }
            ExprKind::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                self.analyze_expr(object);
                // Method-form constructor `Type->new(field => value)`.
                if let ExprKind::Bareword(n) = &object.kind {
                    self.check_constructor_keys(n, args, expr.line);
                    // Only flag the receiver when the file actually
                    // declares some local Types — that's the "user
                    // is doing OOP here" signal that justifies typo-
                    // catching. Without local Types, every Bareword
                    // receiver (`Foo->new`, `IO::Handle->open`, etc.)
                    // would be false-positively flagged.
                    if !self.type_fields.is_empty()
                        && !self.type_fields.contains_key(n)
                        && !self.is_sub_defined(n)
                    {
                        let bare = n.rsplit("::").next().unwrap_or(n);
                        if !bare.is_empty()
                            && bare.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                        {
                            self.error(
                                ErrorKind::UndefinedSubroutine,
                                format!(
                                    "Unknown class `{n}` — `{n}->{method}` calls a constructor on a type that isn't declared in this file or its `require`d libs"
                                ),
                                expr.line,
                            );
                        }
                    }
                }
                // `$obj->X` when `$obj` was bound to a known Type via
                // a constructor expression (`my $p = Point(...)` or
                // `my $p = Point->new(...)`). Symmetric to the
                // `$self->X` check below — both walk the type's
                // field+method sets and flag unknown names.
                if let ExprKind::ScalarVar(name) = &object.kind {
                    if name != "self" {
                        if let Some(class_name) =
                            self.resolve_scalar_type(name).map(|s| s.to_string())
                        {
                            if !self.method_resolves_in_hierarchy(&class_name, method) {
                                let suggestions =
                                    self.collect_hierarchy_members(&class_name);
                                let avail = if suggestions.is_empty() {
                                    "(no fields or methods declared)".to_string()
                                } else {
                                    suggestions.join(", ")
                                };
                                self.error(
                                    ErrorKind::UndefinedSubroutine,
                                    format!(
                                        "`${name}->{method}` — no field or method `{method}` on `{class_name}`; available: {avail}",
                                    ),
                                    expr.line,
                                );
                            }
                        }
                    }
                }
                // `$self->X` inside a class/struct body — `X` must
                // be a field or method of the enclosing type.
                if let ExprKind::ScalarVar(name) = &object.kind {
                    if name == "self" {
                        if let Some(class_name) = self.current_class.clone() {
                            // Walk class + parents via `extends`. Cycle-
                            // guarded so a broken `extends A; A extends X`
                            // loop can't infinite-loop the linter.
                            if !self.method_resolves_in_hierarchy(&class_name, method) {
                                let suggestions =
                                    self.collect_hierarchy_members(&class_name);
                                let avail = if suggestions.is_empty() {
                                    "(no fields or methods declared)".to_string()
                                } else {
                                    suggestions.join(", ")
                                };
                                self.error(
                                    ErrorKind::UndefinedSubroutine,
                                    format!(
                                        "`$self->{method}` — no field or method `{method}` on `{class_name}`; available: {avail}",
                                    ),
                                    expr.line,
                                );
                            }
                        }
                    }
                }
                for a in args {
                    self.analyze_expr(a);
                }
            }
            ExprKind::IndirectCall { target, args, .. } => {
                self.analyze_expr(target);
                for a in args {
                    self.analyze_expr(a);
                }
            }
            ExprKind::BinOp { left, right, .. } => {
                self.analyze_expr(left);
                self.analyze_expr(right);
            }
            ExprKind::UnaryOp { expr: e, .. } => {
                self.analyze_expr(e);
            }
            ExprKind::PostfixOp { expr: e, .. } => {
                self.analyze_expr(e);
            }
            ExprKind::Assign { target, value } => {
                if let ExprKind::ScalarVar(name) = &target.kind {
                    self.declare_scalar(name);
                } else if let ExprKind::ArrayVar(name) = &target.kind {
                    self.declare_array(name);
                } else if let ExprKind::HashVar(name) = &target.kind {
                    self.declare_hash(name);
                } else {
                    self.analyze_expr(target);
                }
                self.analyze_expr(value);
            }
            // `my $x = …` / `our @y = …` / `state %z = …` / `local …` used in
            // EXPRESSION position — the canonical case is `while (my $job = ...)
            // { … $job … }` and `if (my $row = $db->fetch) { … $row … }`. The
            // parser surfaces it as `ExprKind::MyExpr` (see parser.rs:8927); without
            // this arm the var is silently dropped from scope tracking and any
            // reference in the loop / branch body trips strict-vars. Mirrors the
            // `StmtKind::My`/`Our`/`State`/`Local` registration above (qualified
            // spelling for `our`, then analyze each initializer).
            ExprKind::MyExpr { keyword, decls } => {
                let is_package_global = keyword == "our";
                for d in decls {
                    match d.sigil {
                        Sigil::Scalar => {
                            self.declare_scalar(&d.name);
                            if is_package_global {
                                let q = format!("{}::{}", self.current_package, d.name);
                                self.declare_scalar(&q);
                            }
                        }
                        Sigil::Array => {
                            self.declare_array(&d.name);
                            if is_package_global {
                                let q = format!("{}::{}", self.current_package, d.name);
                                self.declare_array(&q);
                            }
                        }
                        Sigil::Hash => {
                            self.declare_hash(&d.name);
                            if is_package_global {
                                let q = format!("{}::{}", self.current_package, d.name);
                                self.declare_hash(&q);
                            }
                        }
                        Sigil::Typeglob => {}
                    }
                    if let Some(init) = &d.initializer {
                        self.analyze_expr(init);
                    }
                }
            }
            ExprKind::CompoundAssign { target, value, .. } => {
                self.analyze_expr(target);
                self.analyze_expr(value);
            }
            ExprKind::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                self.analyze_expr(condition);
                self.analyze_expr(then_expr);
                self.analyze_expr(else_expr);
            }
            ExprKind::List(exprs) | ExprKind::ArrayRef(exprs) => {
                for e in exprs {
                    self.analyze_expr(e);
                }
            }
            ExprKind::HashRef(pairs) => {
                for (k, v) in pairs {
                    self.analyze_expr(k);
                    self.analyze_expr(v);
                }
            }
            ExprKind::CodeRef { params, body } => {
                self.push_scope();
                for p in params {
                    self.declare_param(p);
                }
                self.analyze_block(body);
                self.pop_scope();
            }
            ExprKind::ScalarRef(e)
            | ExprKind::Deref { expr: e, .. }
            | ExprKind::Defined(e)
            | ExprKind::Delete(e) => {
                self.analyze_expr(e);
            }
            ExprKind::Exists(e)
                // `exists &SUB` and `exists &Pkg::sub` are introspection
                // calls — the point is to check whether the sub IS
                // defined, so flagging an "undefined" sub here is the
                // opposite of helpful. Skip the sub-defined check for
                // `SubroutineCodeRef` / `SubroutineRef` payloads;
                // everything else (hash keys, array indices) still gets
                // the normal analysis.
                if !matches!(
                    e.kind,
                    ExprKind::SubroutineCodeRef(_) | ExprKind::SubroutineRef(_)
                ) => {
                    self.analyze_expr(e);
                }
            ExprKind::ArrowDeref { expr, index, kind } => {
                self.analyze_expr(expr);
                if *kind != DerefKind::Call {
                    self.analyze_expr(index);
                }
            }
            ExprKind::Range { from, to, step, .. } => {
                self.analyze_expr(from);
                self.analyze_expr(to);
                if let Some(s) = step {
                    self.analyze_expr(s);
                }
            }
            ExprKind::SliceRange { from, to, step } => {
                if let Some(f) = from {
                    self.analyze_expr(f);
                }
                if let Some(t) = to {
                    self.analyze_expr(t);
                }
                if let Some(s) = step {
                    self.analyze_expr(s);
                }
            }
            ExprKind::InterpolatedString(parts) => {
                // Strict-vars policy inside double-quoted interpolations:
                // DON'T flag bare `$undef` / `@undef` / `%undef`. Strings
                // are commonly used as test descriptions / log messages
                // with template-style placeholders that the user doesn't
                // intend as hard variable references. Bare `$x + 1` in
                // code-context still gets flagged; the string interior
                // gets a free pass. Full `#{ EXPR }` blocks (complex
                // expressions wrapped in Expr) DO get walked, since
                // those are real code — unless the entire expression
                // is just a single sigil-var, in which case the same
                // pass-through policy applies.
                for part in parts {
                    match part {
                        StringPart::Expr(e) => {
                            // Skip the strict-vars check for the simple
                            // sigil-var-only shape (`$var` / `@var` / `%var`
                            // wrapped in Expr by the parser). For richer
                            // expressions (`$x + 1`, fn calls, etc.) walk
                            // normally.
                            match &e.kind {
                                ExprKind::ScalarVar(_)
                                | ExprKind::ArrayVar(_)
                                | ExprKind::HashVar(_) => {}
                                _ => self.analyze_expr(e),
                            }
                        }
                        StringPart::ScalarVar(_)
                        | StringPart::ArrayVar(_)
                        | StringPart::Literal(_) => {}
                    }
                }
            }
            ExprKind::Regex(_, _)
            | ExprKind::Substitution { .. }
            | ExprKind::Transliterate { .. }
            | ExprKind::Match { .. } => {}
            ExprKind::HashSliceDeref { container, keys } => {
                self.analyze_expr(container);
                for k in keys {
                    self.analyze_expr(k);
                }
            }
            ExprKind::AnonymousListSlice { source, indices } => {
                self.analyze_expr(source);
                for i in indices {
                    self.analyze_expr(i);
                }
            }
            ExprKind::SubroutineRef(name) | ExprKind::SubroutineCodeRef(name)
                if !self.is_sub_defined(name) =>
            {
                self.error(
                    ErrorKind::UndefinedSubroutine,
                    format!("Undefined subroutine &{}", name),
                    expr.line,
                );
            }
            ExprKind::DynamicSubCodeRef(e) => self.analyze_expr(e),
            ExprKind::PostfixIf { expr, condition }
            | ExprKind::PostfixUnless { expr, condition }
            | ExprKind::PostfixWhile { expr, condition }
            | ExprKind::PostfixUntil { expr, condition } => {
                self.analyze_expr(expr);
                self.analyze_expr(condition);
            }
            ExprKind::PostfixForeach { expr, list } => {
                self.analyze_expr(list);
                self.analyze_expr(expr);
            }
            ExprKind::Do(e) | ExprKind::Eval(e) => {
                self.analyze_expr(e);
            }
            ExprKind::Caller(Some(e)) => {
                self.analyze_expr(e);
            }
            ExprKind::Length(e) => {
                self.analyze_expr(e);
            }
            ExprKind::Print { args, .. }
            | ExprKind::Say { args, .. }
            | ExprKind::Printf { args, .. } => {
                for a in args {
                    self.analyze_expr(a);
                }
            }
            ExprKind::Die(args)
            | ExprKind::Warn(args)
            | ExprKind::Unlink(args)
            | ExprKind::Chmod(args)
            | ExprKind::System(args)
            | ExprKind::Exec(args) => {
                for a in args {
                    self.analyze_expr(a);
                }
            }
            ExprKind::Push { array, values } | ExprKind::Unshift { array, values } => {
                self.analyze_expr(array);
                for v in values {
                    self.analyze_expr(v);
                }
            }
            ExprKind::Splice {
                array,
                offset,
                length,
                replacement,
            } => {
                self.analyze_expr(array);
                if let Some(o) = offset {
                    self.analyze_expr(o);
                }
                if let Some(l) = length {
                    self.analyze_expr(l);
                }
                for r in replacement {
                    self.analyze_expr(r);
                }
            }
            ExprKind::MapExpr { block, list, .. } | ExprKind::GrepExpr { block, list, .. } => {
                self.push_scope();
                self.analyze_block(block);
                self.pop_scope();
                self.analyze_expr(list);
            }
            ExprKind::SortExpr { list, .. } => {
                self.analyze_expr(list);
            }
            ExprKind::Open { handle, mode, file } => {
                // `open(my $fh, ">", $path)` — `my $fh` is the lexical-
                // filehandle declaration Perl idiom. Register the name
                // so later `print $fh ...` / `close $fh` lookups pass.
                if let ExprKind::OpenMyHandle { name } = &handle.kind {
                    self.declare_scalar(name);
                } else {
                    self.analyze_expr(handle);
                }
                self.analyze_expr(mode);
                if let Some(f) = file {
                    self.analyze_expr(f);
                }
            }
            ExprKind::Close(e)
            | ExprKind::Pop(e)
            | ExprKind::Shift(e)
            | ExprKind::Keys(e)
            | ExprKind::Values(e)
            | ExprKind::Each(e)
            | ExprKind::Chdir(e)
            | ExprKind::Require(e)
            | ExprKind::Ref(e)
            | ExprKind::Chomp(e)
            | ExprKind::Chop(e)
            | ExprKind::Lc(e)
            | ExprKind::Uc(e)
            | ExprKind::Lcfirst(e)
            | ExprKind::Ucfirst(e)
            | ExprKind::Abs(e)
            | ExprKind::Int(e)
            | ExprKind::Sqrt(e)
            | ExprKind::Sin(e)
            | ExprKind::Cos(e)
            | ExprKind::Exp(e)
            | ExprKind::Log(e)
            | ExprKind::Chr(e)
            | ExprKind::Ord(e)
            | ExprKind::Hex(e)
            | ExprKind::Oct(e)
            | ExprKind::Readlink(e)
            | ExprKind::Readdir(e)
            | ExprKind::Closedir(e)
            | ExprKind::Rewinddir(e)
            | ExprKind::Telldir(e) => {
                self.analyze_expr(e);
            }
            ExprKind::Exit(Some(e)) | ExprKind::Rand(Some(e)) | ExprKind::Eof(Some(e)) => {
                self.analyze_expr(e);
            }
            ExprKind::Mkdir { path, mode } => {
                self.analyze_expr(path);
                if let Some(m) = mode {
                    self.analyze_expr(m);
                }
            }
            ExprKind::Rename { old, new }
            | ExprKind::Link { old, new }
            | ExprKind::Symlink { old, new } => {
                self.analyze_expr(old);
                self.analyze_expr(new);
            }
            ExprKind::Chown(files) => {
                for f in files {
                    self.analyze_expr(f);
                }
            }
            ExprKind::Substr {
                string,
                offset,
                length,
                replacement,
            } => {
                self.analyze_expr(string);
                self.analyze_expr(offset);
                if let Some(l) = length {
                    self.analyze_expr(l);
                }
                if let Some(r) = replacement {
                    self.analyze_expr(r);
                }
            }
            ExprKind::Index {
                string,
                substr,
                position,
            }
            | ExprKind::Rindex {
                string,
                substr,
                position,
            } => {
                self.analyze_expr(string);
                self.analyze_expr(substr);
                if let Some(p) = position {
                    self.analyze_expr(p);
                }
            }
            ExprKind::Sprintf { format, args } => {
                self.analyze_expr(format);
                for a in args {
                    self.analyze_expr(a);
                }
            }
            ExprKind::Bless { ref_expr, class } => {
                self.analyze_expr(ref_expr);
                if let Some(c) = class {
                    self.analyze_expr(c);
                }
            }
            ExprKind::AlgebraicMatch { subject, arms } => {
                self.analyze_expr(subject);
                for arm in arms {
                    self.check_match_pattern(&arm.pattern, expr.line);
                    if let Some(g) = &arm.guard {
                        self.analyze_expr(g);
                    }
                    self.analyze_expr(&arm.body);
                }
            }
            _ => {}
        }
    }

    /// Walk a match arm pattern. The only shape that gets a typo check
    /// is `MatchPattern::Value(ExprKind::String("Type::Variant"))` —
    /// the parser auto-quotes bareword enum patterns into String, so
    /// `Sig::Hup => "..."` arrives here as a String literal, not a
    /// FuncCall (which would have already been linted as undefined sub).
    /// Other pattern shapes (Regex, Array, Hash, Any, OptionSome) walk
    /// their inner exprs for ordinary var-defined-ness checks.
    fn check_match_pattern(&mut self, pat: &crate::ast::MatchPattern, line: usize) {
        use crate::ast::{MatchArrayElem, MatchHashPair, MatchPattern};
        match pat {
            MatchPattern::Value(e) => {
                if let ExprKind::String(s) = &e.kind {
                    self.check_qualified_variant_string(s, line);
                }
                self.analyze_expr(e);
            }
            MatchPattern::Array(elems) => {
                for el in elems {
                    if let MatchArrayElem::Expr(e) = el {
                        self.analyze_expr(e);
                    }
                }
            }
            MatchPattern::Hash(pairs) => {
                for p in pairs {
                    match p {
                        MatchHashPair::KeyOnly { key } => self.analyze_expr(key),
                        MatchHashPair::Capture { key, .. } => self.analyze_expr(key),
                    }
                }
            }
            MatchPattern::Any | MatchPattern::Regex { .. } | MatchPattern::OptionSome(_) => {}
        }
    }

    /// `Sig::Term2` arriving as an auto-quoted match-arm pattern. If
    /// `Sig` is a known enum and `Term2` isn't one of its variants,
    /// emit a typo-catching diagnostic with the available variants
    /// listed. Same shape as the `$obj->method` / `Type->new(field => v)`
    /// checks already in place.
    fn check_qualified_variant_string(&mut self, s: &str, line: usize) {
        let Some(idx) = s.rfind("::") else { return };
        let type_name = &s[..idx];
        let variant = &s[idx + 2..];
        if type_name.is_empty() || variant.is_empty() {
            return;
        }
        let type_bare = type_name.rsplit("::").next().unwrap_or(type_name);
        let known = self
            .type_fields
            .get(type_name)
            .or_else(|| self.type_fields.get(type_bare));
        let Some(variants) = known else { return };
        if variants.contains(variant) {
            return;
        }
        let mut available: Vec<&str> = variants.iter().map(String::as_str).collect();
        available.sort();
        let avail = if available.is_empty() {
            "(no variants declared)".to_string()
        } else {
            available.join(", ")
        };
        self.error(
            ErrorKind::UndefinedSubroutine,
            format!(
                "`{type_name}::{variant}` — no variant `{variant}` on `{type_name}`; available: {avail}"
            ),
            line,
        );
    }
}

fn is_special_var(name: &str) -> bool {
    // `main::X` is the qualified form of `X` for every reserved
    // variable — `$main::ARGV`, `@main::INC`, `%main::ENV`, `$main::_`,
    // etc. (per the Perl Documentation: "certain built-in identifiers
    // are forced into the main package"). Strip the prefix and test
    // the bare name so the linter doesn't flag the qualified form as
    // an undeclared global. Recursing also handles
    // `%main::stryke::all` — strip `main::` once, then `stryke::all`
    // resolves through the registry's `%stryke::*` reflection family.
    if let Some(rest) = name.strip_prefix("main::") {
        return is_special_var(rest);
    }
    if name.len() == 1 {
        return true;
    }
    // Perl `^X`-style special vars: `$^X` (interpreter path),
    // `$^O` (OS), `$^V` (version), `$^W` (warnings), `$^T`, `$^R`,
    // `$^N`, `$^H`, `$^I`, `$^L`, `$^A`, `$^C`, `$^B`, `$^D`,
    // `$^E`, `$^F`, `$^G`, `$^M`, `$^P`, `$^S`, `$^U`, plus the
    // `$^{NAME}` long forms (`$^{MATCH}`, `$^{POSTMATCH}`, etc.).
    if name.starts_with('^') {
        return true;
    }
    // `$$` — process id. Parser stores it with the sigil included
    // (`ScalarVar("$$")`), so the strict-vars check sees a 2-char
    // name. Same shape for `$)`, `$(`, `$/`, etc. when carried with
    // their literal punctuation as the "name" part.
    if name == "$$" {
        return true;
    }
    // Sigil-agnostic check against the canonical SPECIAL_VARS registry in
    // builtins.rs (the single source of truth used by `%v` / `%stryke::special_vars`
    // and runtime `is_special_var`). The registry stores spellings WITH sigil
    // (`%all`, `%limits`, `%stryke::aliases`, `$stryke::VERSION`, …); the
    // strict-vars walker calls us with sigil STRIPPED, so we test all three
    // sigil prefixes. Catches the reflection hashes (`%all`, `%limits`,
    // `%parameters`, `%pc`, `%term`, `%uname`, the `%stryke::*` family,
    // `%overload::`), `__FILE__`/`__LINE__`/`__PACKAGE__`/`__SUB__`, and any
    // future special-var addition without further edits here.
    if crate::builtins::is_special_var(&format!("${}", name))
        || crate::builtins::is_special_var(&format!("@{}", name))
        || crate::builtins::is_special_var(&format!("%{}", name))
        || crate::builtins::is_special_var(name)
    {
        return true;
    }
    matches!(
        name,
        "ARGV"
            | "ENV"
            | "SIG"
            | "INC"
            | "AUTOLOAD"
            | "STDERR"
            | "STDIN"
            | "STDOUT"
            | "DATA"
            | "UNIVERSAL"
            | "VERSION"
            | "ISA"
            | "EXPORT"
            | "EXPORT_OK"
            // AOP advice-body context vars — declared by the VM at the
            // entry of every `before`/`after`/`around` body. Visible
            // outside an advice block as ordinary globals (cheap, no
            // pollution), so always-defined is the right model.
            | "INTERCEPT_NAME"
            | "INTERCEPT_ARGS"
            | "INTERCEPT_RESULT"
            | "INTERCEPT_MS"
            | "INTERCEPT_US"
            // stryke-VERSION qualifiers + meta-constants commonly
            // referenced in module headers and never declared.
            | "stryke::VERSION"
    )
}

/// Stryke implicit closure-positional slots — `_0`, `_1`, …, `_99`. These
/// are auto-bound inside any block that takes positional args (sort
/// comparators, reduce blocks, sub bodies, map/grep blocks) and must never
/// be flagged as undeclared by `stryke check` regardless of strict mode.
/// Mirrors the scalar exemption at `vm_helper.rs:strict_scalar_exempt`,
/// extended uniformly to scalar / array / hash sigils — `$_1`, `@_1[0]`,
/// `%_1{k}` are all legitimate topic-var spellings.
fn is_topic_var(name: &str) -> bool {
    // Stryke block-param grammar (sigil already stripped):
    //   `_`                — bare topic
    //   `_N`               — Nth positional arg
    //   `_<<<<<`           — outer-chain, any depth of `<`
    //   `_<N`              — indexed-ascent shortcut
    //   `_N<<<<<` / `_N<M` — positional + outer chain combined
    // Pattern: `_` then (digits? then chevrons? then digits?), with at
    // least one chevron OR digit after `_<` to disambiguate from a
    // bare `_<` operator pair.
    if !name.starts_with('_') {
        return false;
    }
    let rest = &name[1..];
    if rest.is_empty() {
        return true; // bare `_`
    }
    let bytes = rest.as_bytes();
    let mut i = 0;
    // Optional positional digits.
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let digits_consumed = i;
    if i == bytes.len() {
        // `_N` — pure positional. Must have at least one digit.
        return digits_consumed > 0;
    }
    // Optional `<...` outer-chain segment.
    if bytes[i] != b'<' {
        return false;
    }
    while i < bytes.len() && bytes[i] == b'<' {
        i += 1;
    }
    // Optional trailing digits (indexed-ascent shortcut).
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i == bytes.len()
}

/// Map a `require` spec to a filesystem path. `./path` and `../path` resolve
/// against the project root derived from the source file's location. Perl
/// convention: scripts under `t/` / `tests/` / `test/` / `spec/` / `xt/`
/// sit one level below the project root that holds `lib/`. Any other layout:
/// the file's own directory IS the project root. One path computation, no
/// walking. Bareword `Foo::Bar` becomes `<root>/lib/Foo/Bar.pm`. Absolute
/// paths pass through. Shared by the static analyzer's require-follower
/// and the LSP go-to-definition cross-file lookup.
pub fn resolve_require_path_from_file(file: &str, spec: &str) -> Option<PathBuf> {
    let p = Path::new(spec);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    let file_dir = Path::new(file)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    // Detect project root by walking UP from the source file's
    // directory looking for a sibling `lib/`. Typical Perl/CPAN
    // layout: project/lib/Foo/Bar.pm with `require "./lib/Foo/Bar.stk"`
    // anywhere in the tree resolving back to project/. Falls back to:
    // (1) the file's own directory if no ancestor has `lib/`;
    // (2) the parent directory when the file sits in `t/tests/test/
    // spec/xt` (the classic test layout).
    let project_root = find_project_root(&file_dir);
    // Also try the file's own directory as a same-dir fallback (e.g.
    // siblings in the same folder use `./sibling.stk`).
    let candidate_via_root = if spec.starts_with("./") || spec.starts_with("../") {
        project_root.join(spec)
    } else if spec.contains("::") {
        let relpath: PathBuf = PathBuf::from(spec.replace("::", "/")).with_extension("pm");
        project_root.join("lib").join(relpath)
    } else {
        project_root.join(spec)
    };
    if candidate_via_root.exists() {
        return Some(candidate_via_root);
    }
    // Fallback: resolve `./foo.stk` against the file's own directory
    // for the simple sibling-script case.
    if spec.starts_with("./") || spec.starts_with("../") {
        let direct = file_dir.join(spec);
        if direct.exists() {
            return Some(direct);
        }
    }
    None
}

/// Walk UP from `start_dir` looking for an ancestor that has a `lib/`
/// subdirectory (typical Perl/CPAN project layout). Returns the
/// first such ancestor, falling back to the original directory
/// (or its parent when it's `t`/`tests`/`test`/`spec`/`xt`).
fn find_project_root(start_dir: &Path) -> PathBuf {
    let mut cur = start_dir.to_path_buf();
    for _ in 0..16 {
        // capped depth — pathological infinite-symlink protection
        if cur.join("lib").is_dir() {
            return cur;
        }
        match cur.parent() {
            Some(p) if p != cur => cur = p.to_path_buf(),
            _ => break,
        }
    }
    match start_dir.file_name().and_then(|s| s.to_str()) {
        Some("t") | Some("tests") | Some("test") | Some("spec") | Some("xt") => start_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| start_dir.to_path_buf()),
        _ => start_dir.to_path_buf(),
    }
}

/// If `e` is a literal string / single-segment interpolated string / bareword,
/// return its constant text. Used for `require "LITERAL"` / `require Mod::Name`.
/// Returns `None` for any dynamic expression — the analyzer can't follow those.
pub fn static_string_value(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::String(s) => Some(s.clone()),
        ExprKind::Bareword(s) => Some(s.clone()),
        ExprKind::InterpolatedString(parts) => {
            // All parts must be literal text — no variable interpolation.
            let mut out = String::new();
            for part in parts {
                match part {
                    StringPart::Literal(s) => out.push_str(s),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}

pub fn analyze_program(program: &Program, file: &str) -> StrykeResult<()> {
    StaticAnalyzer::new(file).analyze(program)
}

/// Same as [`analyze_program`] but emits strict-vars-style undefined-symbol
/// errors only when the source itself opted into strict (`use strict;`).
/// `stryke check` calls this with `strict_vars = false` so the lint pass is
/// a parse + compile gate, not a strict-vars enforcer.
pub fn analyze_program_with_strict(
    program: &Program,
    file: &str,
    strict_vars: bool,
) -> StrykeResult<()> {
    StaticAnalyzer::with_strict_vars(file, strict_vars).analyze(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_with_file;

    /// Test helper: run the analyzer with `strict_vars=true` so the
    /// undefined-variable detection paths actually fire. The default
    /// `analyze_program` entry point is lenient (parse + compile gate
    /// for `stryke check` — strict-vars-style errors are gated on the
    /// source actually doing `use strict;`); this helper exercises the
    /// strict-on path that the rest of the tests below assume.
    fn lint(code: &str) -> StrykeResult<()> {
        let prog = parse_with_file(code, "test.stk").expect("parse");
        analyze_program_with_strict(&prog, "test.stk", true)
    }

    #[test]
    fn undefined_scalar_detected() {
        let r = lint("p $undefined");
        assert!(r.is_err());
        let e = r.unwrap_err();
        assert_eq!(e.kind, ErrorKind::UndefinedVariable);
        assert!(e.message.contains("$undefined"));
    }

    #[test]
    fn defined_scalar_ok() {
        assert!(lint("my $x = 1; p $x").is_ok());
    }

    #[test]
    fn undefined_sub_detected() {
        let r = lint("nonexistent_function()");
        assert!(r.is_err());
        let e = r.unwrap_err();
        assert_eq!(e.kind, ErrorKind::UndefinedSubroutine);
        assert!(e.message.contains("nonexistent_function"));
    }

    #[test]
    fn defined_sub_ok() {
        assert!(lint("fn foo { 1 } foo()").is_ok());
    }

    #[test]
    fn builtin_sub_ok() {
        assert!(lint("p 'hello'").is_ok());
        assert!(lint("print 'hello'").is_ok());
        assert!(lint("my @x = map { $_ * 2 } 1..3").is_ok());
    }

    #[test]
    fn special_vars_ok() {
        assert!(lint("p $_").is_ok());
        assert!(lint("p @_").is_ok());
        assert!(lint("p $a <=> $b").is_ok());
    }

    /// `my $x = …` / `our $x = …` / `state $x = …` in expression position —
    /// canonical case is `while (my $job = ...) { … $job … }`. Before the
    /// `ExprKind::MyExpr` arm in `analyze_expr`, the declaration was silently
    /// dropped from scope tracking and any body reference tripped
    /// strict-vars. Pin all the conditional-expression contexts where the
    /// pattern is idiomatic so regressions get caught at unit-test time
    /// rather than waiting for the full `examples_strict_lint` sweep.
    #[test]
    fn my_in_while_condition_scopes_var_for_body() {
        assert!(lint("while (my $line = readline(STDIN)) { p $line; }").is_ok());
    }

    #[test]
    fn my_in_if_condition_scopes_var_for_body() {
        assert!(lint("if (my $x = 42) { p $x; }").is_ok());
    }

    #[test]
    fn my_in_until_condition_scopes_var_for_body() {
        assert!(lint("until (my $done = 1) { p $done; last; }").is_ok());
    }

    /// `our` in expression position must register BOTH the bare spelling and
    /// the package-qualified spelling so a sibling fn can reach the global
    /// either way. Mirrors the `StmtKind::Our` branch.
    #[test]
    fn our_in_expression_position_registers_qualified_spelling() {
        assert!(
            lint("package Foo; if (our $cfg = 1) { p $cfg; p $Foo::cfg; }").is_ok(),
            "expression-position `our` should register both `$cfg` and `$Foo::cfg`"
        );
    }

    /// `MyExpr` scoping logic must work for array and hash sigils too, not
    /// just scalar. NOTE: the bytecode compiler currently rejects non-scalar
    /// MyExpr (`compiler.rs:8367` — `Unsupported`), so these scripts won't
    /// run end-to-end yet. The analyzer pin still holds — it asserts the
    /// SCOPING behaviour (decl-in-condition propagates to body) which is
    /// correct regardless of code-gen support, and will stay correct when
    /// the compiler restriction is eventually lifted.
    #[test]
    fn my_in_expression_position_array_and_hash() {
        assert!(lint("if (my @rows = (1, 2, 3)) { p $_ for @rows; }").is_ok());
        assert!(lint("if (my %m = (a => 1)) { p $_ for keys %m; }").is_ok());
    }

    /// Negative pin: the analyzer must STILL flag a genuinely undefined var
    /// even after the MyExpr/sigil-agnostic-special-var changes. Guards
    /// against an over-broad allowlist regressing to "everything passes".
    #[test]
    fn undefined_var_still_flagged_after_relaxations() {
        let r = lint("p $deliberately_never_declared");
        assert!(r.is_err(), "strict-vars must still catch real undefs");
        assert_eq!(r.unwrap_err().kind, ErrorKind::UndefinedVariable);
    }

    /// Sigil-agnostic `is_special_var` delegation to builtins::SPECIAL_VARS must
    /// be a WHITELIST, not a blanket pass. A made-up name that ISN'T in the
    /// canonical registry must still trip strict-vars in contexts the analyzer
    /// inspects. Pin guards against a future "always return true" regression in
    /// the delegation path, which would silently mask typo'd hash / array /
    /// scalar names.
    ///
    /// Scope note: the analyzer currently treats bare assignment (`$x = …`) as
    /// auto-vivification (Perl-style — declares on first write) and doesn't
    /// recurse into every builtin's arguments (`keys %x` doesn't check `%x`).
    /// Those are independent, pre-existing behaviours; this pin uses contexts
    /// the analyzer DOES walk — bare scalar/array/hash references — so the
    /// delegation negative path is exercised cleanly.
    #[test]
    fn sigil_agnostic_special_var_does_not_pass_arbitrary_names() {
        for src in [
            "p $absolutely_not_a_special_var",
            "my %x = %totally_made_up_reflection_hash",
            "my @y = @arr_that_was_never_declared",
        ] {
            let r = lint(src);
            assert!(
                r.is_err(),
                "strict-vars must still flag arbitrary undef vars, accepted: {src}"
            );
            assert_eq!(r.unwrap_err().kind, ErrorKind::UndefinedVariable);
        }
    }

    /// Nested MyExpr — `while (my $a = …) { if (my $b = …) { … $a … $b … } }`
    /// is the natural shape for "read a row, parse a field" pipelines. Both
    /// outer (`$a`) and inner (`$b`) decls must be visible inside the inner
    /// body. Pins the scope-stack arithmetic for the recursive `analyze_block`
    /// calls under the `ExprKind::MyExpr` arm — outer-scope vars from a prior
    /// MyExpr stay reachable from inside a deeper push_scope/pop_scope frame.
    ///
    /// Visibility scope note: a MyExpr decl in `if`/`while` condition position
    /// is declared in the *surrounding* scope (the condition is analyzed before
    /// `push_scope` for the body), so the var remains visible after the block
    /// closes. That's the analyzer's current model — Perl scopes it more
    /// tightly to the block, but tightening here would require condition-only
    /// scope frames. Pin captures the propagation-into-body behaviour that
    /// matters for the strict-vars false-positive fix; the post-block leak
    /// is left as-is for now.
    #[test]
    fn nested_my_in_conditions_both_scopes_propagate() {
        assert!(
            lint(
                "while (my $a = readline(STDIN)) { \
                   if (my $b = length($a)) { p \"$a → $b\"; } \
                 }"
            )
            .is_ok(),
            "nested MyExpr in while+if must scope both decls correctly"
        );
    }

    /// Multi-character reflection hash names (`%all`, `%limits`, `%pc`, the
    /// `%stryke::*` family, `%overload::`) and the `__FILE__` / `__LINE__` /
    /// `__PACKAGE__` / `__SUB__` script-position pseudo-vars must NOT trip
    /// strict-vars. They live in the canonical `SPECIAL_VARS` registry in
    /// builtins.rs and are surfaced via `%v` / `%stryke::special_vars`. This
    /// pin guards against regression of the sigil-agnostic delegation added
    /// to `is_special_var` — without it, `keys %all` and friends fail in the
    /// IDE diagnostic path (always strict) and in any script that opts in
    /// with `use strict`.
    #[test]
    fn reflection_and_meta_special_vars_ok_under_strict() {
        for src in [
            "p scalar keys %all",
            "p scalar keys %b",
            "p scalar keys %limits",
            "p scalar keys %parameters",
            "p scalar keys %pc",
            "p scalar keys %term",
            "p scalar keys %uname",
            "p scalar keys %stryke::all",
            "p scalar keys %stryke::builtins",
            "p scalar keys %stryke::aliases",
            "p scalar keys %stryke::categories",
            "p scalar keys %stryke::descriptions",
            "p scalar keys %stryke::extensions",
            "p scalar keys %stryke::keywords",
            "p scalar keys %stryke::operators",
            "p scalar keys %stryke::perl_compats",
            "p scalar keys %stryke::primaries",
            "p scalar keys %stryke::special_vars",
            "p scalar keys %overload::",
            "p $stryke::VERSION",
            "p __FILE__",
            "p __LINE__",
            "p __PACKAGE__",
            // `main::X` qualified form of every reserved variable.
            // Per the Perl Documentation: "certain built-in identifiers
            // are forced into the main package". The linter must not
            // flag the qualified spelling as undeclared. Mirrors the
            // runtime canonicalization via `strip_main_prefix` in
            // `scope.rs`.
            "p $main::ARGV",
            "p $main::_",
            "p $main::!",
            "p $main::@",
            "p $main::/",
            "p $main::0",
            "p @main::ARGV",
            "p @main::INC",
            "p @main::F",
            "p %main::ENV",
            "p %main::INC",
            "p %main::SIG",
            "p %main::stryke::all",
        ] {
            assert!(
                lint(src).is_ok(),
                "strict-vars false-positive on legitimate stryke special var: {src}"
            );
        }
    }

    #[test]
    fn foreach_var_in_scope() {
        assert!(lint("foreach my $i (1..3) { p $i; }").is_ok());
    }

    #[test]
    fn sub_params_in_scope() {
        assert!(lint("fn foo($x) { p $x; } foo(1)").is_ok());
    }

    #[test]
    fn assignment_declares_var() {
        assert!(lint("$x = 1; p $x").is_ok());
    }

    #[test]
    fn builtin_inc_ok() {
        assert!(lint("my $x = 1; inc($x)").is_ok());
    }

    #[test]
    fn builtin_dec_ok() {
        assert!(lint("my $x = 1; dec($x)").is_ok());
    }

    #[test]
    fn builtin_rev_ok() {
        assert!(lint("my $s = rev 'hello'").is_ok());
    }

    #[test]
    fn builtin_p_alias_for_say_ok() {
        assert!(lint("p 'hello'").is_ok());
    }

    #[test]
    fn builtin_t_thread_ok() {
        assert!(lint("t 1 inc inc").is_ok());
    }

    #[test]
    fn thread_with_undefined_var_detected() {
        let r = lint("t $undefined inc");
        assert!(r.is_err());
    }

    #[test]
    fn try_catch_var_in_scope() {
        assert!(lint("try { die 'err'; } catch ($e) { p $e; }").is_ok());
    }

    #[test]
    fn interpolated_string_undefined_var() {
        // Policy change (post-test_bugs_phase3_pin): bare `$undef`
        // inside `"..."` is a free pass. String interpolation is used
        // as template/description text in tests + log messages, and
        // false positives on `"got $fh ..."` style test descriptions
        // were the dominant noise source. Strict-vars on bare code-
        // context references is unchanged.
        assert!(
            lint(r#"p "hello $undefined""#).is_ok(),
            "undefined scalar inside string-interp must NOT flag",
        );
        // The check still fires in code context.
        assert!(lint(r#"use strict; p $undefined"#).is_err());
    }

    #[test]
    fn interpolated_string_defined_var_ok() {
        assert!(lint(r#"my $x = 1; p "hello $x""#).is_ok());
    }

    #[test]
    fn coderef_params_in_scope() {
        assert!(lint("my $f = fn ($x) { p $x; }; $f->(1)").is_ok());
    }

    #[test]
    fn nested_sub_scope() {
        assert!(lint("fn wrap { my $x = 1; fn inner { p $x; } }").is_ok());
    }

    #[test]
    fn hash_element_access_ok() {
        assert!(lint("my %h = (a => 1); p $h{a}").is_ok());
    }

    #[test]
    fn array_element_access_ok() {
        assert!(lint("my @a = (1, 2, 3); p $a[0]").is_ok());
    }

    #[test]
    fn undefined_hash_detected() {
        let r = lint("p $undefined_hash{key}");
        assert!(r.is_err());
    }

    #[test]
    fn undefined_array_detected() {
        let r = lint("p $undefined_array[0]");
        assert!(r.is_err());
    }

    #[test]
    fn map_with_topic_ok() {
        assert!(lint("my @x = map { $_ * 2 } 1..3").is_ok());
    }

    #[test]
    fn grep_with_topic_ok() {
        assert!(lint("my @x = grep { $_ > 1 } 1..3").is_ok());
    }

    #[test]
    fn sort_with_ab_ok() {
        assert!(lint("my @x = sort { $a <=> $b } 1..3").is_ok());
    }

    #[test]
    fn ternary_undefined_var_detected() {
        let r = lint("my $x = $undefined ? 1 : 0");
        assert!(r.is_err());
    }

    #[test]
    fn binop_undefined_var_detected() {
        let r = lint("my $x = 1 + $undefined");
        assert!(r.is_err());
    }

    #[test]
    fn postfix_if_undefined_detected() {
        let r = lint("p 'x' if $undefined");
        assert!(r.is_err());
    }

    #[test]
    fn while_loop_var_ok() {
        assert!(lint("my $i = 0; while ($i < 10) { p $i; $i++; }").is_ok());
    }

    #[test]
    fn for_loop_init_var_in_scope() {
        assert!(lint("for (my $i = 0; $i < 10; $i++) { p $i; }").is_ok());
    }

    #[test]
    fn given_when_ok() {
        assert!(lint("my $x = 1; given ($x) { when (1) { p 'one'; } }").is_ok());
    }

    #[test]
    fn arrow_deref_ok() {
        assert!(lint("my $h = { a => 1 }; p $h->{a}").is_ok());
    }

    #[test]
    fn method_call_ok() {
        assert!(lint("my $obj = bless {}, 'Foo'; $obj->method()").is_ok());
    }

    #[test]
    fn push_builtin_ok() {
        assert!(lint("my @a; push @a, 1, 2, 3").is_ok());
    }

    #[test]
    fn splice_builtin_ok() {
        assert!(lint("my @a = (1, 2, 3); splice @a, 1, 1, 'x'").is_ok());
    }

    #[test]
    fn substr_builtin_ok() {
        assert!(lint("my $s = 'hello'; p substr($s, 0, 2)").is_ok());
    }

    #[test]
    fn sprintf_builtin_ok() {
        assert!(lint("my $s = sprintf('%d', 42)").is_ok());
    }

    #[test]
    fn range_ok() {
        assert!(lint("my @a = 1..10").is_ok());
    }

    #[test]
    fn qw_ok() {
        assert!(lint("my @a = qw(a b c)").is_ok());
    }

    #[test]
    fn regex_ok() {
        assert!(lint("my $x = 'hello'; $x =~ /ell/").is_ok());
    }

    #[test]
    fn anonymous_sub_captures_outer_var() {
        assert!(lint("my $x = 1; my $f = fn { p $x; }").is_ok());
    }

    #[test]
    fn state_var_ok() {
        assert!(lint("fn Test::counter { state $n = 0; $n++; }").is_ok());
    }

    #[test]
    fn our_var_ok() {
        assert!(lint("our $VERSION = '1.0'").is_ok());
    }

    #[test]
    fn local_var_ok() {
        assert!(lint("local $/ = undef").is_ok());
    }

    #[test]
    fn chained_method_calls_ok() {
        assert!(lint("my $x = Foo->new->bar->baz").is_ok());
    }

    #[test]
    fn list_assignment_ok() {
        assert!(lint("my ($a, $b, $c) = (1, 2, 3); p $a + $b + $c").is_ok());
    }

    #[test]
    fn hash_slice_ok() {
        assert!(lint("my %h = (a => 1, b => 2); my @v = @h{qw(a b)}").is_ok());
    }

    #[test]
    fn array_slice_ok() {
        assert!(lint("my @a = (1, 2, 3, 4); my @b = @a[0, 2]").is_ok());
    }

    #[test]
    fn instance_method_on_typed_var_flags_unknown_method() {
        // `my $p = Point(...)` binds `$p` to Point. `$p->dgdd()` is an
        // unknown method/field on Point and must be flagged the same
        // way `Point->new(dgdd => ...)` and `$self->dgdd` already are.
        let r = lint(
            "class Point { x : Float\n y : Float\n fn mag_sq { 1 } }\n\
             my $p = Point(x => 3, y => 4)\n\
             $p->dgdd()",
        );
        assert!(r.is_err(), "expected error for `$$p->dgdd`");
        let e = r.unwrap_err();
        assert_eq!(e.kind, ErrorKind::UndefinedSubroutine);
        assert!(
            e.message.contains("dgdd") && e.message.contains("Point"),
            "expected message to name `dgdd` and `Point`, got: {}",
            e.message,
        );
    }

    #[test]
    fn instance_method_on_typed_var_known_method_ok() {
        // Same setup, but `$p->mag_sq` IS a declared method — must pass.
        assert!(lint(
            "class Point { x : Float\n y : Float\n fn mag_sq { 1 } }\n\
                 my $p = Point(x => 3, y => 4)\n\
                 $p->mag_sq()"
        )
        .is_ok());
    }

    #[test]
    fn instance_method_on_typed_var_field_ok() {
        // Field access via `->`: `$p->x` reads field `x` on Point.
        assert!(lint(
            "class Point { x : Float\n y : Float }\n\
                 my $p = Point(x => 3, y => 4)\n\
                 p $p->x"
        )
        .is_ok());
    }

    #[test]
    fn strict_never_flags_topic_variants() {
        // Stryke topic / block-param family — strict-vars must never
        // flag ANY of these as undefined, regardless of sigil or shape.
        let cases = [
            // Bare topic + positional.
            "_", "_0", "_1", "_42", // Outer-chain.
            "_<", "_<<<<<", // Indexed-ascent.
            "_<3", "_<10", // Positional + outer chain combined.
            "_2<", "_2<<<", "_2<5",
        ];
        for name in cases {
            assert!(
                super::is_topic_var(name),
                "is_topic_var({name:?}) must return true (topic/block-param form)",
            );
        }
        // Run the analyzer on each form (sigiled + bare contexts) to
        // ensure no UndefinedVariable error fires.
        let src = "use strict\np _ + _1 + _< + _<2 + _<<<<< + _2<<< + _2<5\n";
        let prog = crate::parse_with_file(src, "test.stk").expect("parse");
        super::analyze_program_with_strict(&prog, "test.stk", true)
            .expect("strict-vars must not flag topic-variant block params");
    }

    #[test]
    fn is_topic_var_rejects_non_topic_underscore_names() {
        // Anti-cases: names that START with `_` but aren't topic vars.
        // The grammar is strict — only `_`, `_N`, `_<...`, `_<N`, `_N<<<`,
        // `_N<M` patterns qualify. Anything with a letter after the
        // optional digit/chevron run is NOT a topic var.
        for bad in [
            "_x",     // underscore + letter
            "_foo",   // underscore + word
            "_3abc",  // digits then letters
            "_<bad",  // chevron then letters
            "_2<xyz", // positional + chevron + letters
            "x_",     // doesn't start with underscore
            "__",     // double underscore — not a topic form
            "_<<<x",  // chevrons then letters
        ] {
            assert!(
                !super::is_topic_var(bad),
                "is_topic_var({bad:?}) must return false (not a topic form)",
            );
        }
    }

    #[test]
    fn universal_methods_skip_hierarchy_lookup() {
        // `isa` / `can` / `DOES` / `new` / `BUILD` / `DESTROY` short-
        // circuit the BFS — they work on every class regardless of
        // declared method set.
        let mut a = super::StaticAnalyzer::new("test.stk");
        // Empty class with no methods.
        a.type_methods.insert("Empty".to_string(), HashSet::new());
        a.type_fields.insert("Empty".to_string(), HashSet::new());
        for method in ["isa", "can", "DOES", "does", "new", "BUILD", "DESTROY"] {
            assert!(
                a.method_resolves_in_hierarchy("Empty", method),
                "method `{method}` must resolve on any class via the universal whitelist",
            );
        }
        // A made-up name does NOT short-circuit.
        assert!(
            !a.method_resolves_in_hierarchy("Empty", "totally_made_up"),
            "non-universal unknown method must still be flagged",
        );
    }

    #[test]
    fn method_resolves_walks_extends_chain() {
        // Dog extends Animal; Animal has `trail`. `$self->trail` on
        // Dog must resolve via the parent.
        let mut a = super::StaticAnalyzer::new("test.stk");
        let mut animal_methods = HashSet::new();
        animal_methods.insert("trail".to_string());
        a.type_methods.insert("Animal".to_string(), animal_methods);
        a.type_fields.insert("Animal".to_string(), HashSet::new());
        a.type_methods.insert("Dog".to_string(), HashSet::new());
        a.type_fields.insert("Dog".to_string(), HashSet::new());
        a.type_parents
            .insert("Dog".to_string(), vec!["Animal".to_string()]);
        assert!(
            a.method_resolves_in_hierarchy("Dog", "trail"),
            "`trail` on Dog must resolve via Animal in extends chain",
        );
        // Method on neither class — still fails.
        assert!(
            !a.method_resolves_in_hierarchy("Dog", "fly"),
            "`fly` on Dog must NOT resolve (absent from Dog AND Animal)",
        );
    }

    #[test]
    fn method_resolves_cycle_protected() {
        // Pathological `A extends B; B extends A` mutual recursion
        // must not infinite-loop.
        let mut a = super::StaticAnalyzer::new("test.stk");
        a.type_methods.insert("A".to_string(), HashSet::new());
        a.type_fields.insert("A".to_string(), HashSet::new());
        a.type_methods.insert("B".to_string(), HashSet::new());
        a.type_fields.insert("B".to_string(), HashSet::new());
        a.type_parents
            .insert("A".to_string(), vec!["B".to_string()]);
        a.type_parents
            .insert("B".to_string(), vec!["A".to_string()]);
        // Should return false (method nowhere) without hanging.
        assert!(!a.method_resolves_in_hierarchy("A", "missing"));
        assert!(!a.method_resolves_in_hierarchy("B", "missing"));
    }

    #[test]
    fn dollar_obj_method_in_string_interp_not_flagged() {
        // `"#{ $obj->method }"` inside a string — the interpolation
        // is real code, but the method-call shape mustn't false-
        // positive when the method name happens to look like a typo
        // (since strict-vars-on-by-default skips simple sigil-vars
        // in InterpolatedString but DOES walk `#{ EXPR }`).
        assert!(lint(
            "class P { x: Int = 0\n fn show { $self->x } }\n\
                 my $p = P(x => 5)\np \"got #{ $p->show }\""
        )
        .is_ok(),);
    }

    #[test]
    fn dollar_hash_with_complex_expression_in_string_interp() {
        // `"#{ $hash->{key} + 1 }"` — full expression inside #{...}.
        // The complex-expression branch DOES walk strict (per the
        // current policy), so undefined refs inside still flag.
        assert!(
            lint(
                "use strict\n\
                 my %h = (n => 5)\n\
                 p \"got #{ $h{n} + 1 }\""
            )
            .is_ok(),
            "defined hash element inside #{{}} must not flag",
        );
        let r = lint("use strict\np \"got #{ $undef_typo + 1 }\"");
        assert!(
            r.is_err(),
            "complex-expr #{{}} block must still strict-check unknown vars",
        );
    }

    #[test]
    fn is_topic_var_accepts_extra_grammar_variants() {
        // Edge cases of the grammar not covered by the main test:
        for good in [
            "_99",         // many positional digits
            "_<<<<<<<<<<", // very deep outer chain (10 chevrons)
            "_<999",       // multi-digit indexed ascent
            "_42<<<",      // 2-digit positional + chevrons
            "_42<42",      // 2-digit positional + 2-digit index
        ] {
            assert!(
                super::is_topic_var(good),
                "is_topic_var({good:?}) must return true",
            );
        }
    }

    #[test]
    fn strict_never_flags_sigiled_topic_variants() {
        // Same set but with `$` sigil prefix — `is_topic_var` is called
        // with the bare name (sigil already stripped by the AST), so
        // the bare-name check is what matters.
        let src = "use strict\nmy $tot = $_ + $_0 + $_1 + $_< + $_<2 + $_<<<<< + $_2<<< + $_2<5\np $tot\n";
        let prog = crate::parse_with_file(src, "test.stk").expect("parse");
        super::analyze_program_with_strict(&prog, "test.stk", true)
            .expect("strict-vars must not flag sigiled topic-variant block params");
    }

    #[test]
    fn strict_still_flags_undefined_underscore_prefixed_ident() {
        // Guardrail: an identifier that merely *starts* with `_` (like
        // `$_underscore_name`) is NOT a topic var and SHOULD be flagged.
        let r = lint("p $_underscore_name");
        assert!(r.is_err(), "expected $_underscore_name to be flagged");
    }

    #[test]
    fn qualified_our_scalar_visible_across_packages() {
        // `package Foo; our $x = 1; package main; p $Foo::x` — strict-
        // vars must accept `$Foo::x` from main. Regression for the
        // false-positive on `oursync $val` in `package Counter`.
        assert!(
            lint("package Foo\nour $x = 1\npackage main\np $Foo::x").is_ok(),
            "qualified `$Foo::x` must resolve to `our $x` declared in package Foo",
        );
    }

    #[test]
    fn qualified_oursync_scalar_visible_across_packages() {
        // The exact `oursync` form from examples/test_namespaces_pin.stk.
        assert!(
            lint("package Counter\noursync $val = 0\npackage main\np $Counter::val").is_ok(),
            "qualified `$Counter::val` must resolve to `oursync $val` in package Counter",
        );
    }

    #[test]
    fn qualified_our_array_visible_across_packages() {
        assert!(lint("package Foo\nour @xs = (1,2,3)\npackage main\np @Foo::xs").is_ok());
    }

    #[test]
    fn qualified_our_hash_visible_across_packages() {
        assert!(lint("package Foo\nour %h = (a=>1)\npackage main\np %Foo::h").is_ok());
    }

    #[test]
    fn thread_par_run_compiler_generated_call_not_flagged() {
        // `~p>` desugars to `_thread_par_run(...)` — the linter must
        // not flag this synthetic name as an undefined sub.
        assert!(
            lint("my @xs = (1,2,3)\nmy @r = ~p> @xs map { _ * 2 }").is_ok(),
            "compiler-generated _thread_par_run must be whitelisted",
        );
    }

    #[test]
    fn deque_constructor_not_flagged() {
        // `deque(...)` is a parser-level constructor handled by
        // compiler.rs / vm_helper.rs but not registered in `%all`,
        // so it needs an explicit whitelist entry in is_sub_defined.
        assert!(
            lint("my $dq = deque(1, 2, 3)\np $dq->len").is_ok(),
            "deque constructor must not be flagged as undefined sub",
        );
    }

    #[test]
    fn defer_block_compiler_generated_call_not_flagged() {
        // `defer { BLOCK }` desugars to `defer__internal(fn { BLOCK })`.
        // Same whitelist rule as `_thread_par_run`.
        assert!(
            lint("fn ff { defer { p \"cleanup\" }; 42 }").is_ok(),
            "compiler-generated defer__internal must be whitelisted",
        );
        // Negative guard: an actual unknown `_foo` is still flagged.
        let r = lint("use strict; _unknown_helper()");
        assert!(r.is_err(), "arbitrary `_foo` must still flag");
    }

    #[test]
    fn aop_intercept_context_vars_not_flagged() {
        // `before/after/around` advice bodies see `$INTERCEPT_NAME`,
        // `@INTERCEPT_ARGS`, `$INTERCEPT_RESULT`, `$INTERCEPT_MS`,
        // `$INTERCEPT_US` as VM-injected always-defined vars.
        assert!(lint("before \"fetch\" { p $INTERCEPT_NAME, @INTERCEPT_ARGS }").is_ok());
        assert!(
            lint("after \"fetch\" { p $INTERCEPT_RESULT, $INTERCEPT_MS, $INTERCEPT_US }").is_ok()
        );
    }

    #[test]
    fn string_interpolation_never_flags_undefined_simple_var() {
        // Bare `$undef` / `@undef` / `%undef` interpolated inside
        // `"..."` is a free pass — strings are used as test
        // descriptions / log messages with template placeholders that
        // aren't intended as hard variable refs. Bare code-context
        // references (`p $undef`) still get flagged.
        assert!(
            lint("p \"printf $fh writes to STDOUT\"").is_ok(),
            "simple $var inside string-interp must not be flagged",
        );
        assert!(
            lint("p \"got @items here\"").is_ok(),
            "simple @var inside string-interp must not be flagged",
        );
        // $#arr-inside-string also gets the free pass.
        assert!(
            lint("p \"got $#missing_array items\"").is_ok(),
            "$#arr-style inside string-interp must not be flagged",
        );
        // Negative guard: outside strings, the strict-vars check still
        // fires.
        assert!(
            lint("use strict\np $fh").is_err(),
            "bare $fh outside string must still flag",
        );
    }

    #[test]
    fn string_interp_complex_expr_still_walks_strict() {
        // `"#{ $undef + 1 }"` — the `#{EXPR}` block is real code, so
        // bare `$undef` reference inside the expression should flag.
        let r = lint("use strict\np \"got #{ $undef + 1 }\"");
        assert!(
            r.is_err(),
            "complex expr inside #{{}} must still strict-check vars",
        );
    }

    #[test]
    fn qualified_main_var_visible_in_default_package() {
        // `our $x = ...` in default package `main` — `$main::x` must
        // resolve. Regression for test_bugs_phase3_pin.stk where
        // `BEGIN { $main::log_begin .= ... }` was flagged.
        assert!(lint("our $log_begin = \"\"\nBEGIN { $main::log_begin .= \"B:\" }").is_ok(),);
        assert!(lint("our @items = (1,2)\np @main::items").is_ok());
        assert!(lint("our %map = ()\np keys %main::map").is_ok());
    }

    #[test]
    fn dollar_hash_array_last_index_uses_underlying_array() {
        // `$#name` is the last-index-of-@name shortcut. Strict-vars
        // must check @name (the array), not $#name as a scalar.
        assert!(lint("my @arr = (1,2,3); p $#arr").is_ok());
        let r = lint("use strict\np $#undefined_array");
        assert!(r.is_err(), "$#undefined_array must flag @undefined_array");
        assert!(
            r.unwrap_err().message.contains("@undefined_array"),
            "error must name @undefined_array, not $#undefined_array",
        );
    }

    #[test]
    fn match_arm_enum_variant_typo_flagged() {
        // Auto-quoted enum patterns: `Sig::Term2 => "..."` arrives
        // as MatchPattern::Value(String("Sig::Term2")). When Sig is
        // a known enum and Term2 isn't a variant, must flag.
        let r = lint(
            "enum Sig { Hup, Int, Term, Kill }\n\
             fn handle($s) {\n\
                 match ($s) {\n\
                     Sig::Hup => \"reload\",\n\
                     Sig::Term2 => \"drain\",\n\
                     Sig::Kill => \"reap\",\n\
                 }\n\
             }",
        );
        assert!(r.is_err(), "expected flag on Sig::Term2");
        let msg = r.unwrap_err().message;
        assert!(
            msg.contains("Term2") && msg.contains("Sig"),
            "message must name Term2 and Sig: {msg}",
        );
    }

    #[test]
    fn match_arm_known_enum_variant_passes() {
        // Symmetric guard: real variants must not be flagged.
        assert!(lint(
            "enum Sig { Hup, Int, Term, Kill }\n\
                 fn handle($s) {\n\
                     match ($s) {\n\
                         Sig::Hup => \"reload\",\n\
                         Sig::Int => \"shutdown\",\n\
                         Sig::Term => \"drain\",\n\
                         Sig::Kill => \"reap\",\n\
                     }\n\
                 }"
        )
        .is_ok());
    }

    #[test]
    fn dollar_caret_perl_special_vars_not_flagged() {
        // `$^X`, `$^O`, `$^V`, `$^W`, `$^T` etc. — Perl special vars
        // prefixed with `^`. All must be treated as always-defined.
        for name in ["$^X", "$^O", "$^V", "$^W", "$^T", "$^R", "$^N", "$^H"] {
            assert!(
                lint(&format!("use strict\np {name}")).is_ok(),
                "{name} must not be flagged by strict-vars",
            );
        }
    }

    #[test]
    fn lexical_filehandle_open_my_var_not_flagged() {
        // `open(my $fh, ">", $path)` — `my $fh` inside the call
        // declares a lexical scalar. Later `print $fh ...` /
        // `close $fh` must see it as defined.
        assert!(lint(
            "use strict\nmy $efile = \"/tmp/x\"\n\
                 open(my $wfh, \">\", $efile) or die\n\
                 print $wfh \"line1\\n\"\nclose $wfh"
        )
        .is_ok(),);
    }

    #[test]
    fn exists_subroutine_ref_does_not_flag_undefined() {
        // `exists &Pkg::sub` is introspection — flagging the sub as
        // undefined defeats the entire purpose.
        assert!(lint(
            "package Foo\nfn greet = 1\npackage main\n\
                 p exists(&Foo::greet) ? \"y\" : \"n\"\n\
                 p exists(&Foo::missing) ? \"y\" : \"n\""
        )
        .is_ok(),);
    }

    #[test]
    fn universal_methods_resolve_on_any_class() {
        // `isa`, `can`, `DOES`, `does`, `VERSION`, lifecycle hooks
        // (`new`, `BUILD`, `DESTROY`, `destroy`), and built-in class
        // methods (`clone`, `with`, `to_hash`, `to_hash_rec`,
        // `to_hash_deep`, `fields`, `methods`, `superclass`) are
        // always callable on any class instance — never flag them
        // via $obj->X or $self->X.
        assert!(lint(
            "class Square { side: Float\n fn area { 1 } }\n\
                 my $sq = Square->new(side => 5)\n\
                 p $sq->isa(\"Square\")\n\
                 p $sq->can(\"area\")\n\
                 p $sq->DOES(\"Shape\")\n\
                 my $cloned = $sq->clone()\n\
                 my $changed = $sq->with(side => 9)\n\
                 p $sq->to_hash()\n\
                 p $sq->fields()"
        )
        .is_ok(),);
    }

    #[test]
    fn builtin_struct_methods_resolve_on_any_struct() {
        // Same whitelist applies to structs: `clone`, `with`,
        // `to_hash`, `to_hash_rec`, `to_hash_deep`, `fields`.
        assert!(lint(
            "struct Point { x: Float\n y: Float }\n\
                 my $p = Point(x => 1.0, y => 2.0)\n\
                 my $c = $p->clone()\n\
                 my $u = $p->with(x => 9.0)\n\
                 p $p->to_hash()\n\
                 p $p->fields()"
        )
        .is_ok(),);
    }

    #[test]
    fn class_inheritance_resolves_parent_methods_on_self() {
        // `class Dog extends Animal` — `$self->trail` inside Dog's
        // body must walk up to Animal and find `trail` there.
        assert!(lint(
            "class Animal { name: Str = \"\"\n fn trail { \"...\" } }\n\
                 class Dog extends Animal {\n\
                     breed: Str = \"\"\n\
                     fn show { $self->trail }\n\
                 }"
        )
        .is_ok(),);
    }

    #[test]
    fn class_inheritance_resolves_parent_fields_in_constructor() {
        // `Dog(name => "Rex", breed => "Lab")` — `name` is on Animal,
        // `breed` on Dog. Constructor key check must accept both.
        assert!(lint(
            "class Animal { name: Str = \"\" }\n\
                 class Dog extends Animal { breed: Str = \"\" }\n\
                 my $d = Dog(name => \"Rex\", breed => \"Lab\")\n\
                 p $d->name"
        )
        .is_ok(),);
    }

    #[test]
    fn resolve_require_path_finds_lib_root_from_nested_source() {
        // Project layout: tmp_root/lib/ai/{matrix,neural_network}.stk.
        // From neural_network.stk, `require "./lib/ai/matrix.stk"`
        // must resolve to tmp_root/lib/ai/matrix.stk even though the
        // current file sits inside `lib/ai/`. Without walking up to
        // find the `lib/`-bearing ancestor, the resolver would land
        // in `lib/ai/lib/ai/matrix.stk` which doesn't exist.
        let tmp = std::env::temp_dir().join(format!("stryke_resolve_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("lib").join("ai")).unwrap();
        let mat = tmp.join("lib").join("ai").join("matrix.stk");
        std::fs::write(&mat, "1\n").unwrap();
        let nn = tmp.join("lib").join("ai").join("neural_network.stk");
        std::fs::write(&nn, "1\n").unwrap();
        let resolved =
            super::resolve_require_path_from_file(nn.to_str().unwrap(), "./lib/ai/matrix.stk");
        assert!(
            resolved.as_ref().is_some_and(|p| p == &mat)
                || resolved
                    .as_ref()
                    .is_some_and(|p| { p.canonicalize().ok() == mat.canonicalize().ok() }),
            "expected to resolve to {mat:?}, got {resolved:?}",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_require_path_sibling_in_same_dir() {
        // `require "./sibling.stk"` from same-directory script should
        // resolve regardless of `lib/` presence.
        let tmp =
            std::env::temp_dir().join(format!("stryke_resolve_sibling_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let sib = tmp.join("sibling.stk");
        std::fs::write(&sib, "1\n").unwrap();
        let me = tmp.join("me.stk");
        std::fs::write(&me, "1\n").unwrap();
        let resolved = super::resolve_require_path_from_file(me.to_str().unwrap(), "./sibling.stk");
        assert!(
            resolved.is_some(),
            "expected to resolve ./sibling.stk in same dir, got None",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn positional_constructor_args_not_checked_as_keys() {
        // `Task(1, "Setup", Priority::High)` is positional — none of
        // the args are field names. Must not flag the String /
        // Bareword args as "unknown field".
        assert!(lint(
            "enum Priority { Low, Medium, High, Critical }\n\
                 class Task {\n\
                     id: Int\n\
                     title: Str = \"\"\n\
                     priority: Any = undef\n\
                 }\n\
                 my $t = Task(1, \"Setup\", Priority::High)"
        )
        .is_ok(),);
    }

    #[test]
    fn positional_constructor_with_string_value_not_flagged() {
        // `Person(\"Alice\", 30)` — String literal is the FIRST arg
        // (and would-be field name), but since "Alice" isn't a
        // declared field of Person, the call is positional.
        assert!(lint(
            "class Person { name: Str = \"\"\n age: Int = 0 }\n\
                 my $p = Person(\"Alice\", 30)"
        )
        .is_ok(),);
    }

    #[test]
    fn keyed_constructor_still_flags_typo() {
        // Guardrail: `Point(x => 10, yyg => 20)` — `yyg` IS a typo.
        // First arg matches a declared field (`x`), so the heuristic
        // correctly classifies the call as keyed and the check fires.
        let r = lint(
            "class Point { x: Float\n y: Float }\n\
             my $p = Point(x => 10, yyg => 20)",
        );
        assert!(r.is_err(), "expected `yyg` typo to be flagged");
        assert!(
            r.unwrap_err().message.contains("yyg"),
            "error must name `yyg` field",
        );
    }

    #[test]
    fn dollar_dollar_pid_not_flagged() {
        // `$$` is the process ID — always-defined special var.
        // Parser stores it as `ScalarVar("$$")` so the strict-vars
        // check sees a 2-char name; is_special_var must whitelist.
        assert!(lint("use strict\np $$").is_ok());
        assert!(lint("use strict\n$$ > 0 ? 1 : 0").is_ok());
    }

    #[test]
    fn class_impl_trait_resolves_default_method() {
        // `trait Greetable { fn greeting { "Hello" } }` + `class Person
        // impl Greetable { ... }` — `$p->greeting` must resolve via
        // the trait's default impl. Regression for
        // test_extended_features_pin.stk.
        assert!(lint(
            "trait Greetable {\n\
                     fn greeting { \"Hello\" }\n\
                     fn name\n\
                 }\n\
                 class Person impl Greetable {\n\
                     n: Str = \"\"\n\
                     fn name { $self->n }\n\
                 }\n\
                 my $p = Person(n => \"Alice\")\n\
                 p $p->greeting()"
        )
        .is_ok(),);
    }

    #[test]
    fn class_impl_multiple_traits_resolves_methods_from_all() {
        assert!(lint(
            "trait Greetable { fn greeting { \"hi\" } }\n\
                 trait Loggable  { fn log_it { 1 } }\n\
                 class Hybrid impl Greetable, Loggable {}\n\
                 my $h = Hybrid->new\n\
                 p $h->greeting()\n\
                 p $h->log_it()"
        )
        .is_ok(),);
    }

    #[test]
    fn class_impl_trait_still_flags_unknown_method() {
        // Guardrail: if the method exists on NEITHER the class NOR
        // any implemented trait, still flag.
        let r = lint(
            "trait Greetable { fn greeting }\n\
             class Person impl Greetable { n: Str = \"\" }\n\
             my $p = Person->new\n\
             p $p->fly()",
        );
        assert!(r.is_err(), "expected $p->fly to be flagged");
    }

    #[test]
    fn class_inheritance_still_flags_unknown_method() {
        // Symmetric guard: a method that exists on NEITHER child nor
        // parent must still be flagged.
        let r = lint(
            "class Animal { fn trail { \"\" } }\n\
             class Dog extends Animal {\n\
                 fn show { $self->fly }\n\
             }",
        );
        assert!(r.is_err(), "expected $self->fly to be flagged");
    }

    #[test]
    fn instance_method_on_arrow_new_form_typed_var_flags() {
        // `my $p = Point->new(x => 3)` also binds `$p` to Point.
        let r = lint(
            "class Point { x : Float\n y : Float }\n\
             my $p = Point->new(x => 3, y => 4)\n\
             $p->whatever()",
        );
        assert!(r.is_err(), "expected error for `$$p->whatever`");
    }
}
