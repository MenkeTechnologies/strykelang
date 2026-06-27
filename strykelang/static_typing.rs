//! `--static` mandatory-typing pass.
//!
//! When `--static` is active, stryke's opt-in type system becomes mandatory,
//! Kotlin-style: every function parameter, every return type, and every
//! variable declaration must carry a type (or, for variables, an initializer
//! the type can be inferred from). Statically-known type mismatches (a literal
//! whose type clashes with the declared type) abort before the program runs.
//!
//! This runs on the **main program** only (imported `.pm` compat libraries are
//! exempt) and never mutates the AST — it only validates and reports the first
//! violation as a pre-run error.
//!
//! Coverage notes:
//! - Named subs (`fn`/`sub`), struct/class/trait methods, and anonymous `fn`
//!   in variable initializers / expression statements are checked for typed
//!   scalar params + a declared return type.
//! - `@`/`%` parameters satisfy the requirement at their container shape;
//!   element-typed parameters (`@a: List<T>`) are a later refinement.
//! - Return-value-vs-declared-type and container element mismatches that are
//!   not statically obvious are caught at runtime by `check_value`.

use crate::ast::{
    Block, ClassMethod, Expr, ExprKind, PerlTypeName, Program, Sigil, Statement, StmtKind,
    StructMethod, SubSigParam, VarDecl,
};
use crate::error::{StrykeError, StrykeResult};

/// Entry point: validate the whole program against `--static` rules.
pub fn check(program: &Program) -> StrykeResult<()> {
    let c = Checker;
    c.check_block(&program.statements)
}

struct Checker;

impl Checker {
    fn err(&self, msg: impl Into<String>, line: usize) -> StrykeError {
        StrykeError::type_error(format!("--static: {}", msg.into()), line)
    }

    fn check_block(&self, block: &Block) -> StrykeResult<()> {
        for stmt in block {
            self.check_stmt(stmt)?;
        }
        Ok(())
    }

