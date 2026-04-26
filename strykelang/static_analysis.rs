//! Static analysis pass for detecting undefined variables and subroutines.

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::ast::{
    Block, DerefKind, Expr, ExprKind, MatchArrayElem, Program, Sigil, Statement, StmtKind,
    StringPart, SubSigParam,
};
use crate::error::{ErrorKind, PerlError, PerlResult};

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
    errors: Vec<PerlError>,
    file: String,
    current_package: String,
}

impl StaticAnalyzer {
    pub fn new(file: &str) -> Self {
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

    fn is_scalar_defined(&self, name: &str) -> bool {
        if is_special_var(name) {
            return true;
        }
        self.scopes.iter().rev().any(|s| s.scalars.contains(name))
    }

    fn is_array_defined(&self, name: &str) -> bool {
        if name == "_" || name == "ARGV" {
            return true;
        }
        self.scopes.iter().rev().any(|s| s.arrays.contains(name))
    }

    fn is_hash_defined(&self, name: &str) -> bool {
        if matches!(name, "ENV" | "SIG" | "INC") {
            return true;
        }
        self.scopes.iter().rev().any(|s| s.hashes.contains(name))
    }

    fn is_sub_defined(&self, name: &str) -> bool {
        // Late static binding: static::method() is always valid (runtime-resolved)
        if name.starts_with("static::") {
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
            .push(PerlError::new(kind, msg, line, &self.file));
    }

    pub fn analyze(mut self, program: &Program) -> PerlResult<()> {
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
            StmtKind::Use { module, .. } => {
                self.declare_sub(module);
            }
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
                // Register class name as a callable (constructor)
                self.declare_sub(&def.name);
                // Register static methods and static fields as Class::name
                for m in &def.methods {
                    if m.is_static {
                        self.declare_sub(&format!("{}::{}", def.name, m.name));
                    }
                }
                for sf in &def.static_fields {
                    self.declare_sub(&format!("{}::{}", def.name, sf.name));
                }
            }
            StmtKind::StructDecl { def } => {
                self.declare_sub(&def.name);
            }
            StmtKind::EnumDecl { def } => {
                self.declare_sub(&def.name);
                for v in &def.variants {
                    self.declare_sub(&format!("{}::{}", def.name, v.name));
                }
            }
            _ => {}
        }
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
            | StmtKind::MySync(decls) => {
                for d in decls {
                    match d.sigil {
                        Sigil::Scalar => self.declare_scalar(&d.name),
                        Sigil::Array => self.declare_array(&d.name),
                        Sigil::Hash => self.declare_hash(&d.name),
                        Sigil::Typeglob => {}
                    }
                    if let Some(init) = &d.initializer {
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
            StmtKind::StructDecl { .. }
            | StmtKind::EnumDecl { .. }
            | StmtKind::ClassDecl { .. }
            | StmtKind::TraitDecl { .. }
            | StmtKind::FormatDecl { .. }
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

    fn analyze_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::ScalarVar(name) if !self.is_scalar_defined(name) => {
                self.error(
                    ErrorKind::UndefinedVariable,
                    format!("Global symbol \"${}\" requires explicit package name", name),
                    expr.line,
                );
            }
            ExprKind::ArrayVar(name) if !self.is_array_defined(name) => {
                self.error(
                    ErrorKind::UndefinedVariable,
                    format!("Global symbol \"@{}\" requires explicit package name", name),
                    expr.line,
                );
            }
            ExprKind::HashVar(name) if !self.is_hash_defined(name) => {
                self.error(
                    ErrorKind::UndefinedVariable,
                    format!("Global symbol \"%{}\" requires explicit package name", name),
                    expr.line,
                );
            }
            ExprKind::ArrayElement { array, index } => {
                if !self.is_array_defined(array) && !self.is_scalar_defined(array) {
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
                if !self.is_hash_defined(hash) && !self.is_scalar_defined(hash) {
                    self.error(
                        ErrorKind::UndefinedVariable,
                        format!("Global symbol \"%{}\" requires explicit package name", hash),
                        expr.line,
                    );
                }
                self.analyze_expr(key);
            }
            ExprKind::ArraySlice { array, indices } => {
                if !self.is_array_defined(array) {
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
                if !self.is_hash_defined(hash) {
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
                for a in args {
                    self.analyze_expr(a);
                }
            }
            ExprKind::MethodCall { object, args, .. } => {
                self.analyze_expr(object);
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
            | ExprKind::Exists(e)
            | ExprKind::Delete(e) => {
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
            ExprKind::InterpolatedString(parts) => {
                for part in parts {
                    match part {
                        StringPart::ScalarVar(name) => {
                            if !self.is_scalar_defined(name) {
                                self.error(
                                    ErrorKind::UndefinedVariable,
                                    format!(
                                        "Global symbol \"${}\" requires explicit package name",
                                        name
                                    ),
                                    expr.line,
                                );
                            }
                        }
                        StringPart::ArrayVar(name) => {
                            if !self.is_array_defined(name) {
                                self.error(
                                    ErrorKind::UndefinedVariable,
                                    format!(
                                        "Global symbol \"@{}\" requires explicit package name",
                                        name
                                    ),
                                    expr.line,
                                );
                            }
                        }
                        StringPart::Expr(e) => self.analyze_expr(e),
                        StringPart::Literal(_) => {}
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
                self.analyze_expr(handle);
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
            _ => {}
        }
    }
}

fn is_special_var(name: &str) -> bool {
    if name.len() == 1 {
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
    )
}

pub fn analyze_program(program: &Program, file: &str) -> PerlResult<()> {
    StaticAnalyzer::new(file).analyze(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_with_file;

    fn lint(code: &str) -> PerlResult<()> {
        let prog = parse_with_file(code, "test.stk").expect("parse");
        analyze_program(&prog, "test.stk")
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
        let r = lint(r#"p "hello $undefined""#);
        assert!(r.is_err());
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
}