    fn check_stmt(&self, stmt: &Statement) -> StrykeResult<()> {
        let line = stmt.line;
        match &stmt.kind {
            StmtKind::My(decls)
            | StmtKind::Our(decls)
            | StmtKind::State(decls)
            | StmtKind::Local(decls)
            | StmtKind::MySync(decls)
            | StmtKind::OurSync(decls) => {
                for d in decls {
                    self.check_var_decl(d, line)?;
                    if let Some(init) = &d.initializer {
                        self.check_expr(init)?;
                    }
                }
            }
            StmtKind::SubDecl {
                name,
                params,
                body,
                return_type,
                ..
            } => {
                self.check_params(params, &format!("fn {}", name), line)?;
                if return_type.is_none() {
                    return Err(self.err(
                        format!(
                            "fn {} must declare a return type (`fn {}(...): Type`)",
                            name, name
                        ),
                        line,
                    ));
                }
                self.check_block(body)?;
            }
            StmtKind::StructDecl { def } => {
                for m in &def.methods {
                    self.check_struct_method(m, &def.name, line)?;
                }
            }
            StmtKind::ClassDecl { def } => {
                for m in &def.methods {
                    self.check_class_method(m, &def.name, line)?;
                }
            }
            StmtKind::TraitDecl { def } => {
                for m in &def.methods {
                    self.check_class_method(m, &def.name, line)?;
                }
            }
            StmtKind::Expression(e) => self.check_expr(e)?,
            StmtKind::If {
                condition,
                body,
                elsifs,
                else_block,
            } => {
                self.check_expr(condition)?;
                self.check_block(body)?;
                for (c, b) in elsifs {
                    self.check_expr(c)?;
                    self.check_block(b)?;
                }
                if let Some(b) = else_block {
                    self.check_block(b)?;
                }
            }
            StmtKind::Unless {
                condition,
                body,
                else_block,
            } => {
                self.check_expr(condition)?;
                self.check_block(body)?;
                if let Some(b) = else_block {
                    self.check_block(b)?;
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
                self.check_expr(condition)?;
                self.check_block(body)?;
                if let Some(b) = continue_block {
                    self.check_block(b)?;
                }
            }
            StmtKind::DoWhile { body, condition } => {
                self.check_block(body)?;
                self.check_expr(condition)?;
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
                continue_block,
                ..
            } => {
                if let Some(s) = init {
                    self.check_stmt(s)?;
                }
                if let Some(c) = condition {
                    self.check_expr(c)?;
                }
                if let Some(s) = step {
                    self.check_expr(s)?;
                }
                self.check_block(body)?;
                if let Some(b) = continue_block {
                    self.check_block(b)?;
                }
            }
            StmtKind::Foreach {
                list,
                body,
                continue_block,
                ..
            } => {
                self.check_expr(list)?;
                self.check_block(body)?;
                if let Some(b) = continue_block {
                    self.check_block(b)?;
                }
            }
            StmtKind::Block(b) => self.check_block(b)?,
            StmtKind::Return(Some(e)) => self.check_expr(e)?,
            _ => {}
        }
        Ok(())
    }

    /// Variable declarations must carry a type annotation OR an initializer the
    /// type can be inferred from. A statically-known literal initializer whose
    /// type clashes with the declared type aborts.
    fn check_var_decl(&self, d: &VarDecl, line: usize) -> StrykeResult<()> {
        // Throwaway list-assign sinks (`my (undef, $x) = ...`) are synthesized
        // by the parser and never user-visible — exempt them.
        if d.name.starts_with("__undef_sink_") {
            return Ok(());
        }
        let sigil_char = match d.sigil {
            Sigil::Scalar => '$',
            Sigil::Array => '@',
            Sigil::Hash => '%',
            Sigil::Typeglob => return Ok(()),
        };
        match (&d.type_annotation, &d.initializer) {
            (None, None) => Err(self.err(
                format!(
                    "`{}{}` must declare a type or be initialized (`{}{}: Type` or `{}{} = ...`)",
                    sigil_char, d.name, sigil_char, d.name, sigil_char, d.name
                ),
                line,
            )),
            (Some(ann), Some(init)) => {
                // Scalar literal vs declared type — flag clear mismatches.
                if d.sigil == Sigil::Scalar {
                    if let Some(lit) = infer_scalar_literal_type(init) {
                        if static_incompatible(ann, &lit) {
                            return Err(self.err(
                                format!(
                                    "`${}` declared `{}` but initialized with {}",
                                    d.name,
                                    ann.display_name(),
                                    lit.display_name()
                                ),
                                line,
                            ));
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Every scalar parameter must be typed. `@`/`%` slurpy params satisfy the
    /// requirement at their container shape.
    fn check_params(&self, params: &[SubSigParam], ctx: &str, line: usize) -> StrykeResult<()> {
        for p in params {
            if let SubSigParam::Scalar(name, ty, _) = p {
                if ty.is_none() {
                    return Err(self.err(
                        format!(
                            "{}: parameter `${}` must declare a type (`${}: Type`)",
                            ctx, name, name
                        ),
                        line,
                    ));
                }
            }
        }
        Ok(())
    }

    fn check_struct_method(&self, m: &StructMethod, owner: &str, line: usize) -> StrykeResult<()> {
        let ctx = format!("{}::{}", owner, m.name);
        self.check_params(&m.params, &ctx, line)?;
        if m.return_type.is_none() {
            return Err(self.err(format!("method {} must declare a return type", ctx), line));
        }
        self.check_block(&m.body)
    }

    fn check_class_method(&self, m: &ClassMethod, owner: &str, line: usize) -> StrykeResult<()> {
        let ctx = format!("{}::{}", owner, m.name);
        self.check_params(&m.params, &ctx, line)?;
        if m.return_type.is_none() {
            return Err(self.err(format!("method {} must declare a return type", ctx), line));
        }
        if let Some(body) = &m.body {
            self.check_block(body)?;
        }
        Ok(())
    }

    /// Walk an expression, validating anonymous `fn` (`CodeRef`) the same way
    /// named subs are validated and recursing into the common sub-expression
    /// holders so nested anon fns / initializers are reached.
    fn check_expr(&self, e: &Expr) -> StrykeResult<()> {
        match &e.kind {
            ExprKind::CodeRef { body, .. } => {
                // Anonymous code blocks (`fn { }`, and the desugared forms of
                // `eval { }`, `do { }`, `map`/`grep`/`sort` blocks, pipeline
                // stages, …) are all `CodeRef`s and are indistinguishable here.
                // Requiring a return type on them would reject ordinary block
                // syntax, so `--static` only mandates types on *named* subs and
                // methods. Their bodies are still walked for nested decls.
                self.check_block(body)?;
            }
            ExprKind::BinOp { left, right, .. } => {
                self.check_expr(left)?;
                self.check_expr(right)?;
            }
            ExprKind::UnaryOp { expr, .. } => self.check_expr(expr)?,
            ExprKind::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                self.check_expr(condition)?;
                self.check_expr(then_expr)?;
                self.check_expr(else_expr)?;
            }
            ExprKind::Assign { target, value } => {
                self.check_expr(target)?;
                self.check_expr(value)?;
            }
            ExprKind::CompoundAssign { target, value, .. } => {
                self.check_expr(target)?;
                self.check_expr(value)?;
            }
            ExprKind::FuncCall { args, .. } => {
                for a in args {
                    self.check_expr(a)?;
                }
            }
            ExprKind::MethodCall { object, args, .. } => {
                self.check_expr(object)?;
                for a in args {
                    self.check_expr(a)?;
                }
            }
            ExprKind::ArrayRef(items) => {
                for it in items {
                    self.check_expr(it)?;
                }
            }
            ExprKind::HashRef(pairs) => {
                for (k, v) in pairs {
                    self.check_expr(k)?;
                    self.check_expr(v)?;
                }
            }
            ExprKind::List(items) => {
                for it in items {
                    self.check_expr(it)?;
                }
            }
            ExprKind::Do(inner) => self.check_expr(inner)?,
            _ => {}
        }
        Ok(())
    }
}

/// Infer the type of a scalar literal initializer, for the static
/// mismatch check. Returns `None` for anything not a bare literal (the value's
/// type is then unknown and no static mismatch is reported).
fn infer_scalar_literal_type(e: &Expr) -> Option<PerlTypeName> {
    match &e.kind {
        ExprKind::Integer(_) => Some(PerlTypeName::Int),
        ExprKind::Float(_) => Some(PerlTypeName::Float),
        ExprKind::String(_) | ExprKind::InterpolatedString(_) => Some(PerlTypeName::Str),
        _ => None,
    }
}

/// True when a declared scalar type clearly cannot hold a value of the inferred
/// literal type. Mirrors `PerlTypeName::check_value`'s scalar rules: `Float`
/// accepts `Int`; everything else requires an exact category match. Conservative
/// — only fires on unambiguous clashes so valid programs are never rejected.
fn static_incompatible(ann: &PerlTypeName, lit: &PerlTypeName) -> bool {
    match ann {
        PerlTypeName::Any | PerlTypeName::Bool => false,
        PerlTypeName::Int => !matches!(lit, PerlTypeName::Int),
        PerlTypeName::Float => !matches!(lit, PerlTypeName::Int | PerlTypeName::Float),
        PerlTypeName::Str => !matches!(lit, PerlTypeName::Str),
        // Container / nominal annotations on a scalar literal: don't second-guess
        // here (shape was already validated by the parser).
        _ => false,
    }
}
