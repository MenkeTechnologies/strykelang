use crate::ast::*;
use crate::error::{ErrorKind, PerlError, PerlResult};
use crate::interpreter::Interpreter;
use crate::lexer::{Lexer, LITERAL_DOLLAR_IN_DQUOTE};
use crate::token::Token;

/// True when `[` after `expr` is chained array access (`$r->{k}[0]`, `$a[1][2]`, `$$r[0]`).
/// False for `(sort ...)[0]` / `@{ ... }[i]` — those slice a list value, not an array ref container.
fn postfix_lbracket_is_arrow_container(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::ArrayElement { .. }
            | ExprKind::HashElement { .. }
            | ExprKind::ArrowDeref { .. }
            | ExprKind::Deref {
                kind: Sigil::Scalar,
                ..
            }
    )
}

fn destructure_stmt_from_var_decls(keyword: &str, decls: Vec<VarDecl>, line: usize) -> Statement {
    let kind = match keyword {
        "my" => StmtKind::My(decls),
        "mysync" => StmtKind::MySync(decls),
        "our" => StmtKind::Our(decls),
        "local" => StmtKind::Local(decls),
        "state" => StmtKind::State(decls),
        _ => unreachable!("parse_my_our_local keyword"),
    };
    Statement {
        label: None,
        kind,
        line,
    }
}

fn destructure_stmt_die_string(line: usize, msg: &str) -> Statement {
    Statement {
        label: None,
        kind: StmtKind::Expression(Expr {
            kind: ExprKind::Die(vec![Expr {
                kind: ExprKind::String(msg.to_string()),
                line,
            }]),
            line,
        }),
        line,
    }
}

fn destructure_stmt_unless_die(line: usize, cond: Expr, msg: &str) -> Statement {
    Statement {
        label: None,
        kind: StmtKind::Unless {
            condition: cond,
            body: vec![destructure_stmt_die_string(line, msg)],
            else_block: None,
        },
        line,
    }
}

fn destructure_expr_scalar_tmp(name: &str, line: usize) -> Expr {
    Expr {
        kind: ExprKind::ScalarVar(name.to_string()),
        line,
    }
}

fn destructure_expr_array_len(tmp: &str, line: usize) -> Expr {
    Expr {
        kind: ExprKind::Deref {
            expr: Box::new(destructure_expr_scalar_tmp(tmp, line)),
            kind: Sigil::Array,
        },
        line,
    }
}

pub struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
    /// Monotonic slot id for `rate_limit(...)` sliding-window state in the interpreter.
    next_rate_limit_slot: u32,
    /// When > 0, `expr` `(` is not parsed as [`ExprKind::IndirectCall`] — e.g. `sort $k (1)` must
    /// treat `(1)` as the sort list, not `$k(1)`.
    suppress_indirect_paren_call: u32,
    /// When > 0, the current expression is being parsed as the RHS of `|>`
    /// (pipe-forward). Builtins that normally require a list/string/second arg
    /// (`map`, `grep`, `sort`, `join`, `reverse` / `reversed`, `split`, …) may accept a
    /// placeholder when this flag is set, because [`Self::pipe_forward_apply`]
    /// will substitute the piped value in afterwards.
    pipe_rhs_depth: u32,
    /// When > 0, [`Self::parse_pipe_forward`] will **not** consume a trailing `|>`
    /// and leaves it for an outer parser instead. Bumped while parsing paren-less
    /// arg lists (`parse_list_until_terminator`, paren-less method args, `map`/`grep`
    /// LIST, …) so `@a |> head 2 |> join "-"` chains left-associatively as
    /// `(@a |> head 2) |> join "-"` instead of `head` swallowing the outer `|>`
    /// as part of its first arg. Reset to 0 on entry to any parenthesized
    /// arg list (`parse_arg_list`) so `head(2 |> foo, 3)` still works.
    no_pipe_forward_depth: u32,
    /// When > 0, `{` after a scalar / scalar deref is not `%hash{key}` / `->{}`, so
    /// `if let` / `while let` scrutinees can be followed by `{ ... }`.
    suppress_scalar_hash_brace: u32,
    /// Counter for `while let` / similar desugar temps (`$__while_let_0`, …).
    next_desugar_tmp: u32,
    /// Source path for [`PerlError`] (matches lexer / `parse_with_file`).
    error_file: String,
}

impl Parser {
    pub fn new(tokens: Vec<(Token, usize)>) -> Self {
        Self::new_with_file(tokens, "-e")
    }

    pub fn new_with_file(tokens: Vec<(Token, usize)>, file: impl Into<String>) -> Self {
        Self {
            tokens,
            pos: 0,
            next_rate_limit_slot: 0,
            suppress_indirect_paren_call: 0,
            pipe_rhs_depth: 0,
            no_pipe_forward_depth: 0,
            suppress_scalar_hash_brace: 0,
            next_desugar_tmp: 0,
            error_file: file.into(),
        }
    }

    fn alloc_desugar_tmp(&mut self) -> u32 {
        let n = self.next_desugar_tmp;
        self.next_desugar_tmp = self.next_desugar_tmp.saturating_add(1);
        n
    }

    /// True when we are currently parsing the RHS of a `|>` pipe-forward.
    /// Used by builtins (`map`, `grep`, `sort`, `join`, …) to supply a
    /// placeholder list instead of erroring on a missing operand.
    #[inline]
    fn in_pipe_rhs(&self) -> bool {
        self.pipe_rhs_depth > 0
    }

    /// List-slurping builtin: the operand is entirely the LHS of `|>` (no following list tokens).
    fn pipe_supplies_slurped_list_operand(&self) -> bool {
        self.in_pipe_rhs()
            && matches!(
                self.peek(),
                Token::Semicolon
                    | Token::RBrace
                    | Token::RParen
                    | Token::Eof
                    | Token::Comma
                    | Token::PipeForward
            )
    }

    /// Empty placeholder list used as a stand-in for the list operand of
    /// list-taking builtins when they appear on the RHS of `|>`.
    /// [`Self::pipe_forward_apply`] rewrites this slot with the actual piped
    /// value at desugar time, so the placeholder is never evaluated.
    #[inline]
    fn pipe_placeholder_list(&self, line: usize) -> Expr {
        Expr {
            kind: ExprKind::List(vec![]),
            line,
        }
    }

    /// Lift a `Bareword("f")` to `FuncCall { f, [$_] }`.
    ///
    /// perlrs extension contexts (map/grep/fore expression forms, pipe-forward)
    /// call this so that `map sha512, @list` invokes `sha512($_)` for each
    /// element instead of stringifying the bareword.  Non-bareword expressions
    /// pass through unchanged.
    ///
    /// Also injects `$_` into known builtins that were parsed with zero
    /// arguments (e.g. `fore unlink`, `map stat`) so they operate on the
    /// topic variable instead of being no-ops.
    fn lift_bareword_to_topic_call(expr: Expr) -> Expr {
        let line = expr.line;
        let topic = || Expr {
            kind: ExprKind::ScalarVar("_".into()),
            line,
        };
        match expr.kind {
            ExprKind::Bareword(ref name) => Expr {
                kind: ExprKind::FuncCall {
                    name: name.clone(),
                    args: vec![topic()],
                },
                line,
            },
            // Builtins that take Vec<Expr> args — inject $_ when empty.
            ExprKind::Unlink(ref args) if args.is_empty() => Expr {
                kind: ExprKind::Unlink(vec![topic()]),
                line,
            },
            ExprKind::Chmod(ref args) if args.is_empty() => Expr {
                kind: ExprKind::Chmod(vec![topic()]),
                line,
            },
            // Builtins that take Box<Expr> — inject $_ when arg is implicit.
            ExprKind::Stat(_) => expr,
            ExprKind::Lstat(_) => expr,
            ExprKind::Readlink(_) => expr,
            _ => expr,
        }
    }

    /// Like [`Self::lift_bareword_to_topic_call`] but also rewrites
    /// builtins that require exactly one argument (stat, lstat, readlink)
    /// from a parse error into `builtin($_)` when used in pipe contexts
    /// where the parser would otherwise reject the zero-arg form.
    fn parse_pipe_builtin_or_expr(&mut self) -> PerlResult<Expr> {
        let line = self.peek_line();
        let topic = Expr {
            kind: ExprKind::ScalarVar("_".into()),
            line,
        };
        match self.peek() {
            Token::Ident(ref kw)
                if matches!(
                    kw.as_str(),
                    "stat" | "lstat" | "readlink" | "unlink" | "rm"
                        | "rmdir" | "chdir" | "chmod"
                ) =>
            {
                let kw = kw.clone();
                self.advance(); // consume keyword
                // Try to parse args; if none follow, inject $_.
                let args = self.parse_builtin_args()?;
                match kw.as_str() {
                    "stat" => {
                        let arg = if args.is_empty() {
                            topic
                        } else {
                            args[0].clone()
                        };
                        Ok(Expr {
                            kind: ExprKind::Stat(Box::new(arg)),
                            line,
                        })
                    }
                    "lstat" => {
                        let arg = if args.is_empty() {
                            topic
                        } else {
                            args[0].clone()
                        };
                        Ok(Expr {
                            kind: ExprKind::Lstat(Box::new(arg)),
                            line,
                        })
                    }
                    "readlink" => {
                        let arg = if args.is_empty() {
                            topic
                        } else {
                            args[0].clone()
                        };
                        Ok(Expr {
                            kind: ExprKind::Readlink(Box::new(arg)),
                            line,
                        })
                    }
                    "unlink" | "rm" => {
                        let a = if args.is_empty() {
                            vec![topic]
                        } else {
                            args
                        };
                        Ok(Expr {
                            kind: ExprKind::Unlink(a),
                            line,
                        })
                    }
                    "rmdir" => {
                        let a = if args.is_empty() {
                            vec![topic]
                        } else {
                            args
                        };
                        Ok(Expr {
                            kind: ExprKind::FuncCall {
                                name: "rmdir".into(),
                                args: a,
                            },
                            line,
                        })
                    }
                    "chdir" => {
                        let arg = if args.is_empty() {
                            topic
                        } else {
                            args[0].clone()
                        };
                        Ok(Expr {
                            kind: ExprKind::Chdir(Box::new(arg)),
                            line,
                        })
                    }
                    "chmod" => {
                        let a = if args.is_empty() {
                            vec![topic]
                        } else {
                            args
                        };
                        Ok(Expr {
                            kind: ExprKind::Chmod(a),
                            line,
                        })
                    }
                    _ => unreachable!(),
                }
            }
            _ => {
                let expr = self.parse_assign_expr_stop_at_pipe()?;
                Ok(Self::lift_bareword_to_topic_call(expr))
            }
        }
    }

    /// `parse_assign_expr` with `no_pipe_forward_depth` bumped for the
    /// duration, so any trailing `|>` is left to the enclosing parser instead
    /// of being absorbed into this sub-expression. Used by paren-less arg
    /// parsers (`parse_list_until_terminator`, `chunked`/`windowed` paren-less,
    /// paren-less method args, …) so `@a |> head 2 |> join "-"` chains
    /// left-associatively instead of letting `head`'s first arg swallow the
    /// outer `|>`. The counter is restored on both success and error paths.
    fn parse_assign_expr_stop_at_pipe(&mut self) -> PerlResult<Expr> {
        self.no_pipe_forward_depth = self.no_pipe_forward_depth.saturating_add(1);
        let r = self.parse_assign_expr();
        self.no_pipe_forward_depth = self.no_pipe_forward_depth.saturating_sub(1);
        r
    }

    fn syntax_err(&self, message: impl Into<String>, line: usize) -> PerlError {
        PerlError::new(ErrorKind::Syntax, message, line, self.error_file.clone())
    }

    fn alloc_rate_limit_slot(&mut self) -> u32 {
        let s = self.next_rate_limit_slot;
        self.next_rate_limit_slot = self.next_rate_limit_slot.saturating_add(1);
        s
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .map(|(t, _)| t)
            .unwrap_or(&Token::Eof)
    }

    fn peek_line(&self) -> usize {
        self.tokens.get(self.pos).map(|(_, l)| *l).unwrap_or(0)
    }

    fn peek_at(&self, offset: usize) -> &Token {
        self.tokens
            .get(self.pos + offset)
            .map(|(t, _)| t)
            .unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> (Token, usize) {
        let tok = self
            .tokens
            .get(self.pos)
            .cloned()
            .unwrap_or((Token::Eof, 0));
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> PerlResult<usize> {
        let (tok, line) = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(line)
        } else {
            Err(self.syntax_err(format!("Expected {:?}, got {:?}", expected, tok), line))
        }
    }

    fn eat(&mut self, expected: &Token) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    /// True when a file test (`-d`, `-f`, …) may omit its operand and use `$_` (Perl filetest default).
    fn filetest_allows_implicit_topic(tok: &Token) -> bool {
        matches!(
            tok,
            Token::RParen
                | Token::Semicolon
                | Token::Comma
                | Token::RBrace
                | Token::Eof
                | Token::LogAnd
                | Token::LogOr
                | Token::LogAndWord
                | Token::LogOrWord
                | Token::PipeForward
        )
    }

    // ── Top level ──

    pub fn parse_program(&mut self) -> PerlResult<Program> {
        let mut statements = Vec::new();
        while !self.at_eof() {
            if matches!(self.peek(), Token::Semicolon) {
                let line = self.peek_line();
                self.advance();
                statements.push(Statement {
                    label: None,
                    kind: StmtKind::Empty,
                    line,
                });
                continue;
            }
            statements.push(self.parse_statement()?);
        }
        Ok(Program { statements })
    }

    // ── Statements ──

    fn parse_statement(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();

        // Statement label `FOO:` / `boot:` / `BAR_BAZ:` (not `Foo::` — that is `Ident` + `::`).
        // Uppercase-only was too strict: XSLoader.pm uses `boot:` before `my $xs = ...`.
        let label = match self.peek().clone() {
            Token::Ident(_) => {
                if matches!(self.peek_at(1), Token::Colon)
                    && !matches!(self.peek_at(2), Token::Colon)
                {
                    let (tok, _) = self.advance();
                    let l = match tok {
                        Token::Ident(l) => l,
                        _ => unreachable!(),
                    };
                    self.advance(); // ':'
                    Some(l)
                } else {
                    None
                }
            }
            _ => None,
        };

        let mut stmt =
            match self.peek().clone() {
                Token::FormatDecl { .. } => {
                    let tok_line = self.peek_line();
                    let (tok, _) = self.advance();
                    match tok {
                        Token::FormatDecl { name, lines } => Statement {
                            label: label.clone(),
                            kind: StmtKind::FormatDecl { name, lines },
                            line: tok_line,
                        },
                        _ => unreachable!(),
                    }
                }
                Token::Ident(ref kw) => match kw.as_str() {
                    "if" => self.parse_if()?,
                    "unless" => self.parse_unless()?,
                    "while" => {
                        let mut s = self.parse_while()?;
                        if let StmtKind::While {
                            label: ref mut lbl, ..
                        } = s.kind
                        {
                            *lbl = label.clone();
                        }
                        s
                    }
                    "until" => {
                        let mut s = self.parse_until()?;
                        if let StmtKind::Until {
                            label: ref mut lbl, ..
                        } = s.kind
                        {
                            *lbl = label.clone();
                        }
                        s
                    }
                    "for" => {
                        let mut s = self.parse_for_or_foreach()?;
                        match s.kind {
                            StmtKind::For {
                                label: ref mut lbl, ..
                            }
                            | StmtKind::Foreach {
                                label: ref mut lbl, ..
                            } => *lbl = label.clone(),
                            _ => {}
                        }
                        s
                    }
                    "foreach" => {
                        let mut s = self.parse_foreach()?;
                        if let StmtKind::Foreach {
                            label: ref mut lbl, ..
                        } = s.kind
                        {
                            *lbl = label.clone();
                        }
                        s
                    }
                    "sub" => self.parse_sub_decl()?,
                    "struct" => self.parse_struct_decl()?,
                    "my" => self.parse_my_our_local("my", false)?,
                    "state" => self.parse_my_our_local("state", false)?,
                    "mysync" => self.parse_my_our_local("mysync", false)?,
                    "frozen" => {
                        // frozen my $x = val; — expect "my" keyword after "frozen"
                        self.advance(); // consume "frozen"
                        if let Token::Ident(ref kw) = self.peek().clone() {
                            if kw == "my" {
                                let mut stmt = self.parse_my_our_local("my", false)?;
                                // Mark all decls as frozen
                                if let StmtKind::My(ref mut decls) = stmt.kind {
                                    for decl in decls.iter_mut() {
                                        decl.frozen = true;
                                    }
                                }
                                stmt
                            } else {
                                return Err(self
                                    .syntax_err("Expected 'my' after 'frozen'", self.peek_line()));
                            }
                        } else {
                            return Err(
                                self.syntax_err("Expected 'my' after 'frozen'", self.peek_line())
                            );
                        }
                    }
                    "typed" => {
                        self.advance();
                        if let Token::Ident(ref kw) = self.peek().clone() {
                            if kw == "my" {
                                self.parse_my_our_local("my", true)?
                            } else {
                                return Err(self
                                    .syntax_err("Expected 'my' after 'typed'", self.peek_line()));
                            }
                        } else {
                            return Err(
                                self.syntax_err("Expected 'my' after 'typed'", self.peek_line())
                            );
                        }
                    }
                    "our" => self.parse_my_our_local("our", false)?,
                    "local" => self.parse_my_our_local("local", false)?,
                    "package" => self.parse_package()?,
                    "use" => self.parse_use()?,
                    "no" => self.parse_no()?,
                    "return" => self.parse_return()?,
                    "last" => {
                        self.advance();
                        let lbl = if let Token::Ident(ref s) = self.peek() {
                            if s.chars().all(|c| c.is_uppercase() || c == '_') {
                                let (Token::Ident(l), _) = self.advance() else {
                                    unreachable!()
                                };
                                Some(l)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        let stmt = Statement {
                            label: None,
                            kind: StmtKind::Last(lbl.or(label.clone())),
                            line,
                        };
                        self.parse_stmt_postfix_modifier(stmt)?
                    }
                    "next" => {
                        self.advance();
                        let lbl = if let Token::Ident(ref s) = self.peek() {
                            if s.chars().all(|c| c.is_uppercase() || c == '_') {
                                let (Token::Ident(l), _) = self.advance() else {
                                    unreachable!()
                                };
                                Some(l)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        let stmt = Statement {
                            label: None,
                            kind: StmtKind::Next(lbl.or(label.clone())),
                            line,
                        };
                        self.parse_stmt_postfix_modifier(stmt)?
                    }
                    "redo" => {
                        self.advance();
                        self.eat(&Token::Semicolon);
                        Statement {
                            label: None,
                            kind: StmtKind::Redo(label.clone()),
                            line,
                        }
                    }
                    "BEGIN" => {
                        self.advance();
                        let block = self.parse_block()?;
                        Statement {
                            label: None,
                            kind: StmtKind::Begin(block),
                            line,
                        }
                    }
                    "END" => {
                        self.advance();
                        let block = self.parse_block()?;
                        Statement {
                            label: None,
                            kind: StmtKind::End(block),
                            line,
                        }
                    }
                    "UNITCHECK" => {
                        self.advance();
                        let block = self.parse_block()?;
                        Statement {
                            label: None,
                            kind: StmtKind::UnitCheck(block),
                            line,
                        }
                    }
                    "CHECK" => {
                        self.advance();
                        let block = self.parse_block()?;
                        Statement {
                            label: None,
                            kind: StmtKind::Check(block),
                            line,
                        }
                    }
                    "INIT" => {
                        self.advance();
                        let block = self.parse_block()?;
                        Statement {
                            label: None,
                            kind: StmtKind::Init(block),
                            line,
                        }
                    }
                    "goto" => {
                        self.advance();
                        let target = self.parse_expression()?;
                        let stmt = Statement {
                            label: None,
                            kind: StmtKind::Goto {
                                target: Box::new(target),
                            },
                            line,
                        };
                        // `goto $l if COND;` / `goto &$cr if defined &$cr;` (XSLoader.pm)
                        self.parse_stmt_postfix_modifier(stmt)?
                    }
                    "continue" => {
                        self.advance();
                        let block = self.parse_block()?;
                        Statement {
                            label: None,
                            kind: StmtKind::Continue(block),
                            line,
                        }
                    }
                    "try" => self.parse_try_catch()?,
                    "tie" => self.parse_tie_stmt()?,
                    "given" => self.parse_given()?,
                    "when" => self.parse_when_stmt()?,
                    "default" => self.parse_default_stmt()?,
                    "eval_timeout" => self.parse_eval_timeout()?,
                    "do" => {
                        if matches!(self.peek_at(1), Token::LBrace) {
                            self.advance();
                            let body = self.parse_block()?;
                            if let Token::Ident(ref w) = self.peek().clone() {
                                if w == "while" {
                                    self.advance();
                                    self.expect(&Token::LParen)?;
                                    let mut condition = self.parse_expression()?;
                                    Self::mark_match_scalar_g_for_boolean_condition(&mut condition);
                                    self.expect(&Token::RParen)?;
                                    self.eat(&Token::Semicolon);
                                    Statement {
                                        label: label.clone(),
                                        kind: StmtKind::DoWhile { body, condition },
                                        line,
                                    }
                                } else {
                                    let inner_line = body.first().map(|s| s.line).unwrap_or(line);
                                    let inner = Expr {
                                        kind: ExprKind::CodeRef {
                                            params: vec![],
                                            body,
                                        },
                                        line: inner_line,
                                    };
                                    let expr = Expr {
                                        kind: ExprKind::Do(Box::new(inner)),
                                        line,
                                    };
                                    let stmt = Statement {
                                        label: label.clone(),
                                        kind: StmtKind::Expression(expr),
                                        line,
                                    };
                                    // `do { } if EXPR` / `do { } unless EXPR` — postfix modifier, not a new `if (` statement.
                                    self.parse_stmt_postfix_modifier(stmt)?
                                }
                            } else {
                                let inner_line = body.first().map(|s| s.line).unwrap_or(line);
                                let inner = Expr {
                                    kind: ExprKind::CodeRef {
                                        params: vec![],
                                        body,
                                    },
                                    line: inner_line,
                                };
                                let expr = Expr {
                                    kind: ExprKind::Do(Box::new(inner)),
                                    line,
                                };
                                let stmt = Statement {
                                    label: label.clone(),
                                    kind: StmtKind::Expression(expr),
                                    line,
                                };
                                self.parse_stmt_postfix_modifier(stmt)?
                            }
                        } else {
                            if let Some(expr) = self.try_parse_bareword_stmt_call() {
                                let stmt = self.maybe_postfix_modifier(expr)?;
                                self.parse_stmt_postfix_modifier(stmt)?
                            } else {
                                let expr = self.parse_expression()?;
                                let stmt = self.maybe_postfix_modifier(expr)?;
                                self.parse_stmt_postfix_modifier(stmt)?
                            }
                        }
                    }
                    _ => {
                        // `foo;` or `{ foo }` — bareword statement is a zero-arg call (topic `$_` at runtime).
                        if let Some(expr) = self.try_parse_bareword_stmt_call() {
                            let stmt = self.maybe_postfix_modifier(expr)?;
                            self.parse_stmt_postfix_modifier(stmt)?
                        } else {
                            let expr = self.parse_expression()?;
                            let stmt = self.maybe_postfix_modifier(expr)?;
                            self.parse_stmt_postfix_modifier(stmt)?
                        }
                    }
                },
                Token::LBrace => {
                    let block = self.parse_block()?;
                    let stmt = Statement {
                        label: None,
                        kind: StmtKind::Block(block),
                        line,
                    };
                    // `{ … } if EXPR` / `{ … } unless EXPR` — same postfix rule as `do { } if …` (not `if (`).
                    self.parse_stmt_postfix_modifier(stmt)?
                }
                _ => {
                    let expr = self.parse_expression()?;
                    let stmt = self.maybe_postfix_modifier(expr)?;
                    self.parse_stmt_postfix_modifier(stmt)?
                }
            };

        stmt.label = label;
        Ok(stmt)
    }

    /// Handle postfix if/unless on statement-level keywords like last/next.
    fn parse_stmt_postfix_modifier(&mut self, stmt: Statement) -> PerlResult<Statement> {
        let line = stmt.line;
        if let Token::Ident(ref kw) = self.peek().clone() {
            match kw.as_str() {
                "if" => {
                    self.advance();
                    let mut cond = self.parse_expression()?;
                    Self::mark_match_scalar_g_for_boolean_condition(&mut cond);
                    self.eat(&Token::Semicolon);
                    return Ok(Statement {
                        label: None,
                        kind: StmtKind::If {
                            condition: cond,
                            body: vec![stmt],
                            elsifs: vec![],
                            else_block: None,
                        },
                        line,
                    });
                }
                "unless" => {
                    self.advance();
                    let mut cond = self.parse_expression()?;
                    Self::mark_match_scalar_g_for_boolean_condition(&mut cond);
                    self.eat(&Token::Semicolon);
                    return Ok(Statement {
                        label: None,
                        kind: StmtKind::Unless {
                            condition: cond,
                            body: vec![stmt],
                            else_block: None,
                        },
                        line,
                    });
                }
                "while" | "until" | "for" | "foreach" => {
                    // `do { } for @a` / `{ } while COND` — same postfix forms as [`maybe_postfix_modifier`],
                    // not a new `for (` / `while (` statement (which would require `(` after `for`).
                    if let Some(expr) = Self::stmt_into_postfix_body_expr(stmt) {
                        let out = self.maybe_postfix_modifier(expr)?;
                        self.eat(&Token::Semicolon);
                        return Ok(out);
                    }
                    return Err(self.syntax_err(
                        format!("postfix `{}` is not supported on this statement form", kw),
                        self.peek_line(),
                    ));
                }
                // `{ } pmap @a` / `{ } pflat_map @a` / `{ } pfor @a` / `do { } …` — same shapes as prefix forms.
                "pmap" | "pflat_map" | "pgrep" | "pfor" | "preduce" | "pcache" => {
                    let line = stmt.line;
                    let block = self.stmt_into_parallel_block(stmt)?;
                    let which = kw.as_str();
                    self.advance();
                    self.eat(&Token::Comma);
                    let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                    self.eat(&Token::Semicolon);
                    let list = Box::new(list);
                    let progress = progress.map(Box::new);
                    let kind = match which {
                        "pmap" => ExprKind::PMapExpr {
                            block,
                            list,
                            progress,
                            flat_outputs: false,
                            on_cluster: None,
                        },
                        "pflat_map" => ExprKind::PMapExpr {
                            block,
                            list,
                            progress,
                            flat_outputs: true,
                            on_cluster: None,
                        },
                        "pgrep" => ExprKind::PGrepExpr {
                            block,
                            list,
                            progress,
                        },
                        "pfor" => ExprKind::PForExpr {
                            block,
                            list,
                            progress,
                        },
                        "preduce" => ExprKind::PReduceExpr {
                            block,
                            list,
                            progress,
                        },
                        "pcache" => ExprKind::PcacheExpr {
                            block,
                            list,
                            progress,
                        },
                        _ => unreachable!(),
                    };
                    return Ok(Statement {
                        label: None,
                        kind: StmtKind::Expression(Expr { kind, line }),
                        line,
                    });
                }
                _ => {}
            }
        }
        self.eat(&Token::Semicolon);
        Ok(stmt)
    }

    /// Block body for postfix `pmap` / `pfor` / … — bare `{ }`, `do { }`, or any expression
    /// statement (wrapped as a one-line block, e.g. `` `cmd` pfor @a ``).
    fn stmt_into_parallel_block(&self, stmt: Statement) -> PerlResult<Block> {
        let line = stmt.line;
        match stmt.kind {
            StmtKind::Block(block) => Ok(block),
            StmtKind::Expression(expr) => {
                if let ExprKind::Do(ref inner) = expr.kind {
                    if let ExprKind::CodeRef { ref body, .. } = inner.kind {
                        return Ok(body.clone());
                    }
                }
                Ok(vec![Statement {
                    label: None,
                    kind: StmtKind::Expression(expr),
                    line,
                }])
            }
            _ => Err(self.syntax_err(
                "postfix parallel op expects `do { }`, a bare `{ }` block, or an expression statement",
                line,
            )),
        }
    }

    /// `StmtKind::Expression` or a bare block (`StmtKind::Block`) as an [`Expr`] for postfix
    /// `while` / `until` / `for` / `foreach` (mirrors `do { }` → [`ExprKind::Do`](ExprKind::Do)([`CodeRef`](ExprKind::CodeRef))).
    fn stmt_into_postfix_body_expr(stmt: Statement) -> Option<Expr> {
        match stmt.kind {
            StmtKind::Expression(expr) => Some(expr),
            StmtKind::Block(block) => {
                let line = stmt.line;
                let inner = Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body: block,
                    },
                    line,
                };
                Some(Expr {
                    kind: ExprKind::Do(Box::new(inner)),
                    line,
                })
            }
            _ => None,
        }
    }

    /// Statement-modifier keywords that must not be consumed as part of a comma-separated list
    /// (same set as [`parse_list_until_terminator`]).
    fn peek_is_postfix_stmt_modifier_keyword(&self) -> bool {
        matches!(
            self.peek(),
            Token::Ident(ref kw)
                if matches!(
                    kw.as_str(),
                    "if" | "unless" | "while" | "until" | "for" | "foreach"
                )
        )
    }

    fn maybe_postfix_modifier(&mut self, expr: Expr) -> PerlResult<Statement> {
        let line = expr.line;
        match self.peek() {
            Token::Ident(ref kw) => match kw.as_str() {
                "if" => {
                    self.advance();
                    let cond = self.parse_expression()?;
                    Ok(Statement {
                        label: None,
                        kind: StmtKind::Expression(Expr {
                            kind: ExprKind::PostfixIf {
                                expr: Box::new(expr),
                                condition: Box::new(cond),
                            },
                            line,
                        }),
                        line,
                    })
                }
                "unless" => {
                    self.advance();
                    let cond = self.parse_expression()?;
                    Ok(Statement {
                        label: None,
                        kind: StmtKind::Expression(Expr {
                            kind: ExprKind::PostfixUnless {
                                expr: Box::new(expr),
                                condition: Box::new(cond),
                            },
                            line,
                        }),
                        line,
                    })
                }
                "while" => {
                    self.advance();
                    let cond = self.parse_expression()?;
                    Ok(Statement {
                        label: None,
                        kind: StmtKind::Expression(Expr {
                            kind: ExprKind::PostfixWhile {
                                expr: Box::new(expr),
                                condition: Box::new(cond),
                            },
                            line,
                        }),
                        line,
                    })
                }
                "until" => {
                    self.advance();
                    let cond = self.parse_expression()?;
                    Ok(Statement {
                        label: None,
                        kind: StmtKind::Expression(Expr {
                            kind: ExprKind::PostfixUntil {
                                expr: Box::new(expr),
                                condition: Box::new(cond),
                            },
                            line,
                        }),
                        line,
                    })
                }
                "for" | "foreach" => {
                    self.advance();
                    let list = self.parse_expression()?;
                    Ok(Statement {
                        label: None,
                        kind: StmtKind::Expression(Expr {
                            kind: ExprKind::PostfixForeach {
                                expr: Box::new(expr),
                                list: Box::new(list),
                            },
                            line,
                        }),
                        line,
                    })
                }
                _ => Ok(Statement {
                    label: None,
                    kind: StmtKind::Expression(expr),
                    line,
                }),
            },
            _ => Ok(Statement {
                label: None,
                kind: StmtKind::Expression(expr),
                line,
            }),
        }
    }

    /// `name;` or `name}` — a bare identifier statement is a sub call with no explicit args (`$_` implied).
    fn try_parse_bareword_stmt_call(&mut self) -> Option<Expr> {
        let saved = self.pos;
        let line = self.peek_line();
        let mut name = match self.peek() {
            Token::Ident(n) => n.clone(),
            _ => return None,
        };
        // Names that begin `parse_named_expr` (builtins / `undef` / …) must use that path, not a sub call.
        if name.starts_with('\x00') || !Self::bareword_stmt_may_be_sub(&name) {
            return None;
        }
        self.advance();
        while self.eat(&Token::PackageSep) {
            match self.advance() {
                (Token::Ident(part), _) => {
                    name = format!("{}::{}", name, part);
                }
                _ => {
                    self.pos = saved;
                    return None;
                }
            }
        }
        match self.peek() {
            Token::Semicolon | Token::RBrace => Some(Expr {
                kind: ExprKind::FuncCall { name, args: vec![] },
                line,
            }),
            _ => {
                self.pos = saved;
                None
            }
        }
    }

    /// Identifiers that start a [`parse_named_expr`] arm (builtins / special forms), not a bare sub call.
    fn bareword_stmt_may_be_sub(name: &str) -> bool {
        !matches!(
            name,
            "__FILE__"
                | "__LINE__"
                | "abs"
                | "async"
                | "spawn"
                | "atan2"
                | "await"
                | "barrier"
                | "bless"
                | "caller"
                | "capture"
                | "chdir"
                | "chmod"
                | "chomp"
                | "chop"
                | "chr"
                | "chown"
                | "closedir"
                | "close"
                | "collect"
                | "cos"
                | "crypt"
                | "defined"
                | "delete"
                | "die"
                | "deque"
                | "do"
                | "each"
                | "eof"
                | "fore"
                | "eval"
                | "exec"
                | "exists"
                | "exit"
                | "exp"
                | "fan"
                | "fan_cap"
                | "fc"
                | "fetch_url"
                | "dirs"
                | "files"
                | "filesf"
                | "filter"
                | "getcwd"
                | "glob_par"
                | "par_sed"
                | "glob"
                | "grep"
                | "heap"
                | "hex"
                | "index"
                | "int"
                | "join"
                | "keys"
                | "lcfirst"
                | "lc"
                | "length"
                | "link"
                | "log"
                | "lstat"
                | "map"
                | "flat_map"
                | "flatten"
                | "set"
                | "list_count"
                | "list_size"
                | "count"
                | "size"
                | "cnt"
                | "all"
                | "any"
                | "none"
                | "take_while"
                | "drop_while"
                | "tap"
                | "peek"
                | "group_by"
                | "chunk_by"
                | "with_index"
                | "puniq"
                | "pfirst"
                | "pany"
                | "uniq"
                | "distinct"
                | "shuffle"
                | "chunked"
                | "windowed"
                | "match"
                | "mkdir"
                | "every"
                | "gen"
                | "oct"
                | "open"
                | "opendir"
                | "ord"
                | "par_lines"
                | "par_walk"
                | "pipe"
                | "pipes"
                | "block_devices"
                | "char_devices"
                | "rate_limit"
                | "retry"
                | "pcache"
                | "pchannel"
                | "pfor"
                | "pgrep"
                | "pipeline"
                | "pmap_chunked"
                | "pmap_reduce"
                | "pmap_on"
                | "pflat_map_on"
                | "pmap"
                | "pflat_map"
                | "pop"
                | "pos"
                | "ppool"
                | "preduce_init"
                | "preduce"
                | "pselect"
                | "printf"
                | "print"
                | "psort"
                | "push"
                | "pwatch"
                | "rand"
                | "readdir"
                | "readlink"
                | "reduce"
                | "fold"
                | "inject"
                | "first"
                | "detect"
                | "find"
                | "find_all"
                | "ref"
                | "rename"
                | "require"
                | "reverse"
                | "reversed"
                | "rewinddir"
                | "rindex"
                | "rmdir"
                | "rm"
                | "say"
                | "scalar"
                | "seekdir"
                | "shift"
                | "sin"
                | "slurp"
                | "sockets"
                | "sort"
                | "splice"
                | "split"
                | "sprintf"
                | "sqrt"
                | "srand"
                | "stat"
                | "study"
                | "substr"
                | "symlink"
                | "sym_links"
                | "system"
                | "telldir"
                | "timer"
                | "trace"
                | "ucfirst"
                | "uc"
                | "undef"
                | "umask"
                | "unlink"
                | "unshift"
                | "utime"
                | "values"
                | "wantarray"
                | "warn"
                | "watch"
                | "yield"
                | "sub"
        )
    }

    fn parse_block(&mut self) -> PerlResult<Block> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            if self.eat(&Token::Semicolon) {
                continue;
            }
            stmts.push(self.parse_statement()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(stmts)
    }

    /// `try { } catch ($err) { }` with optional `finally { }`
    fn parse_try_catch(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // try
        let try_block = self.parse_block()?;
        match self.peek() {
            Token::Ident(ref k) if k == "catch" => {
                self.advance();
            }
            _ => {
                return Err(self.syntax_err("expected 'catch' after try block", self.peek_line()));
            }
        }
        self.expect(&Token::LParen)?;
        let catch_var = self.parse_scalar_var_name()?;
        self.expect(&Token::RParen)?;
        let catch_block = self.parse_block()?;
        let finally_block = match self.peek() {
            Token::Ident(ref k) if k == "finally" => {
                self.advance();
                Some(self.parse_block()?)
            }
            _ => None,
        };
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::TryCatch {
                try_block,
                catch_var,
                catch_block,
                finally_block,
            },
            line,
        })
    }

    /// `tie %hash | tie @arr | tie $x , 'Class', ...args`
    fn parse_tie_stmt(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // tie
        let target = match self.peek().clone() {
            Token::HashVar(h) => {
                self.advance();
                TieTarget::Hash(h)
            }
            Token::ArrayVar(a) => {
                self.advance();
                TieTarget::Array(a)
            }
            Token::ScalarVar(s) => {
                self.advance();
                TieTarget::Scalar(s)
            }
            tok => {
                return Err(self.syntax_err(
                    format!("tie expects $scalar, @array, or %hash, got {:?}", tok),
                    self.peek_line(),
                ));
            }
        };
        self.expect(&Token::Comma)?;
        let class = self.parse_assign_expr()?;
        let mut args = Vec::new();
        while self.eat(&Token::Comma) {
            if matches!(self.peek(), Token::Semicolon | Token::RBrace | Token::Eof) {
                break;
            }
            args.push(self.parse_assign_expr()?);
        }
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::Tie {
                target,
                class,
                args,
            },
            line,
        })
    }

    /// `given (EXPR) { ... }`
    fn parse_given(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance();
        self.expect(&Token::LParen)?;
        let topic = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::Given { topic, body },
            line,
        })
    }

    /// `when (COND) { ... }` — only meaningful inside `given`
    fn parse_when_stmt(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance();
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::When { cond, body },
            line,
        })
    }

    /// `default { ... }` — only meaningful inside `given`
    fn parse_default_stmt(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance();
        let body = self.parse_block()?;
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::DefaultCase { body },
            line,
        })
    }

    /// `match (EXPR) { PATTERN => EXPR, ... }`
    fn parse_algebraic_match_expr(&mut self, line: usize) -> PerlResult<Expr> {
        self.expect(&Token::LParen)?;
        let subject = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::LBrace)?;
        let mut arms = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            if self.eat(&Token::Semicolon) {
                continue;
            }
            let pattern = self.parse_match_pattern()?;
            let guard = if matches!(self.peek(), Token::Ident(ref s) if s == "if") {
                self.advance();
                // Use assign-level parsing so `=>` after the guard is not consumed as a comma/fat-comma
                // separator (see [`Self::parse_comma_expr`]).
                Some(Box::new(self.parse_assign_expr()?))
            } else {
                None
            };
            self.expect(&Token::FatArrow)?;
            // Use assign-level parsing so commas separate arms, not `List` elements.
            let body = self.parse_assign_expr()?;
            arms.push(MatchArm {
                pattern,
                guard,
                body,
            });
            self.eat(&Token::Comma);
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr {
            kind: ExprKind::AlgebraicMatch {
                subject: Box::new(subject),
                arms,
            },
            line,
        })
    }

    fn parse_match_pattern(&mut self) -> PerlResult<MatchPattern> {
        match self.peek().clone() {
            Token::Regex(pattern, flags) => {
                self.advance();
                Ok(MatchPattern::Regex { pattern, flags })
            }
            Token::Ident(ref s) if s == "_" => {
                self.advance();
                Ok(MatchPattern::Any)
            }
            Token::Ident(ref s) if s == "Some" => {
                self.advance();
                self.expect(&Token::LParen)?;
                let name = self.parse_scalar_var_name()?;
                self.expect(&Token::RParen)?;
                Ok(MatchPattern::OptionSome(name))
            }
            Token::LBracket => self.parse_match_array_pattern(),
            Token::LBrace => self.parse_match_hash_pattern(),
            Token::LParen => {
                self.advance();
                let e = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                Ok(MatchPattern::Value(Box::new(e)))
            }
            _ => {
                let e = self.parse_assign_expr()?;
                Ok(MatchPattern::Value(Box::new(e)))
            }
        }
    }

    /// Contents of `[ ... ]` for algebraic array patterns and `sub ($a, [ ... ])` signatures.
    fn parse_match_array_elems_until_rbracket(&mut self) -> PerlResult<Vec<MatchArrayElem>> {
        let mut elems = Vec::new();
        if self.eat(&Token::RBracket) {
            return Ok(vec![]);
        }
        loop {
            if matches!(self.peek(), Token::Star) {
                self.advance();
                elems.push(MatchArrayElem::Rest);
                self.eat(&Token::Comma);
                if !matches!(self.peek(), Token::RBracket) {
                    return Err(self.syntax_err(
                        "`*` must be the last element in an array match pattern",
                        self.peek_line(),
                    ));
                }
                self.expect(&Token::RBracket)?;
                return Ok(elems);
            }
            if let Token::ArrayVar(name) = self.peek().clone() {
                self.advance();
                elems.push(MatchArrayElem::RestBind(name));
                self.eat(&Token::Comma);
                if !matches!(self.peek(), Token::RBracket) {
                    return Err(self.syntax_err(
                        "`@name` rest bind must be the last element in an array match pattern",
                        self.peek_line(),
                    ));
                }
                self.expect(&Token::RBracket)?;
                return Ok(elems);
            }
            if let Token::ScalarVar(name) = self.peek().clone() {
                self.advance();
                elems.push(MatchArrayElem::CaptureScalar(name));
                if self.eat(&Token::Comma) {
                    if matches!(self.peek(), Token::RBracket) {
                        break;
                    }
                    continue;
                }
                break;
            }
            let e = self.parse_assign_expr()?;
            elems.push(MatchArrayElem::Expr(e));
            if self.eat(&Token::Comma) {
                if matches!(self.peek(), Token::RBracket) {
                    break;
                }
                continue;
            }
            break;
        }
        self.expect(&Token::RBracket)?;
        Ok(elems)
    }

    fn parse_match_array_pattern(&mut self) -> PerlResult<MatchPattern> {
        self.expect(&Token::LBracket)?;
        let elems = self.parse_match_array_elems_until_rbracket()?;
        Ok(MatchPattern::Array(elems))
    }

    fn parse_match_hash_pattern(&mut self) -> PerlResult<MatchPattern> {
        self.expect(&Token::LBrace)?;
        let mut pairs = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            if self.eat(&Token::Semicolon) {
                continue;
            }
            let key = self.parse_assign_expr()?;
            self.expect(&Token::FatArrow)?;
            match self.advance().0 {
                Token::Ident(ref s) if s == "_" => {
                    pairs.push(MatchHashPair::KeyOnly { key });
                }
                Token::ScalarVar(name) => {
                    pairs.push(MatchHashPair::Capture { key, name });
                }
                tok => {
                    return Err(self.syntax_err(
                        format!(
                            "hash match pattern must bind with `=> $name` or `=> _`, got {:?}",
                            tok
                        ),
                        self.peek_line(),
                    ));
                }
            }
            self.eat(&Token::Comma);
        }
        self.expect(&Token::RBrace)?;
        Ok(MatchPattern::Hash(pairs))
    }

    /// `eval_timeout SECS { ... }`
    fn parse_eval_timeout(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance();
        let timeout = self.parse_postfix()?;
        let body = self.parse_block_or_bareword_block_no_args()?;
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::EvalTimeout { timeout, body },
            line,
        })
    }

    fn mark_match_scalar_g_for_boolean_condition(cond: &mut Expr) {
        match &mut cond.kind {
            ExprKind::Match {
                flags, scalar_g, ..
            } => {
                if flags.contains('g') {
                    *scalar_g = true;
                }
            }
            ExprKind::UnaryOp {
                op: UnaryOp::LogNot,
                expr,
            } => {
                if let ExprKind::Match {
                    flags, scalar_g, ..
                } = &mut expr.kind
                {
                    if flags.contains('g') {
                        *scalar_g = true;
                    }
                }
            }
            _ => {}
        }
    }

    fn parse_if(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'if'
        if matches!(self.peek(), Token::Ident(ref s) if s == "let") {
            return self.parse_if_let(line);
        }
        self.expect(&Token::LParen)?;
        let mut cond = self.parse_expression()?;
        Self::mark_match_scalar_g_for_boolean_condition(&mut cond);
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;

        let mut elsifs = Vec::new();
        let mut else_block = None;

        loop {
            if let Token::Ident(ref kw) = self.peek().clone() {
                if kw == "elsif" {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    let mut c = self.parse_expression()?;
                    Self::mark_match_scalar_g_for_boolean_condition(&mut c);
                    self.expect(&Token::RParen)?;
                    let b = self.parse_block()?;
                    elsifs.push((c, b));
                    continue;
                }
                if kw == "else" {
                    self.advance();
                    else_block = Some(self.parse_block()?);
                }
            }
            break;
        }

        Ok(Statement {
            label: None,
            kind: StmtKind::If {
                condition: cond,
                body,
                elsifs,
                else_block,
            },
            line,
        })
    }

    /// `if let PAT = EXPR { ... } [ else { ... } ]` — desugars to [`ExprKind::AlgebraicMatch`].
    fn parse_if_let(&mut self, line: usize) -> PerlResult<Statement> {
        self.advance(); // `let`
        let pattern = self.parse_match_pattern()?;
        self.expect(&Token::Assign)?;
        // Use assign-level parsing so a following `{ ... }` is the `if let` body, not an anon hash.
        self.suppress_scalar_hash_brace = self.suppress_scalar_hash_brace.saturating_add(1);
        let rhs = self.parse_assign_expr();
        self.suppress_scalar_hash_brace = self.suppress_scalar_hash_brace.saturating_sub(1);
        let rhs = rhs?;
        let then_block = self.parse_block()?;
        let else_block_opt = match self.peek().clone() {
            Token::Ident(ref kw) if kw == "else" => {
                self.advance();
                Some(self.parse_block()?)
            }
            Token::Ident(ref kw) if kw == "elsif" => {
                return Err(self.syntax_err(
                    "`if let` does not support `elsif`; use `else { }` or a full `match`",
                    self.peek_line(),
                ));
            }
            _ => None,
        };
        let then_expr = Self::expr_do_anon_block(then_block, line);
        let else_expr = if let Some(eb) = else_block_opt {
            Self::expr_do_anon_block(eb, line)
        } else {
            Expr {
                kind: ExprKind::Undef,
                line,
            }
        };
        let arms = vec![
            MatchArm {
                pattern,
                guard: None,
                body: then_expr,
            },
            MatchArm {
                pattern: MatchPattern::Any,
                guard: None,
                body: else_expr,
            },
        ];
        Ok(Statement {
            label: None,
            kind: StmtKind::Expression(Expr {
                kind: ExprKind::AlgebraicMatch {
                    subject: Box::new(rhs),
                    arms,
                },
                line,
            }),
            line,
        })
    }

    fn expr_do_anon_block(block: Block, outer_line: usize) -> Expr {
        let inner_line = block.first().map(|s| s.line).unwrap_or(outer_line);
        Expr {
            kind: ExprKind::Do(Box::new(Expr {
                kind: ExprKind::CodeRef {
                    params: vec![],
                    body: block,
                },
                line: inner_line,
            })),
            line: outer_line,
        }
    }

    fn parse_unless(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'unless'
        self.expect(&Token::LParen)?;
        let mut cond = self.parse_expression()?;
        Self::mark_match_scalar_g_for_boolean_condition(&mut cond);
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        let else_block = if let Token::Ident(ref kw) = self.peek().clone() {
            if kw == "else" {
                self.advance();
                Some(self.parse_block()?)
            } else {
                None
            }
        } else {
            None
        };
        Ok(Statement {
            label: None,
            kind: StmtKind::Unless {
                condition: cond,
                body,
                else_block,
            },
            line,
        })
    }

    fn parse_while(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'while'
        if matches!(self.peek(), Token::Ident(ref s) if s == "let") {
            return self.parse_while_let(line);
        }
        self.expect(&Token::LParen)?;
        let mut cond = self.parse_expression()?;
        Self::mark_match_scalar_g_for_boolean_condition(&mut cond);
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        let continue_block = self.parse_optional_continue_block()?;
        Ok(Statement {
            label: None,
            kind: StmtKind::While {
                condition: cond,
                body,
                label: None,
                continue_block,
            },
            line,
        })
    }

    /// `while let PAT = EXPR { ... }` — desugars to a `match` that returns 0/1 plus `unless ($tmp) { last }`
    /// so bytecode does not run `last` inside a tree-assisted [`Op::AlgebraicMatch`] arm.
    fn parse_while_let(&mut self, line: usize) -> PerlResult<Statement> {
        self.advance(); // `let`
        let pattern = self.parse_match_pattern()?;
        self.expect(&Token::Assign)?;
        self.suppress_scalar_hash_brace = self.suppress_scalar_hash_brace.saturating_add(1);
        let rhs = self.parse_assign_expr();
        self.suppress_scalar_hash_brace = self.suppress_scalar_hash_brace.saturating_sub(1);
        let rhs = rhs?;
        let mut user_body = self.parse_block()?;
        let continue_block = self.parse_optional_continue_block()?;
        user_body.push(Statement::new(
            StmtKind::Expression(Expr {
                kind: ExprKind::Integer(1),
                line,
            }),
            line,
        ));
        let tmp = format!("__while_let_{}", self.alloc_desugar_tmp());
        let match_expr = Expr {
            kind: ExprKind::AlgebraicMatch {
                subject: Box::new(rhs),
                arms: vec![
                    MatchArm {
                        pattern,
                        guard: None,
                        body: Self::expr_do_anon_block(user_body, line),
                    },
                    MatchArm {
                        pattern: MatchPattern::Any,
                        guard: None,
                        body: Expr {
                            kind: ExprKind::Integer(0),
                            line,
                        },
                    },
                ],
            },
            line,
        };
        let my_stmt = Statement::new(
            StmtKind::My(vec![VarDecl {
                sigil: Sigil::Scalar,
                name: tmp.clone(),
                initializer: Some(match_expr),
                frozen: false,
                type_annotation: None,
            }]),
            line,
        );
        let unless_last = Statement::new(
            StmtKind::Unless {
                condition: Expr {
                    kind: ExprKind::ScalarVar(tmp),
                    line,
                },
                body: vec![Statement::new(StmtKind::Last(None), line)],
                else_block: None,
            },
            line,
        );
        Ok(Statement::new(
            StmtKind::While {
                condition: Expr {
                    kind: ExprKind::Integer(1),
                    line,
                },
                body: vec![my_stmt, unless_last],
                label: None,
                continue_block,
            },
            line,
        ))
    }

    fn parse_until(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'until'
        self.expect(&Token::LParen)?;
        let mut cond = self.parse_expression()?;
        Self::mark_match_scalar_g_for_boolean_condition(&mut cond);
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        let continue_block = self.parse_optional_continue_block()?;
        Ok(Statement {
            label: None,
            kind: StmtKind::Until {
                condition: cond,
                body,
                label: None,
                continue_block,
            },
            line,
        })
    }

    /// `continue { ... }` after a loop body (optional).
    fn parse_optional_continue_block(&mut self) -> PerlResult<Option<Block>> {
        if let Token::Ident(ref kw) = self.peek().clone() {
            if kw == "continue" {
                self.advance();
                return Ok(Some(self.parse_block()?));
            }
        }
        Ok(None)
    }

    fn parse_for_or_foreach(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'for'

        // Peek to determine if C-style for or foreach
        // C-style: for (init; cond; step)
        // foreach-style: for $var (list) or for (list)
        match self.peek() {
            Token::LParen => {
                // Check if next after ( is a semicolon or an assignment — C-style
                // Or if it's a list — foreach-style
                // Heuristic: if the token after ( is 'my' or '$' followed by
                // content that contains ';', it's C-style.
                let saved = self.pos;
                self.advance(); // consume (
                                // Look for semicolon at paren depth 0
                let mut depth = 1;
                let mut has_semi = false;
                let mut scan = self.pos;
                while scan < self.tokens.len() {
                    match &self.tokens[scan].0 {
                        Token::LParen => depth += 1,
                        Token::RParen => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Token::Semicolon if depth == 1 => {
                            has_semi = true;
                            break;
                        }
                        _ => {}
                    }
                    scan += 1;
                }
                self.pos = saved;

                if has_semi {
                    self.parse_c_style_for(line)
                } else {
                    // foreach without explicit var — uses $_
                    self.expect(&Token::LParen)?;
                    let list = self.parse_expression()?;
                    self.expect(&Token::RParen)?;
                    let body = self.parse_block()?;
                    let continue_block = self.parse_optional_continue_block()?;
                    Ok(Statement {
                        label: None,
                        kind: StmtKind::Foreach {
                            var: "_".to_string(),
                            list,
                            body,
                            label: None,
                            continue_block,
                        },
                        line,
                    })
                }
            }
            Token::Ident(ref kw) if kw == "my" => {
                self.advance(); // 'my'
                let var = self.parse_scalar_var_name()?;
                self.expect(&Token::LParen)?;
                let list = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                let body = self.parse_block()?;
                let continue_block = self.parse_optional_continue_block()?;
                Ok(Statement {
                    label: None,
                    kind: StmtKind::Foreach {
                        var,
                        list,
                        body,
                        label: None,
                        continue_block,
                    },
                    line,
                })
            }
            Token::ScalarVar(_) => {
                let var = self.parse_scalar_var_name()?;
                self.expect(&Token::LParen)?;
                let list = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                let body = self.parse_block()?;
                let continue_block = self.parse_optional_continue_block()?;
                Ok(Statement {
                    label: None,
                    kind: StmtKind::Foreach {
                        var,
                        list,
                        body,
                        label: None,
                        continue_block,
                    },
                    line,
                })
            }
            _ => self.parse_c_style_for(line),
        }
    }

    fn parse_c_style_for(&mut self, line: usize) -> PerlResult<Statement> {
        self.expect(&Token::LParen)?;
        let init = if self.eat(&Token::Semicolon) {
            None
        } else {
            let s = self.parse_statement()?;
            self.eat(&Token::Semicolon);
            Some(Box::new(s))
        };
        let mut condition = if matches!(self.peek(), Token::Semicolon) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        if let Some(ref mut c) = condition {
            Self::mark_match_scalar_g_for_boolean_condition(c);
        }
        self.expect(&Token::Semicolon)?;
        let step = if matches!(self.peek(), Token::RParen) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        let continue_block = self.parse_optional_continue_block()?;
        Ok(Statement {
            label: None,
            kind: StmtKind::For {
                init,
                condition,
                step,
                body,
                label: None,
                continue_block,
            },
            line,
        })
    }

    fn parse_foreach(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'foreach'
        let var = match self.peek() {
            Token::Ident(ref kw) if kw == "my" => {
                self.advance();
                self.parse_scalar_var_name()?
            }
            Token::ScalarVar(_) => self.parse_scalar_var_name()?,
            _ => "_".to_string(),
        };
        self.expect(&Token::LParen)?;
        let list = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        let continue_block = self.parse_optional_continue_block()?;
        Ok(Statement {
            label: None,
            kind: StmtKind::Foreach {
                var,
                list,
                body,
                label: None,
                continue_block,
            },
            line,
        })
    }

    fn parse_scalar_var_name(&mut self) -> PerlResult<String> {
        match self.advance() {
            (Token::ScalarVar(name), _) => Ok(name),
            (tok, line) => {
                Err(self.syntax_err(format!("Expected scalar variable, got {:?}", tok), line))
            }
        }
    }

    /// After `(` was consumed: Perl5 prototype characters until `)` (or `$)` + `{`).
    fn parse_legacy_sub_prototype_tail(&mut self) -> PerlResult<String> {
        let mut s = String::new();
        loop {
            match self.peek().clone() {
                Token::RParen => {
                    self.advance();
                    break;
                }
                Token::Eof => {
                    return Err(self.syntax_err(
                        "Unterminated sub prototype (expected ')' before end of input)",
                        self.peek_line(),
                    ));
                }
                Token::ScalarVar(v) if v == ")" => {
                    // Lexer merges `$` + `)` into one token (`$)`). In `sub name ($) {`, the
                    // closing `)` of the prototype is not a separate `RParen` — next is `{`.
                    self.advance();
                    s.push('$');
                    if matches!(self.peek(), Token::LBrace) {
                        break;
                    }
                }
                Token::Ident(i) => {
                    let i = i.clone();
                    self.advance();
                    s.push_str(&i);
                }
                Token::Semicolon => {
                    self.advance();
                    s.push(';');
                }
                Token::LParen => {
                    self.advance();
                    s.push('(');
                }
                Token::LBracket => {
                    self.advance();
                    s.push('[');
                }
                Token::RBracket => {
                    self.advance();
                    s.push(']');
                }
                Token::Backslash => {
                    self.advance();
                    s.push('\\');
                }
                Token::Comma => {
                    self.advance();
                    s.push(',');
                }
                Token::ScalarVar(v) => {
                    let v = v.clone();
                    self.advance();
                    s.push('$');
                    s.push_str(&v);
                }
                Token::ArrayVar(v) => {
                    let v = v.clone();
                    self.advance();
                    s.push('@');
                    s.push_str(&v);
                }
                // Bare `@` / `%` in prototypes (e.g. Try::Tiny's `sub try (&;@)`).
                Token::ArrayAt => {
                    self.advance();
                    s.push('@');
                }
                Token::HashVar(v) => {
                    let v = v.clone();
                    self.advance();
                    s.push('%');
                    s.push_str(&v);
                }
                Token::HashPercent => {
                    self.advance();
                    s.push('%');
                }
                Token::Plus => {
                    self.advance();
                    s.push('+');
                }
                Token::Minus => {
                    self.advance();
                    s.push('-');
                }
                Token::BitAnd => {
                    self.advance();
                    s.push('&');
                }
                tok => {
                    return Err(self.syntax_err(
                        format!("Unexpected token in sub prototype: {:?}", tok),
                        self.peek_line(),
                    ));
                }
            }
        }
        Ok(s)
    }

    fn sub_signature_list_starts_here(&self) -> bool {
        match self.peek() {
            Token::LBrace | Token::LBracket => true,
            Token::ScalarVar(name) if name != "$$" && name != ")" => true,
            _ => false,
        }
    }

    fn parse_sub_signature_hash_key(&mut self) -> PerlResult<String> {
        let (tok, line) = self.advance();
        match tok {
            Token::Ident(i) => Ok(i),
            Token::SingleString(s) | Token::DoubleString(s) => Ok(s),
            tok => Err(self.syntax_err(
                format!(
                    "sub signature: expected hash key (identifier or string), got {:?}",
                    tok
                ),
                line,
            )),
        }
    }

    fn parse_sub_signature_param_list(&mut self) -> PerlResult<Vec<SubSigParam>> {
        let mut params = Vec::new();
        loop {
            if matches!(self.peek(), Token::RParen) {
                break;
            }
            match self.peek().clone() {
                Token::ScalarVar(name) => {
                    if name == "$$" || name == ")" {
                        return Err(self.syntax_err(
                            format!(
                                "`{name}` cannot start a perlrs sub signature (use legacy prototype `($$)` etc.)"
                            ),
                            self.peek_line(),
                        ));
                    }
                    self.advance();
                    params.push(SubSigParam::Scalar(name));
                }
                Token::LBracket => {
                    self.advance();
                    let elems = self.parse_match_array_elems_until_rbracket()?;
                    params.push(SubSigParam::ArrayDestruct(elems));
                }
                Token::LBrace => {
                    self.advance();
                    let mut pairs = Vec::new();
                    loop {
                        if matches!(self.peek(), Token::RBrace | Token::Eof) {
                            break;
                        }
                        if self.eat(&Token::Comma) {
                            continue;
                        }
                        let key = self.parse_sub_signature_hash_key()?;
                        self.expect(&Token::FatArrow)?;
                        let bind = self.parse_scalar_var_name()?;
                        pairs.push((key, bind));
                        self.eat(&Token::Comma);
                    }
                    self.expect(&Token::RBrace)?;
                    params.push(SubSigParam::HashDestruct(pairs));
                }
                tok => {
                    return Err(self.syntax_err(
                        format!(
                            "expected `$name`, `[ ... ]`, or `{{ ... }}` in sub signature, got {:?}",
                            tok
                        ),
                        self.peek_line(),
                    ));
                }
            }
            match self.peek() {
                Token::Comma => {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        return Err(self.syntax_err(
                            "trailing `,` before `)` in sub signature",
                            self.peek_line(),
                        ));
                    }
                }
                Token::RParen => break,
                _ => {
                    return Err(self.syntax_err(
                        format!(
                            "expected `,` or `)` after sub signature parameter, got {:?}",
                            self.peek()
                        ),
                        self.peek_line(),
                    ));
                }
            }
        }
        Ok(params)
    }

    /// Optional `sub` parens: either a Perl 5 prototype string or a perlrs **`$name` / `{ k => $v }`** signature.
    fn parse_sub_sig_or_prototype_opt(&mut self) -> PerlResult<(Vec<SubSigParam>, Option<String>)> {
        if !matches!(self.peek(), Token::LParen) {
            return Ok((vec![], None));
        }
        self.advance();
        if matches!(self.peek(), Token::RParen) {
            self.advance();
            return Ok((vec![], Some(String::new())));
        }
        if self.sub_signature_list_starts_here() {
            let params = self.parse_sub_signature_param_list()?;
            self.expect(&Token::RParen)?;
            return Ok((params, None));
        }
        let proto = self.parse_legacy_sub_prototype_tail()?;
        Ok((vec![], Some(proto)))
    }

    /// Optional subroutine attributes after name/prototype: `sub foo : lvalue { }`, `sub : ATTR(ARGS) { }`.
    fn parse_sub_attributes(&mut self) -> PerlResult<()> {
        while self.eat(&Token::Colon) {
            match self.advance() {
                (Token::Ident(_), _) => {}
                (tok, line) => {
                    return Err(self.syntax_err(
                        format!("Expected attribute name after `:`, got {:?}", tok),
                        line,
                    ));
                }
            }
            if self.eat(&Token::LParen) {
                let mut depth = 1usize;
                while depth > 0 {
                    match self.advance().0 {
                        Token::LParen => depth += 1,
                        Token::RParen => {
                            depth -= 1;
                        }
                        Token::Eof => {
                            return Err(self.syntax_err(
                                "Unterminated sub attribute argument list",
                                self.peek_line(),
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    fn parse_sub_decl(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'sub'
        match self.peek().clone() {
            Token::Ident(_) => {
                let name = self.parse_package_qualified_identifier()?;
                let (params, prototype) = self.parse_sub_sig_or_prototype_opt()?;
                self.parse_sub_attributes()?;
                let body = self.parse_block()?;
                Ok(Statement {
                    label: None,
                    kind: StmtKind::SubDecl {
                        name,
                        params,
                        body,
                        prototype,
                    },
                    line,
                })
            }
            Token::LParen | Token::LBrace | Token::Colon => {
                // Statement-level anonymous sub: `sub { }`, `sub () { }`, `sub :lvalue { }`
                let (params, _prototype) = self.parse_sub_sig_or_prototype_opt()?;
                self.parse_sub_attributes()?;
                let body = self.parse_block()?;
                Ok(Statement {
                    label: None,
                    kind: StmtKind::Expression(Expr {
                        kind: ExprKind::CodeRef { params, body },
                        line,
                    }),
                    line,
                })
            }
            tok => Err(self.syntax_err(
                format!("Expected sub name, `(`, `{{`, or `:`, got {:?}", tok),
                self.peek_line(),
            )),
        }
    }

    /// `struct Name { field => Type, ... }`
    fn parse_struct_decl(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // struct
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, err_line) => {
                return Err(
                    self.syntax_err(format!("Expected struct name, got {:?}", tok), err_line)
                )
            }
        };
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let field_name = match self.advance() {
                (Token::Ident(n), _) => n,
                (tok, err_line) => {
                    return Err(
                        self.syntax_err(format!("Expected field name, got {:?}", tok), err_line)
                    )
                }
            };
            self.expect(&Token::FatArrow)?;
            let ty = self.parse_type_name()?;
            fields.push((field_name, ty));
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::StructDecl {
                def: StructDef { name, fields },
            },
            line,
        })
    }

    fn local_simple_target_to_var_decl(target: &Expr) -> Option<VarDecl> {
        match &target.kind {
            ExprKind::ScalarVar(name) => Some(VarDecl {
                sigil: Sigil::Scalar,
                name: name.clone(),
                initializer: None,
                frozen: false,
                type_annotation: None,
            }),
            ExprKind::ArrayVar(name) => Some(VarDecl {
                sigil: Sigil::Array,
                name: name.clone(),
                initializer: None,
                frozen: false,
                type_annotation: None,
            }),
            ExprKind::HashVar(name) => Some(VarDecl {
                sigil: Sigil::Hash,
                name: name.clone(),
                initializer: None,
                frozen: false,
                type_annotation: None,
            }),
            ExprKind::Typeglob(name) => Some(VarDecl {
                sigil: Sigil::Typeglob,
                name: name.clone(),
                initializer: None,
                frozen: false,
                type_annotation: None,
            }),
            _ => None,
        }
    }

    fn parse_decl_array_destructure(
        &mut self,
        keyword: &str,
        line: usize,
    ) -> PerlResult<Statement> {
        self.expect(&Token::LBracket)?;
        let elems = self.parse_match_array_elems_until_rbracket()?;
        self.expect(&Token::Assign)?;
        self.suppress_scalar_hash_brace += 1;
        let rhs = self.parse_expression()?;
        self.suppress_scalar_hash_brace -= 1;
        let stmt = self.desugar_array_destructure(keyword, line, elems, rhs)?;
        self.parse_stmt_postfix_modifier(stmt)
    }

    fn parse_decl_hash_destructure(&mut self, keyword: &str, line: usize) -> PerlResult<Statement> {
        let MatchPattern::Hash(pairs) = self.parse_match_hash_pattern()? else {
            unreachable!("parse_match_hash_pattern returns Hash");
        };
        self.expect(&Token::Assign)?;
        self.suppress_scalar_hash_brace += 1;
        let rhs = self.parse_expression()?;
        self.suppress_scalar_hash_brace -= 1;
        let stmt = self.desugar_hash_destructure(keyword, line, pairs, rhs)?;
        self.parse_stmt_postfix_modifier(stmt)
    }

    fn desugar_array_destructure(
        &mut self,
        keyword: &str,
        line: usize,
        elems: Vec<MatchArrayElem>,
        rhs: Expr,
    ) -> PerlResult<Statement> {
        let tmp = format!("__perlrs_ds_{}", self.alloc_desugar_tmp());
        let mut stmts: Vec<Statement> = Vec::new();
        stmts.push(destructure_stmt_from_var_decls(
            keyword,
            vec![VarDecl {
                sigil: Sigil::Scalar,
                name: tmp.clone(),
                initializer: Some(rhs),
                frozen: false,
                type_annotation: None,
            }],
            line,
        ));

        let has_rest = elems
            .iter()
            .any(|e| matches!(e, MatchArrayElem::Rest | MatchArrayElem::RestBind(_)));
        let fixed_slots = elems
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    MatchArrayElem::CaptureScalar(_) | MatchArrayElem::Expr(_)
                )
            })
            .count();
        if !has_rest {
            let cond = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(destructure_expr_array_len(&tmp, line)),
                    op: BinOp::NumEq,
                    right: Box::new(Expr {
                        kind: ExprKind::Integer(fixed_slots as i64),
                        line,
                    }),
                },
                line,
            };
            stmts.push(destructure_stmt_unless_die(
                line,
                cond,
                "array destructure: length mismatch",
            ));
        }

        let mut idx: i64 = 0;
        for elem in elems {
            match elem {
                MatchArrayElem::Rest => break,
                MatchArrayElem::RestBind(name) => {
                    let list_source = Expr {
                        kind: ExprKind::Deref {
                            expr: Box::new(destructure_expr_scalar_tmp(&tmp, line)),
                            kind: Sigil::Array,
                        },
                        line,
                    };
                    let last_ix = Expr {
                        kind: ExprKind::BinOp {
                            left: Box::new(destructure_expr_array_len(&tmp, line)),
                            op: BinOp::Sub,
                            right: Box::new(Expr {
                                kind: ExprKind::Integer(1),
                                line,
                            }),
                        },
                        line,
                    };
                    let range = Expr {
                        kind: ExprKind::Range {
                            from: Box::new(Expr {
                                kind: ExprKind::Integer(idx),
                                line,
                            }),
                            to: Box::new(last_ix),
                            exclusive: false,
                        },
                        line,
                    };
                    let slice = Expr {
                        kind: ExprKind::AnonymousListSlice {
                            source: Box::new(list_source),
                            indices: vec![range],
                        },
                        line,
                    };
                    stmts.push(destructure_stmt_from_var_decls(
                        keyword,
                        vec![VarDecl {
                            sigil: Sigil::Array,
                            name,
                            initializer: Some(slice),
                            frozen: false,
                            type_annotation: None,
                        }],
                        line,
                    ));
                    break;
                }
                MatchArrayElem::CaptureScalar(name) => {
                    let arrow = Expr {
                        kind: ExprKind::ArrowDeref {
                            expr: Box::new(destructure_expr_scalar_tmp(&tmp, line)),
                            index: Box::new(Expr {
                                kind: ExprKind::Integer(idx),
                                line,
                            }),
                            kind: DerefKind::Array,
                        },
                        line,
                    };
                    stmts.push(destructure_stmt_from_var_decls(
                        keyword,
                        vec![VarDecl {
                            sigil: Sigil::Scalar,
                            name,
                            initializer: Some(arrow),
                            frozen: false,
                            type_annotation: None,
                        }],
                        line,
                    ));
                    idx += 1;
                }
                MatchArrayElem::Expr(e) => {
                    let elem_subj = Expr {
                        kind: ExprKind::ArrowDeref {
                            expr: Box::new(destructure_expr_scalar_tmp(&tmp, line)),
                            index: Box::new(Expr {
                                kind: ExprKind::Integer(idx),
                                line,
                            }),
                            kind: DerefKind::Array,
                        },
                        line,
                    };
                    let match_expr = Expr {
                        kind: ExprKind::AlgebraicMatch {
                            subject: Box::new(elem_subj),
                            arms: vec![
                                MatchArm {
                                    pattern: MatchPattern::Value(Box::new(e.clone())),
                                    guard: None,
                                    body: Expr {
                                        kind: ExprKind::Integer(0),
                                        line,
                                    },
                                },
                                MatchArm {
                                    pattern: MatchPattern::Any,
                                    guard: None,
                                    body: Expr {
                                        kind: ExprKind::Die(vec![Expr {
                                            kind: ExprKind::String(
                                                "array destructure: element pattern mismatch"
                                                    .to_string(),
                                            ),
                                            line,
                                        }]),
                                        line,
                                    },
                                },
                            ],
                        },
                        line,
                    };
                    stmts.push(Statement {
                        label: None,
                        kind: StmtKind::Expression(match_expr),
                        line,
                    });
                    idx += 1;
                }
            }
        }

        Ok(Statement {
            label: None,
            kind: StmtKind::StmtGroup(stmts),
            line,
        })
    }

    fn desugar_hash_destructure(
        &mut self,
        keyword: &str,
        line: usize,
        pairs: Vec<MatchHashPair>,
        rhs: Expr,
    ) -> PerlResult<Statement> {
        let tmp = format!("__perlrs_ds_{}", self.alloc_desugar_tmp());
        let mut stmts: Vec<Statement> = Vec::new();
        stmts.push(destructure_stmt_from_var_decls(
            keyword,
            vec![VarDecl {
                sigil: Sigil::Scalar,
                name: tmp.clone(),
                initializer: Some(rhs),
                frozen: false,
                type_annotation: None,
            }],
            line,
        ));

        for pair in pairs {
            match pair {
                MatchHashPair::KeyOnly { key } => {
                    let exists_op = Expr {
                        kind: ExprKind::Exists(Box::new(Expr {
                            kind: ExprKind::ArrowDeref {
                                expr: Box::new(destructure_expr_scalar_tmp(&tmp, line)),
                                index: Box::new(key),
                                kind: DerefKind::Hash,
                            },
                            line,
                        })),
                        line,
                    };
                    stmts.push(destructure_stmt_unless_die(
                        line,
                        exists_op,
                        "hash destructure: missing required key",
                    ));
                }
                MatchHashPair::Capture { key, name } => {
                    let init = Expr {
                        kind: ExprKind::ArrowDeref {
                            expr: Box::new(destructure_expr_scalar_tmp(&tmp, line)),
                            index: Box::new(key),
                            kind: DerefKind::Hash,
                        },
                        line,
                    };
                    stmts.push(destructure_stmt_from_var_decls(
                        keyword,
                        vec![VarDecl {
                            sigil: Sigil::Scalar,
                            name,
                            initializer: Some(init),
                            frozen: false,
                            type_annotation: None,
                        }],
                        line,
                    ));
                }
            }
        }

        Ok(Statement {
            label: None,
            kind: StmtKind::StmtGroup(stmts),
            line,
        })
    }

    fn parse_my_our_local(
        &mut self,
        keyword: &str,
        allow_type_annotation: bool,
    ) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'my'/'our'/'local'

        if keyword == "local"
            && !matches!(self.peek(), Token::LParen | Token::LBracket | Token::LBrace)
        {
            let target = self.parse_postfix()?;
            let mut initializer: Option<Expr> = None;
            if self.eat(&Token::Assign) {
                initializer = Some(self.parse_expression()?);
            } else if matches!(
                self.peek(),
                Token::OrAssign | Token::DefinedOrAssign | Token::AndAssign
            ) {
                if matches!(&target.kind, ExprKind::Typeglob(_)) {
                    return Err(self.syntax_err(
                        "compound assignment on typeglob declaration is not supported",
                        self.peek_line(),
                    ));
                }
                let op = match self.peek().clone() {
                    Token::OrAssign => BinOp::LogOr,
                    Token::DefinedOrAssign => BinOp::DefinedOr,
                    Token::AndAssign => BinOp::LogAnd,
                    _ => unreachable!(),
                };
                self.advance();
                let rhs = self.parse_assign_expr()?;
                let tgt_line = target.line;
                initializer = Some(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(target.clone()),
                        op,
                        value: Box::new(rhs),
                    },
                    line: tgt_line,
                });
            }

            let kind = if let Some(mut decl) = Self::local_simple_target_to_var_decl(&target) {
                decl.initializer = initializer;
                StmtKind::Local(vec![decl])
            } else {
                StmtKind::LocalExpr {
                    target,
                    initializer,
                }
            };
            let stmt = Statement {
                label: None,
                kind,
                line,
            };
            return self.parse_stmt_postfix_modifier(stmt);
        }

        if matches!(self.peek(), Token::LBracket) {
            return self.parse_decl_array_destructure(keyword, line);
        }
        if matches!(self.peek(), Token::LBrace) {
            return self.parse_decl_hash_destructure(keyword, line);
        }

        let mut decls = Vec::new();

        if self.eat(&Token::LParen) {
            // my ($a, @b, %c)
            while !matches!(self.peek(), Token::RParen | Token::Eof) {
                let decl = self.parse_var_decl(allow_type_annotation)?;
                decls.push(decl);
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
        } else {
            decls.push(self.parse_var_decl(allow_type_annotation)?);
        }

        // Optional initializer: my $x = expr — plus `our @EXPORT = our @EXPORT_OK = qw(...)` (Try::Tiny).
        if self.eat(&Token::Assign) {
            if keyword == "our" && decls.len() == 1 {
                while matches!(self.peek(), Token::Ident(ref i) if i == "our") {
                    self.advance();
                    decls.push(self.parse_var_decl(allow_type_annotation)?);
                    if !self.eat(&Token::Assign) {
                        return Err(self.syntax_err(
                            "expected `=` after `our` in chained our-declaration",
                            self.peek_line(),
                        ));
                    }
                }
            }
            let val = self.parse_expression()?;
            if decls.len() == 1 {
                decls[0].initializer = Some(val);
            } else {
                for decl in &mut decls {
                    decl.initializer = Some(val.clone());
                }
            }
        } else if decls.len() == 1 {
            // `our $Verbose ||= 0` (Exporter.pm) — compound assign on a single decl
            let op = match self.peek().clone() {
                Token::OrAssign => Some(BinOp::LogOr),
                Token::DefinedOrAssign => Some(BinOp::DefinedOr),
                Token::AndAssign => Some(BinOp::LogAnd),
                _ => None,
            };
            if let Some(op) = op {
                let d = &decls[0];
                if matches!(d.sigil, Sigil::Typeglob) {
                    return Err(self.syntax_err(
                        "compound assignment on typeglob declaration is not supported",
                        self.peek_line(),
                    ));
                }
                self.advance();
                let rhs = self.parse_assign_expr()?;
                let target = Expr {
                    kind: match d.sigil {
                        Sigil::Scalar => ExprKind::ScalarVar(d.name.clone()),
                        Sigil::Array => ExprKind::ArrayVar(d.name.clone()),
                        Sigil::Hash => ExprKind::HashVar(d.name.clone()),
                        Sigil::Typeglob => unreachable!(),
                    },
                    line,
                };
                decls[0].initializer = Some(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(target),
                        op,
                        value: Box::new(rhs),
                    },
                    line,
                });
            }
        }

        let kind = match keyword {
            "my" => StmtKind::My(decls),
            "mysync" => StmtKind::MySync(decls),
            "our" => StmtKind::Our(decls),
            "local" => StmtKind::Local(decls),
            "state" => StmtKind::State(decls),
            _ => unreachable!(),
        };
        let stmt = Statement {
            label: None,
            kind,
            line,
        };
        // `my $x = 1 if $y;` — statement modifier applies to the whole declaration (Perl).
        self.parse_stmt_postfix_modifier(stmt)
    }

    fn parse_var_decl(&mut self, allow_type_annotation: bool) -> PerlResult<VarDecl> {
        let mut decl = match self.advance() {
            (Token::ScalarVar(name), _) => VarDecl {
                sigil: Sigil::Scalar,
                name,
                initializer: None,
                frozen: false,
                type_annotation: None,
            },
            (Token::ArrayVar(name), _) => VarDecl {
                sigil: Sigil::Array,
                name,
                initializer: None,
                frozen: false,
                type_annotation: None,
            },
            (Token::HashVar(name), _) => VarDecl {
                sigil: Sigil::Hash,
                name,
                initializer: None,
                frozen: false,
                type_annotation: None,
            },
            (Token::Star, _line) => {
                let name = match self.advance() {
                    (Token::Ident(n), _) => n,
                    (tok, l) => {
                        return Err(self
                            .syntax_err(format!("Expected identifier after *, got {:?}", tok), l));
                    }
                };
                VarDecl {
                    sigil: Sigil::Typeglob,
                    name,
                    initializer: None,
                    frozen: false,
                    type_annotation: None,
                }
            }
            (tok, line) => {
                return Err(self.syntax_err(
                    format!("Expected variable in declaration, got {:?}", tok),
                    line,
                ));
            }
        };
        if allow_type_annotation && self.eat(&Token::Colon) {
            let ty = self.parse_type_name()?;
            if decl.sigil != Sigil::Scalar {
                return Err(self.syntax_err(
                    "`: Type` is only valid for scalar declarations (typed my $name : Int)",
                    self.peek_line(),
                ));
            }
            decl.type_annotation = Some(ty);
        }
        Ok(decl)
    }

    fn parse_type_name(&mut self) -> PerlResult<PerlTypeName> {
        let line = self.peek_line();
        match self.advance() {
            (Token::Ident(name), _) => match name.as_str() {
                "Int" => Ok(PerlTypeName::Int),
                "Str" => Ok(PerlTypeName::Str),
                "Float" => Ok(PerlTypeName::Float),
                _ => Err(self.syntax_err(
                    format!("unknown type `{name}` (supported: Int, Str, Float)"),
                    line,
                )),
            },
            (tok, line) => {
                Err(self.syntax_err(format!("Expected type name after `:`, got {:?}", tok), line))
            }
        }
    }

    fn parse_package(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'package'
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => {
                return Err(self.syntax_err(format!("Expected package name, got {:?}", tok), line))
            }
        };
        // Handle Foo::Bar
        let mut full_name = name;
        while self.eat(&Token::PackageSep) {
            if let (Token::Ident(part), _) = self.advance() {
                full_name = format!("{}::{}", full_name, part);
            }
        }
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::Package { name: full_name },
            line,
        })
    }

    fn parse_use(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'use'
        let (tok, tok_line) = self.advance();
        match tok {
            Token::Float(v) => {
                self.eat(&Token::Semicolon);
                Ok(Statement {
                    label: None,
                    kind: StmtKind::UsePerlVersion { version: v },
                    line,
                })
            }
            Token::Integer(n) => {
                if matches!(self.peek(), Token::Semicolon | Token::Eof) {
                    self.eat(&Token::Semicolon);
                    Ok(Statement {
                        label: None,
                        kind: StmtKind::UsePerlVersion { version: n as f64 },
                        line,
                    })
                } else {
                    Err(self.syntax_err(
                        format!("Expected ';' after use VERSION (got {:?})", self.peek()),
                        line,
                    ))
                }
            }
            Token::Ident(n) => {
                let mut full_name = n;
                while self.eat(&Token::PackageSep) {
                    if let (Token::Ident(part), _) = self.advance() {
                        full_name = format!("{}::{}", full_name, part);
                    }
                }
                if full_name == "overload" {
                    let mut pairs = Vec::new();
                    let mut parse_overload_pairs = |this: &mut Self| -> PerlResult<()> {
                        loop {
                            if matches!(this.peek(), Token::RParen | Token::Semicolon | Token::Eof)
                            {
                                break;
                            }
                            let key_e = this.parse_assign_expr()?;
                            this.expect(&Token::FatArrow)?;
                            let val_e = this.parse_assign_expr()?;
                            let key = this.expr_to_overload_key(&key_e)?;
                            let val = this.expr_to_overload_sub(&val_e)?;
                            pairs.push((key, val));
                            if !this.eat(&Token::Comma) {
                                break;
                            }
                        }
                        Ok(())
                    };
                    if self.eat(&Token::LParen) {
                        // `use overload ();` — common in JSON::PP and other modules.
                        parse_overload_pairs(self)?;
                        self.expect(&Token::RParen)?;
                    } else if !matches!(self.peek(), Token::Semicolon | Token::Eof) {
                        parse_overload_pairs(self)?;
                    }
                    self.eat(&Token::Semicolon);
                    return Ok(Statement {
                        label: None,
                        kind: StmtKind::UseOverload { pairs },
                        line,
                    });
                }
                let mut imports = Vec::new();
                if !matches!(self.peek(), Token::Semicolon | Token::Eof) {
                    loop {
                        if matches!(self.peek(), Token::Semicolon | Token::Eof) {
                            break;
                        }
                        imports.push(self.parse_expression()?);
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                }
                self.eat(&Token::Semicolon);
                Ok(Statement {
                    label: None,
                    kind: StmtKind::Use {
                        module: full_name,
                        imports,
                    },
                    line,
                })
            }
            other => Err(self.syntax_err(
                format!("Expected module name or version after use, got {:?}", other),
                tok_line,
            )),
        }
    }

    fn parse_no(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'no'
        let module = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => {
                return Err(self.syntax_err(
                    format!("Expected module name after no, got {:?}", tok),
                    line,
                ))
            }
        };
        let mut full_name = module;
        while self.eat(&Token::PackageSep) {
            if let (Token::Ident(part), _) = self.advance() {
                full_name = format!("{}::{}", full_name, part);
            }
        }
        let mut imports = Vec::new();
        if !matches!(self.peek(), Token::Semicolon | Token::Eof) {
            loop {
                if matches!(self.peek(), Token::Semicolon | Token::Eof) {
                    break;
                }
                imports.push(self.parse_expression()?);
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
        }
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::No {
                module: full_name,
                imports,
            },
            line,
        })
    }

    fn parse_return(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'return'
        let val = if matches!(self.peek(), Token::Semicolon | Token::RBrace | Token::Eof) {
            None
        } else {
            // Only parse up to the assign level to avoid consuming postfix if/unless
            Some(self.parse_assign_expr()?)
        };
        // Check for postfix modifiers on return
        let stmt = Statement {
            label: None,
            kind: StmtKind::Return(val),
            line,
        };
        if let Token::Ident(ref kw) = self.peek().clone() {
            match kw.as_str() {
                "if" => {
                    self.advance();
                    let cond = self.parse_expression()?;
                    self.eat(&Token::Semicolon);
                    return Ok(Statement {
                        label: None,
                        kind: StmtKind::If {
                            condition: cond,
                            body: vec![stmt],
                            elsifs: vec![],
                            else_block: None,
                        },
                        line,
                    });
                }
                "unless" => {
                    self.advance();
                    let cond = self.parse_expression()?;
                    self.eat(&Token::Semicolon);
                    return Ok(Statement {
                        label: None,
                        kind: StmtKind::Unless {
                            condition: cond,
                            body: vec![stmt],
                            else_block: None,
                        },
                        line,
                    });
                }
                _ => {}
            }
        }
        self.eat(&Token::Semicolon);
        Ok(stmt)
    }

    // ── Expressions (Pratt / precedence climbing) ──

    fn parse_expression(&mut self) -> PerlResult<Expr> {
        self.parse_comma_expr()
    }

    fn parse_comma_expr(&mut self) -> PerlResult<Expr> {
        let expr = self.parse_assign_expr()?;
        let mut exprs = vec![expr];
        while self.eat(&Token::Comma) || self.eat(&Token::FatArrow) {
            if matches!(
                self.peek(),
                Token::RParen | Token::RBracket | Token::RBrace | Token::Semicolon | Token::Eof
            ) {
                break; // trailing comma
            }
            exprs.push(self.parse_assign_expr()?);
        }
        if exprs.len() == 1 {
            return Ok(exprs.pop().unwrap());
        }
        let line = exprs[0].line;
        Ok(Expr {
            kind: ExprKind::List(exprs),
            line,
        })
    }

    fn parse_assign_expr(&mut self) -> PerlResult<Expr> {
        let expr = self.parse_ternary()?;
        let line = expr.line;

        match self.peek().clone() {
            Token::Assign => {
                self.advance();
                let right = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::Assign {
                        target: Box::new(expr),
                        value: Box::new(right),
                    },
                    line,
                })
            }
            Token::PlusAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::Add,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::MinusAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::Sub,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::MulAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::Mul,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::DivAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::Div,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::ModAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::Mod,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::PowAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::Pow,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::DotAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::Concat,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::BitAndAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::BitAnd,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::BitOrAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::BitOr,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::XorAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::BitXor,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::ShiftLeftAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::ShiftLeft,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::ShiftRightAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::ShiftRight,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::OrAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::LogOr,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::DefinedOrAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::DefinedOr,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            Token::AndAssign => {
                self.advance();
                let r = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::CompoundAssign {
                        target: Box::new(expr),
                        op: BinOp::LogAnd,
                        value: Box::new(r),
                    },
                    line,
                })
            }
            _ => Ok(expr),
        }
    }

    fn parse_ternary(&mut self) -> PerlResult<Expr> {
        let expr = self.parse_pipe_forward()?;
        if self.eat(&Token::Question) {
            let line = expr.line;
            let then_expr = self.parse_assign_expr()?;
            self.expect(&Token::Colon)?;
            let else_expr = self.parse_assign_expr()?;
            return Ok(Expr {
                kind: ExprKind::Ternary {
                    condition: Box::new(expr),
                    then_expr: Box::new(then_expr),
                    else_expr: Box::new(else_expr),
                },
                line,
            });
        }
        Ok(expr)
    }

    /// `EXPR |> CALL` — pipe-forward (F#/Elixir). Left-associative; the LHS is threaded
    /// in as the **first argument** of the RHS call at parse time (pure AST rewrite,
    /// no runtime cost). `x |> f(a, b)` → `f(x, a, b)`; `x |> f` → `f(x)`; chain
    /// `x |> f |> g(2)` → `g(f(x), 2)`. Precedence sits between `?:` and `||`, so
    /// `x + 1 |> f || y` parses as `f(x + 1) || y`.
    fn parse_pipe_forward(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_or_word()?;
        // Inside a paren-less arg list, `|>` is a hard terminator for the
        // enclosing call — leave it for the outer `parse_pipe_forward` loop
        // so `qw(…) |> head 2 |> join "-"` chains left-to-right as
        // `(qw(…) |> head 2) |> join "-"` instead of `head` swallowing the
        // outer `|>` via its first-arg `parse_assign_expr`.
        if self.no_pipe_forward_depth > 0 {
            return Ok(left);
        }
        while matches!(self.peek(), Token::PipeForward) {
            let line = left.line;
            self.advance();
            // Set pipe-RHS context so list-taking builtins (`map`, `grep`,
            // `join`, …) accept a placeholder in place of their list operand.
            self.pipe_rhs_depth = self.pipe_rhs_depth.saturating_add(1);
            let right_result = self.parse_or_word();
            self.pipe_rhs_depth = self.pipe_rhs_depth.saturating_sub(1);
            let right = right_result?;
            left = self.pipe_forward_apply(left, right, line)?;
        }
        Ok(left)
    }

    /// Desugar `lhs |> rhs`: thread `lhs` into the call that `rhs` represents as
    /// its **first** argument (Elixir / R / proposed-JS convention).
    ///
    /// The strategy depends on the shape of `rhs`:
    /// - Generic calls (`FuncCall`, `MethodCall`, `IndirectCall`) and variadic
    ///   builtins (`Print`, `Say`, `Printf`, `Die`, `Warn`, `Sprintf`, `System`,
    ///   `Exec`, `Unlink`, `Chmod`, `Chown`, `Glob`, …) — **prepend** `lhs` to
    ///   the args list. So `URL |> json_jq ".[]"` → `json_jq(URL, ".[]")`,
    ///   matching the `(data, filter)` signature the builtin expects.
    /// - Unary-style builtins (`Length`, `Abs`, `Lc`, `Uc`, `Defined`, `Ref`,
    ///   `Keys`, `Values`, `Pop`, `Shift`, …) — **replace** the sole operand with
    ///   `lhs` (these parse a single default `$_` when called without an arg, so
    ///   piping overrides that default; first-arg and last-arg are identical).
    /// - List-taking higher-order forms (`map`, `flat_map`, `grep`, `sort`, `join`, `reduce`, `fold`,
    ///   `pmap`, `pflat_map`, `pgrep`, `pfor`, …) — **replace** the `list` field with `lhs`, so
    ///   `@arr |> map { $_ * 2 }` becomes `map { $_ * 2 } @arr`.
    /// - `Bareword("f")` — lift to `FuncCall { f, [lhs] }`.
    /// - Scalar / deref / coderef expressions — wrap in `IndirectCall` with `lhs`
    ///   as the sole argument.
    /// - Ambiguous forms (binary ops, ternaries, literals, lists) — parse error,
    ///   since silently calling a non-callable at runtime would be worse.
    fn pipe_forward_apply(&self, lhs: Expr, rhs: Expr, line: usize) -> PerlResult<Expr> {
        let Expr { kind, line: rline } = rhs;
        let new_kind = match kind {
            // ── Generic / user-defined calls ───────────────────────────────────
            ExprKind::FuncCall { name, mut args } => {
                match name.as_str() {
                    "puniq" | "uniq" | "distinct" | "flatten" | "set" | "list_count"
                    | "list_size" | "count" | "size" | "cnt" | "with_index" | "shuffle" => {
                        if args.is_empty() {
                            args.push(lhs);
                        } else {
                            args[0] = lhs;
                        }
                    }
                    "chunked" | "windowed" => {
                        if args.is_empty() {
                            return Err(self.syntax_err(
                                "|>: chunked(N) / windowed(N) needs size — e.g. `@a |> windowed(2)`",
                                line,
                            ));
                        }
                        args.insert(0, lhs);
                    }
                    "List::Util::reduce" | "List::Util::fold" => {
                        args.push(lhs);
                    }
                    "pfirst" | "pany" | "any" | "all" | "none" | "first" | "take_while"
                    | "drop_while" | "tap" | "peek" | "group_by" | "chunk_by" => {
                        if args.len() < 2 {
                            return Err(self.syntax_err(
                                format!(
                                    "|>: `{name}` needs {{ BLOCK }}, LIST so the list can receive the pipe"
                                ),
                                line,
                            ));
                        }
                        args[1] = lhs;
                    }
                    "take" | "head" | "tail" | "drop" | "List::Util::head" | "List::Util::tail" => {
                        if args.is_empty() {
                            return Err(self.syntax_err(
                                "|>: `{name}` needs N last — e.g. `@a |> take(3)` for `take(@a, 3)`",
                                line,
                            ));
                        }
                        // `LIST |> take N` → `take(LIST, N)` (prepend piped list before trailing count)
                        args.insert(0, lhs);
                    }
                    _ => {
                        args.insert(0, lhs);
                    }
                }
                ExprKind::FuncCall { name, args }
            }
            ExprKind::MethodCall {
                object,
                method,
                mut args,
                super_call,
            } => {
                args.insert(0, lhs);
                ExprKind::MethodCall {
                    object,
                    method,
                    args,
                    super_call,
                }
            }
            ExprKind::IndirectCall {
                target,
                mut args,
                ampersand,
                pass_caller_arglist: _,
            } => {
                args.insert(0, lhs);
                ExprKind::IndirectCall {
                    target,
                    args,
                    ampersand,
                    // Prepending an explicit first arg means this is no longer
                    // "pass the caller's @_" — that form is only bare `&$cr`.
                    pass_caller_arglist: false,
                }
            }

            // ── Print-like / diagnostic ops (variadic) ─────────────────────────
            ExprKind::Print { handle, mut args } => {
                args.insert(0, lhs);
                ExprKind::Print { handle, args }
            }
            ExprKind::Say { handle, mut args } => {
                args.insert(0, lhs);
                ExprKind::Say { handle, args }
            }
            ExprKind::Printf { handle, mut args } => {
                args.insert(0, lhs);
                ExprKind::Printf { handle, args }
            }
            ExprKind::Die(mut args) => {
                args.insert(0, lhs);
                ExprKind::Die(args)
            }
            ExprKind::Warn(mut args) => {
                args.insert(0, lhs);
                ExprKind::Warn(args)
            }

            // ── Sprintf: first-arg pipe threads lhs into the `format` slot ─────
            //   `"n=%d" |> sprintf(42)` → `sprintf("n=%d", 42)` is awkward,
            //   but piping the format string is the rarer case. Prepending
            //   to the values list gives `sprintf(format, lhs, ...args)` for
            //   the common `$n |> sprintf "count=%d"` case.
            ExprKind::Sprintf { format, mut args } => {
                args.insert(0, lhs);
                ExprKind::Sprintf { format, args }
            }

            // ── System / exec / globbing / filesystem variadics ────────────────
            ExprKind::System(mut args) => {
                args.insert(0, lhs);
                ExprKind::System(args)
            }
            ExprKind::Exec(mut args) => {
                args.insert(0, lhs);
                ExprKind::Exec(args)
            }
            ExprKind::Unlink(mut args) => {
                args.insert(0, lhs);
                ExprKind::Unlink(args)
            }
            ExprKind::Chmod(mut args) => {
                args.insert(0, lhs);
                ExprKind::Chmod(args)
            }
            ExprKind::Chown(mut args) => {
                args.insert(0, lhs);
                ExprKind::Chown(args)
            }
            ExprKind::Glob(mut args) => {
                args.insert(0, lhs);
                ExprKind::Glob(args)
            }
            ExprKind::Files(mut args) => {
                args.insert(0, lhs);
                ExprKind::Files(args)
            }
            ExprKind::Filesf(mut args) => {
                args.insert(0, lhs);
                ExprKind::Filesf(args)
            }
            ExprKind::Dirs(mut args) => {
                args.insert(0, lhs);
                ExprKind::Dirs(args)
            }
            ExprKind::SymLinks(mut args) => {
                args.insert(0, lhs);
                ExprKind::SymLinks(args)
            }
            ExprKind::Sockets(mut args) => {
                args.insert(0, lhs);
                ExprKind::Sockets(args)
            }
            ExprKind::Pipes(mut args) => {
                args.insert(0, lhs);
                ExprKind::Pipes(args)
            }
            ExprKind::BlockDevices(mut args) => {
                args.insert(0, lhs);
                ExprKind::BlockDevices(args)
            }
            ExprKind::CharDevices(mut args) => {
                args.insert(0, lhs);
                ExprKind::CharDevices(args)
            }
            ExprKind::GlobPar { mut args, progress } => {
                args.insert(0, lhs);
                ExprKind::GlobPar { args, progress }
            }
            ExprKind::ParSed { mut args, progress } => {
                args.insert(0, lhs);
                ExprKind::ParSed { args, progress }
            }

            // ── Unary-style builtins: replace the lone operand with `lhs` ──────
            ExprKind::Length(_) => ExprKind::Length(Box::new(lhs)),
            ExprKind::Abs(_) => ExprKind::Abs(Box::new(lhs)),
            ExprKind::Int(_) => ExprKind::Int(Box::new(lhs)),
            ExprKind::Sqrt(_) => ExprKind::Sqrt(Box::new(lhs)),
            ExprKind::Sin(_) => ExprKind::Sin(Box::new(lhs)),
            ExprKind::Cos(_) => ExprKind::Cos(Box::new(lhs)),
            ExprKind::Exp(_) => ExprKind::Exp(Box::new(lhs)),
            ExprKind::Log(_) => ExprKind::Log(Box::new(lhs)),
            ExprKind::Hex(_) => ExprKind::Hex(Box::new(lhs)),
            ExprKind::Oct(_) => ExprKind::Oct(Box::new(lhs)),
            ExprKind::Lc(_) => ExprKind::Lc(Box::new(lhs)),
            ExprKind::Uc(_) => ExprKind::Uc(Box::new(lhs)),
            ExprKind::Lcfirst(_) => ExprKind::Lcfirst(Box::new(lhs)),
            ExprKind::Ucfirst(_) => ExprKind::Ucfirst(Box::new(lhs)),
            ExprKind::Fc(_) => ExprKind::Fc(Box::new(lhs)),
            ExprKind::Chr(_) => ExprKind::Chr(Box::new(lhs)),
            ExprKind::Ord(_) => ExprKind::Ord(Box::new(lhs)),
            ExprKind::Chomp(_) => ExprKind::Chomp(Box::new(lhs)),
            ExprKind::Chop(_) => ExprKind::Chop(Box::new(lhs)),
            ExprKind::Defined(_) => ExprKind::Defined(Box::new(lhs)),
            ExprKind::Ref(_) => ExprKind::Ref(Box::new(lhs)),
            ExprKind::ScalarContext(_) => ExprKind::ScalarContext(Box::new(lhs)),
            ExprKind::Keys(_) => ExprKind::Keys(Box::new(lhs)),
            ExprKind::Values(_) => ExprKind::Values(Box::new(lhs)),
            ExprKind::Each(_) => ExprKind::Each(Box::new(lhs)),
            ExprKind::Pop(_) => ExprKind::Pop(Box::new(lhs)),
            ExprKind::Shift(_) => ExprKind::Shift(Box::new(lhs)),
            ExprKind::Delete(_) => ExprKind::Delete(Box::new(lhs)),
            ExprKind::Exists(_) => ExprKind::Exists(Box::new(lhs)),
            ExprKind::ReverseExpr(_) => ExprKind::ReverseExpr(Box::new(lhs)),
            ExprKind::Slurp(_) => ExprKind::Slurp(Box::new(lhs)),
            ExprKind::Capture(_) => ExprKind::Capture(Box::new(lhs)),
            ExprKind::Qx(_) => ExprKind::Qx(Box::new(lhs)),
            ExprKind::FetchUrl(_) => ExprKind::FetchUrl(Box::new(lhs)),
            ExprKind::Close(_) => ExprKind::Close(Box::new(lhs)),
            ExprKind::Chdir(_) => ExprKind::Chdir(Box::new(lhs)),
            ExprKind::Readdir(_) => ExprKind::Readdir(Box::new(lhs)),
            ExprKind::Closedir(_) => ExprKind::Closedir(Box::new(lhs)),
            ExprKind::Rewinddir(_) => ExprKind::Rewinddir(Box::new(lhs)),
            ExprKind::Telldir(_) => ExprKind::Telldir(Box::new(lhs)),
            ExprKind::Stat(_) => ExprKind::Stat(Box::new(lhs)),
            ExprKind::Lstat(_) => ExprKind::Lstat(Box::new(lhs)),
            ExprKind::Readlink(_) => ExprKind::Readlink(Box::new(lhs)),
            ExprKind::Study(_) => ExprKind::Study(Box::new(lhs)),
            ExprKind::Await(_) => ExprKind::Await(Box::new(lhs)),
            ExprKind::Eval(_) => ExprKind::Eval(Box::new(lhs)),

            // ── Higher-order / list-taking forms: replace the `list` slot ──────
            ExprKind::MapExpr {
                block,
                list: _,
                flatten_array_refs,
            } => ExprKind::MapExpr {
                block,
                list: Box::new(lhs),
                flatten_array_refs,
            },
            ExprKind::MapExprComma {
                expr,
                list: _,
                flatten_array_refs,
            } => ExprKind::MapExprComma {
                expr,
                list: Box::new(lhs),
                flatten_array_refs,
            },
            ExprKind::GrepExpr { block, list: _ } => ExprKind::GrepExpr {
                block,
                list: Box::new(lhs),
            },
            ExprKind::GrepExprComma { expr, list: _ } => ExprKind::GrepExprComma {
                expr,
                list: Box::new(lhs),
            },
            ExprKind::ForEachExpr { block, list: _ } => ExprKind::ForEachExpr {
                block,
                list: Box::new(lhs),
            },
            ExprKind::SortExpr { cmp, list: _ } => ExprKind::SortExpr {
                cmp,
                list: Box::new(lhs),
            },
            ExprKind::JoinExpr { separator, list: _ } => ExprKind::JoinExpr {
                separator,
                list: Box::new(lhs),
            },
            ExprKind::ReduceExpr { block, list: _ } => ExprKind::ReduceExpr {
                block,
                list: Box::new(lhs),
            },
            ExprKind::PMapExpr {
                block,
                list: _,
                progress,
                flat_outputs,
                on_cluster,
            } => ExprKind::PMapExpr {
                block,
                list: Box::new(lhs),
                progress,
                flat_outputs,
                on_cluster,
            },
            ExprKind::PMapChunkedExpr {
                chunk_size,
                block,
                list: _,
                progress,
            } => ExprKind::PMapChunkedExpr {
                chunk_size,
                block,
                list: Box::new(lhs),
                progress,
            },
            ExprKind::PGrepExpr {
                block,
                list: _,
                progress,
            } => ExprKind::PGrepExpr {
                block,
                list: Box::new(lhs),
                progress,
            },
            ExprKind::PForExpr {
                block,
                list: _,
                progress,
            } => ExprKind::PForExpr {
                block,
                list: Box::new(lhs),
                progress,
            },
            ExprKind::PSortExpr {
                cmp,
                list: _,
                progress,
            } => ExprKind::PSortExpr {
                cmp,
                list: Box::new(lhs),
                progress,
            },
            ExprKind::PReduceExpr {
                block,
                list: _,
                progress,
            } => ExprKind::PReduceExpr {
                block,
                list: Box::new(lhs),
                progress,
            },
            ExprKind::PcacheExpr {
                block,
                list: _,
                progress,
            } => ExprKind::PcacheExpr {
                block,
                list: Box::new(lhs),
                progress,
            },
            ExprKind::PReduceInitExpr {
                init,
                block,
                list: _,
                progress,
            } => ExprKind::PReduceInitExpr {
                init,
                block,
                list: Box::new(lhs),
                progress,
            },
            ExprKind::PMapReduceExpr {
                map_block,
                reduce_block,
                list: _,
                progress,
            } => ExprKind::PMapReduceExpr {
                map_block,
                reduce_block,
                list: Box::new(lhs),
                progress,
            },

            // ── Push / unshift: first arg is the array, so pipe the LHS
            //     into the **values** list — `"x" |> push(@arr)` → `push @arr, "x"`
            //     is unchanged, but `@arr |> push "x"` is unnatural; use push
            //     directly for that.
            ExprKind::Push { array, mut values } => {
                values.insert(0, lhs);
                ExprKind::Push { array, values }
            }
            ExprKind::Unshift { array, mut values } => {
                values.insert(0, lhs);
                ExprKind::Unshift { array, values }
            }

            // ── Split: pipe the subject string — `$line |> split /,/` ─────────
            ExprKind::SplitExpr {
                pattern,
                string: _,
                limit,
            } => ExprKind::SplitExpr {
                pattern,
                string: Box::new(lhs),
                limit,
            },

            // ── Bareword function name → plain unary call ──────────────────────
            ExprKind::Bareword(name) => ExprKind::FuncCall {
                name,
                args: vec![lhs],
            },

            // ── Callable scalars / coderefs / derefs → IndirectCall ────────────
            kind @ (ExprKind::ScalarVar(_)
            | ExprKind::ArrayElement { .. }
            | ExprKind::HashElement { .. }
            | ExprKind::Deref { .. }
            | ExprKind::ArrowDeref { .. }
            | ExprKind::CodeRef { .. }
            | ExprKind::SubroutineRef(_)
            | ExprKind::SubroutineCodeRef(_)
            | ExprKind::DynamicSubCodeRef(_)) => ExprKind::IndirectCall {
                target: Box::new(Expr { kind, line: rline }),
                args: vec![lhs],
                ampersand: false,
                pass_caller_arglist: false,
            },

            other => {
                return Err(self.syntax_err(
                    format!(
                        "right-hand side of `|>` must be a call, builtin, or coderef \
                         expression (got {})",
                        Self::expr_kind_name(&other)
                    ),
                    line,
                ));
            }
        };
        Ok(Expr {
            kind: new_kind,
            line,
        })
    }

    /// Short label for an `ExprKind` (used in `|>` error messages).
    fn expr_kind_name(kind: &ExprKind) -> &'static str {
        match kind {
            ExprKind::Integer(_) | ExprKind::Float(_) => "numeric literal",
            ExprKind::String(_) | ExprKind::InterpolatedString(_) => "string literal",
            ExprKind::BinOp { .. } => "binary expression",
            ExprKind::UnaryOp { .. } => "unary expression",
            ExprKind::Ternary { .. } => "ternary expression",
            ExprKind::Assign { .. } | ExprKind::CompoundAssign { .. } => "assignment",
            ExprKind::List(_) => "list expression",
            ExprKind::Range { .. } => "range expression",
            _ => "expression",
        }
    }

    // or / not (lowest precedence word operators)
    fn parse_or_word(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_and_word()?;
        while matches!(self.peek(), Token::LogOrWord) {
            let line = left.line;
            self.advance();
            let right = self.parse_and_word()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op: BinOp::LogOrWord,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_and_word(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_not_word()?;
        while matches!(self.peek(), Token::LogAndWord) {
            let line = left.line;
            self.advance();
            let right = self.parse_not_word()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op: BinOp::LogAndWord,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_not_word(&mut self) -> PerlResult<Expr> {
        if matches!(self.peek(), Token::LogNotWord) {
            let line = self.peek_line();
            self.advance();
            let expr = self.parse_not_word()?;
            return Ok(Expr {
                kind: ExprKind::UnaryOp {
                    op: UnaryOp::LogNotWord,
                    expr: Box::new(expr),
                },
                line,
            });
        }
        self.parse_range()
    }

    fn parse_log_or(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_log_and()?;
        loop {
            let op = match self.peek() {
                Token::LogOr => BinOp::LogOr,
                Token::DefinedOr => BinOp::DefinedOr,
                _ => break,
            };
            let line = left.line;
            self.advance();
            let right = self.parse_log_and()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_log_and(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_bit_or()?;
        while matches!(self.peek(), Token::LogAnd) {
            let line = left.line;
            self.advance();
            let right = self.parse_bit_or()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op: BinOp::LogAnd,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_bit_or(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_bit_xor()?;
        while matches!(self.peek(), Token::BitOr) {
            let line = left.line;
            self.advance();
            let right = self.parse_bit_xor()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op: BinOp::BitOr,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_bit_xor(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_bit_and()?;
        while matches!(self.peek(), Token::BitXor) {
            let line = left.line;
            self.advance();
            let right = self.parse_bit_and()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op: BinOp::BitXor,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_bit_and(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_equality()?;
        while matches!(self.peek(), Token::BitAnd) {
            let line = left.line;
            self.advance();
            let right = self.parse_equality()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op: BinOp::BitAnd,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.peek() {
                Token::NumEq => BinOp::NumEq,
                Token::NumNe => BinOp::NumNe,
                Token::StrEq => BinOp::StrEq,
                Token::StrNe => BinOp::StrNe,
                Token::Spaceship => BinOp::Spaceship,
                Token::StrCmp => BinOp::StrCmp,
                _ => break,
            };
            let line = left.line;
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_shift()?;
        loop {
            let op = match self.peek() {
                Token::NumLt => BinOp::NumLt,
                Token::NumGt => BinOp::NumGt,
                Token::NumLe => BinOp::NumLe,
                Token::NumGe => BinOp::NumGe,
                Token::StrLt => BinOp::StrLt,
                Token::StrGt => BinOp::StrGt,
                Token::StrLe => BinOp::StrLe,
                Token::StrGe => BinOp::StrGe,
                _ => break,
            };
            let line = left.line;
            self.advance();
            let right = self.parse_shift()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_addition()?;
        loop {
            let op = match self.peek() {
                Token::ShiftLeft => BinOp::ShiftLeft,
                Token::ShiftRight => BinOp::ShiftRight,
                _ => break,
            };
            let line = left.line;
            self.advance();
            let right = self.parse_addition()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_addition(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                Token::Dot => BinOp::Concat,
                _ => break,
            };
            let line = left.line;
            self.advance();
            let right = self.parse_multiplication()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_regex_bind()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                Token::X => {
                    let line = left.line;
                    self.advance();
                    let right = self.parse_regex_bind()?;
                    left = Expr {
                        kind: ExprKind::Repeat {
                            expr: Box::new(left),
                            count: Box::new(right),
                        },
                        line,
                    };
                    continue;
                }
                _ => break,
            };
            let line = left.line;
            self.advance();
            let right = self.parse_regex_bind()?;
            left = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                line,
            };
        }
        Ok(left)
    }

    fn parse_regex_bind(&mut self) -> PerlResult<Expr> {
        let left = self.parse_unary()?;
        match self.peek() {
            Token::BindMatch => {
                let line = left.line;
                self.advance();
                match self.peek().clone() {
                    Token::Regex(pattern, flags) => {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::Match {
                                expr: Box::new(left),
                                pattern,
                                flags,
                                scalar_g: false,
                            },
                            line,
                        })
                    }
                    Token::Ident(ref s) if s.starts_with('\x00') => {
                        let (Token::Ident(encoded), _) = self.advance() else {
                            unreachable!()
                        };
                        let parts: Vec<&str> = encoded.split('\x00').collect();
                        if parts.len() >= 4 && parts[1] == "s" {
                            Ok(Expr {
                                kind: ExprKind::Substitution {
                                    expr: Box::new(left),
                                    pattern: parts[2].to_string(),
                                    replacement: parts[3].to_string(),
                                    flags: parts.get(4).unwrap_or(&"").to_string(),
                                },
                                line,
                            })
                        } else if parts.len() >= 4 && parts[1] == "tr" {
                            Ok(Expr {
                                kind: ExprKind::Transliterate {
                                    expr: Box::new(left),
                                    from: parts[2].to_string(),
                                    to: parts[3].to_string(),
                                    flags: parts.get(4).unwrap_or(&"").to_string(),
                                },
                                line,
                            })
                        } else {
                            Err(self.syntax_err("Invalid regex binding", line))
                        }
                    }
                    _ => {
                        let rhs = self.parse_unary()?;
                        Ok(Expr {
                            kind: ExprKind::BinOp {
                                left: Box::new(left),
                                op: BinOp::BindMatch,
                                right: Box::new(rhs),
                            },
                            line,
                        })
                    }
                }
            }
            Token::BindNotMatch => {
                let line = left.line;
                self.advance();
                match self.peek().clone() {
                    Token::Regex(pattern, flags) => {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::UnaryOp {
                                op: UnaryOp::LogNot,
                                expr: Box::new(Expr {
                                    kind: ExprKind::Match {
                                        expr: Box::new(left),
                                        pattern,
                                        flags,
                                        scalar_g: false,
                                    },
                                    line,
                                }),
                            },
                            line,
                        })
                    }
                    Token::Ident(ref s) if s.starts_with('\x00') => {
                        let (Token::Ident(encoded), _) = self.advance() else {
                            unreachable!()
                        };
                        let parts: Vec<&str> = encoded.split('\x00').collect();
                        if parts.len() >= 4 && parts[1] == "s" {
                            Ok(Expr {
                                kind: ExprKind::UnaryOp {
                                    op: UnaryOp::LogNot,
                                    expr: Box::new(Expr {
                                        kind: ExprKind::Substitution {
                                            expr: Box::new(left),
                                            pattern: parts[2].to_string(),
                                            replacement: parts[3].to_string(),
                                            flags: parts.get(4).unwrap_or(&"").to_string(),
                                        },
                                        line,
                                    }),
                                },
                                line,
                            })
                        } else if parts.len() >= 4 && parts[1] == "tr" {
                            Ok(Expr {
                                kind: ExprKind::UnaryOp {
                                    op: UnaryOp::LogNot,
                                    expr: Box::new(Expr {
                                        kind: ExprKind::Transliterate {
                                            expr: Box::new(left),
                                            from: parts[2].to_string(),
                                            to: parts[3].to_string(),
                                            flags: parts.get(4).unwrap_or(&"").to_string(),
                                        },
                                        line,
                                    }),
                                },
                                line,
                            })
                        } else {
                            Err(self.syntax_err("Invalid regex binding after !~", line))
                        }
                    }
                    _ => {
                        let rhs = self.parse_unary()?;
                        Ok(Expr {
                            kind: ExprKind::BinOp {
                                left: Box::new(left),
                                op: BinOp::BindNotMatch,
                                right: Box::new(rhs),
                            },
                            line,
                        })
                    }
                }
            }
            _ => Ok(left),
        }
    }

    /// Perl `..` / `...` operator — precedence sits between `?:` and `||` (`perlop`), so
    /// `$x .. $x + 3` parses as `$x .. ($x + 3)` and `1..$n||5` parses as `1..($n||5)`. Both
    /// operands recurse through `parse_log_or`, which in turn walks down through all tighter
    /// operators (additive, multiplicative, regex bind, unary). Non-associative: the right
    /// operand is a single `parse_log_or` so `1..5..10` is a parse error in Perl, but we accept
    /// it greedily (left-associated) because the lexer already forbids `..` after a range RHS.
    fn parse_range(&mut self) -> PerlResult<Expr> {
        let left = self.parse_log_or()?;
        let line = left.line;
        let exclusive = if self.eat(&Token::RangeExclusive) {
            true
        } else if self.eat(&Token::Range) {
            false
        } else {
            return Ok(left);
        };
        let right = self.parse_log_or()?;
        Ok(Expr {
            kind: ExprKind::Range {
                from: Box::new(left),
                to: Box::new(right),
                exclusive,
            },
            line,
        })
    }

    /// `name` or `Foo::Bar::baz` — used after `sub`, unary `&`, etc.
    fn parse_package_qualified_identifier(&mut self) -> PerlResult<String> {
        let mut name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, l) => {
                return Err(self.syntax_err(format!("Expected identifier, got {:?}", tok), l));
            }
        };
        while self.eat(&Token::PackageSep) {
            match self.advance() {
                (Token::Ident(part), _) => {
                    name.push_str("::");
                    name.push_str(&part);
                }
                (tok, l) => {
                    return Err(self
                        .syntax_err(format!("Expected identifier after `::`, got {:?}", tok), l));
                }
            }
        }
        Ok(name)
    }

    /// After consuming unary `&`: `name` or `Foo::Bar::baz` (Perl `&foo` / `&Foo::bar`).
    fn parse_qualified_subroutine_name(&mut self) -> PerlResult<String> {
        self.parse_package_qualified_identifier()
    }

    fn parse_unary(&mut self) -> PerlResult<Expr> {
        let line = self.peek_line();
        match self.peek().clone() {
            Token::Minus => {
                self.advance();
                let expr = self.parse_power()?;
                Ok(Expr {
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::Negate,
                        expr: Box::new(expr),
                    },
                    line,
                })
            }
            // Unary `+EXPR` — Perl uses this to disambiguate barewords in hash subscripts (`$h{+Foo}`)
            // and for scalar context; treat as a no-op on the parsed operand.
            Token::Plus => {
                self.advance();
                self.parse_unary()
            }
            Token::LogNot => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr {
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::LogNot,
                        expr: Box::new(expr),
                    },
                    line,
                })
            }
            Token::BitNot => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr {
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::BitNot,
                        expr: Box::new(expr),
                    },
                    line,
                })
            }
            Token::Increment => {
                self.advance();
                let expr = self.parse_postfix()?;
                Ok(Expr {
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::PreIncrement,
                        expr: Box::new(expr),
                    },
                    line,
                })
            }
            Token::Decrement => {
                self.advance();
                let expr = self.parse_postfix()?;
                Ok(Expr {
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::PreDecrement,
                        expr: Box::new(expr),
                    },
                    line,
                })
            }
            Token::BitAnd => {
                // Unary `&name` / `&Pkg::name` (call / coderef); binary `&` is in `parse_bit_and`.
                // `&$coderef(...)` — call sub whose ref is in a scalar (core `B.pm` / `&$recurse($sym)`).
                self.advance();
                if matches!(self.peek(), Token::LBrace) {
                    self.advance();
                    let inner = self.parse_expression()?;
                    self.expect(&Token::RBrace)?;
                    return Ok(Expr {
                        kind: ExprKind::DynamicSubCodeRef(Box::new(inner)),
                        line,
                    });
                }
                if matches!(self.peek(), Token::Ident(_)) {
                    let name = self.parse_qualified_subroutine_name()?;
                    return Ok(Expr {
                        kind: ExprKind::SubroutineRef(name),
                        line,
                    });
                }
                let target = self.parse_primary()?;
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&Token::RParen)?;
                    return Ok(Expr {
                        kind: ExprKind::IndirectCall {
                            target: Box::new(target),
                            args,
                            ampersand: true,
                            pass_caller_arglist: false,
                        },
                        line,
                    });
                }
                // `&$coderef` / `&{expr}` with no `(...)` — call with caller's @_ (Perl `&$sub`).
                Ok(Expr {
                    kind: ExprKind::IndirectCall {
                        target: Box::new(target),
                        args: vec![],
                        ampersand: true,
                        pass_caller_arglist: true,
                    },
                    line,
                })
            }
            Token::Backslash => {
                self.advance();
                let expr = self.parse_unary()?;
                if let ExprKind::SubroutineRef(name) = expr.kind {
                    return Ok(Expr {
                        kind: ExprKind::SubroutineCodeRef(name),
                        line,
                    });
                }
                if matches!(expr.kind, ExprKind::DynamicSubCodeRef(_)) {
                    return Ok(expr);
                }
                // `\` uses `ScalarRef`; array/hash vars and `\@{...}` lower to binding or alias refs.
                Ok(Expr {
                    kind: ExprKind::ScalarRef(Box::new(expr)),
                    line,
                })
            }
            Token::FileTest(op) => {
                self.advance();
                // Perl: `-d` with no operand uses `$_` (e.g. `if (-d)` inside `for` / `while read`).
                let expr = if Self::filetest_allows_implicit_topic(self.peek()) {
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line: self.peek_line(),
                    }
                } else {
                    self.parse_unary()?
                };
                Ok(Expr {
                    kind: ExprKind::FileTest {
                        op,
                        expr: Box::new(expr),
                    },
                    line,
                })
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> PerlResult<Expr> {
        let left = self.parse_postfix()?;
        if matches!(self.peek(), Token::Power) {
            let line = left.line;
            self.advance();
            let right = self.parse_unary()?; // right-associative
            return Ok(Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op: BinOp::Pow,
                    right: Box::new(right),
                },
                line,
            });
        }
        Ok(left)
    }

    fn parse_postfix(&mut self) -> PerlResult<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek().clone() {
                Token::Increment => {
                    let line = expr.line;
                    self.advance();
                    expr = Expr {
                        kind: ExprKind::PostfixOp {
                            expr: Box::new(expr),
                            op: PostfixOp::Increment,
                        },
                        line,
                    };
                }
                Token::Decrement => {
                    let line = expr.line;
                    self.advance();
                    expr = Expr {
                        kind: ExprKind::PostfixOp {
                            expr: Box::new(expr),
                            op: PostfixOp::Decrement,
                        },
                        line,
                    };
                }
                Token::LParen => {
                    if self.suppress_indirect_paren_call > 0 {
                        break;
                    }
                    let line = expr.line;
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&Token::RParen)?;
                    expr = Expr {
                        kind: ExprKind::IndirectCall {
                            target: Box::new(expr),
                            args,
                            ampersand: false,
                            pass_caller_arglist: false,
                        },
                        line,
                    };
                }
                Token::Arrow => {
                    let line = expr.line;
                    self.advance();
                    match self.peek().clone() {
                        Token::LBracket => {
                            self.advance();
                            let index = self.parse_expression()?;
                            self.expect(&Token::RBracket)?;
                            expr = Expr {
                                kind: ExprKind::ArrowDeref {
                                    expr: Box::new(expr),
                                    index: Box::new(index),
                                    kind: DerefKind::Array,
                                },
                                line,
                            };
                        }
                        Token::LBrace => {
                            self.advance();
                            let key = self.parse_expression()?;
                            self.expect(&Token::RBrace)?;
                            expr = Expr {
                                kind: ExprKind::ArrowDeref {
                                    expr: Box::new(expr),
                                    index: Box::new(key),
                                    kind: DerefKind::Hash,
                                },
                                line,
                            };
                        }
                        Token::LParen => {
                            self.advance();
                            let args = self.parse_arg_list()?;
                            self.expect(&Token::RParen)?;
                            expr = Expr {
                                kind: ExprKind::ArrowDeref {
                                    expr: Box::new(expr),
                                    index: Box::new(Expr {
                                        kind: ExprKind::List(args),
                                        line,
                                    }),
                                    kind: DerefKind::Call,
                                },
                                line,
                            };
                        }
                        Token::Ident(method) => {
                            self.advance();
                            if method == "SUPER" {
                                self.expect(&Token::PackageSep)?;
                                let real_method = match self.advance() {
                                    (Token::Ident(n), _) => n,
                                    (tok, l) => {
                                        return Err(self.syntax_err(
                                            format!(
                                                "Expected method name after SUPER::, got {:?}",
                                                tok
                                            ),
                                            l,
                                        ));
                                    }
                                };
                                let args = if self.eat(&Token::LParen) {
                                    let a = self.parse_arg_list()?;
                                    self.expect(&Token::RParen)?;
                                    a
                                } else {
                                    self.parse_method_arg_list_no_paren()?
                                };
                                expr = Expr {
                                    kind: ExprKind::MethodCall {
                                        object: Box::new(expr),
                                        method: real_method,
                                        args,
                                        super_call: true,
                                    },
                                    line,
                                };
                            } else {
                                let mut method_name = method;
                                while self.eat(&Token::PackageSep) {
                                    match self.advance() {
                                        (Token::Ident(part), _) => {
                                            method_name.push_str("::");
                                            method_name.push_str(&part);
                                        }
                                        (tok, l) => {
                                            return Err(self.syntax_err(
                                                format!(
                                                    "Expected identifier after :: in method name, got {:?}",
                                                    tok
                                                ),
                                                l,
                                            ));
                                        }
                                    }
                                }
                                let args = if self.eat(&Token::LParen) {
                                    let a = self.parse_arg_list()?;
                                    self.expect(&Token::RParen)?;
                                    a
                                } else {
                                    self.parse_method_arg_list_no_paren()?
                                };
                                expr = Expr {
                                    kind: ExprKind::MethodCall {
                                        object: Box::new(expr),
                                        method: method_name,
                                        args,
                                        super_call: false,
                                    },
                                    line,
                                };
                            }
                        }
                        // `x` is lexed as `Token::X` (repeat op); after `->` it is a method name.
                        Token::X => {
                            self.advance();
                            let args = if self.eat(&Token::LParen) {
                                let a = self.parse_arg_list()?;
                                self.expect(&Token::RParen)?;
                                a
                            } else {
                                self.parse_method_arg_list_no_paren()?
                            };
                            expr = Expr {
                                kind: ExprKind::MethodCall {
                                    object: Box::new(expr),
                                    method: "x".to_string(),
                                    args,
                                    super_call: false,
                                },
                                line,
                            };
                        }
                        _ => break,
                    }
                }
                Token::LBracket => {
                    // `$a[i]` — or chained `$r->{k}[i]` / `$a[1][2]` — or list slice `(sort ...)[0]`.
                    let line = expr.line;
                    if matches!(expr.kind, ExprKind::ScalarVar(_)) {
                        if let ExprKind::ScalarVar(ref name) = expr.kind {
                            let name = name.clone();
                            self.advance();
                            let index = self.parse_expression()?;
                            self.expect(&Token::RBracket)?;
                            expr = Expr {
                                kind: ExprKind::ArrayElement {
                                    array: name,
                                    index: Box::new(index),
                                },
                                line,
                            };
                        }
                    } else if postfix_lbracket_is_arrow_container(&expr) {
                        self.advance();
                        let indices = self.parse_arg_list()?;
                        self.expect(&Token::RBracket)?;
                        expr = Expr {
                            kind: ExprKind::ArrowDeref {
                                expr: Box::new(expr),
                                index: Box::new(Expr {
                                    kind: ExprKind::List(indices),
                                    line,
                                }),
                                kind: DerefKind::Array,
                            },
                            line,
                        };
                    } else {
                        self.advance();
                        let indices = self.parse_arg_list()?;
                        self.expect(&Token::RBracket)?;
                        expr = Expr {
                            kind: ExprKind::AnonymousListSlice {
                                source: Box::new(expr),
                                indices,
                            },
                            line,
                        };
                    }
                }
                Token::LBrace => {
                    if self.suppress_scalar_hash_brace > 0 {
                        break;
                    }
                    // `$h{k}`, or chained `$h{k2}{k3}` / `$r->{a}{b}` / `$a[0]{k}` — second+ `{…}` is
                    // hash subscript on the scalar value (same as `-> { … }` without extra `->`).
                    let line = expr.line;
                    let is_scalar_named_hash = matches!(expr.kind, ExprKind::ScalarVar(_));
                    let is_chainable_hash_subscript = is_scalar_named_hash
                        || matches!(
                            expr.kind,
                            ExprKind::HashElement { .. }
                                | ExprKind::ArrayElement { .. }
                                | ExprKind::ArrowDeref { .. }
                                | ExprKind::Deref {
                                    kind: Sigil::Scalar,
                                    ..
                                }
                        );
                    if !is_chainable_hash_subscript {
                        break;
                    }
                    self.advance();
                    let key = self.parse_expression()?;
                    self.expect(&Token::RBrace)?;
                    expr = if is_scalar_named_hash {
                        if let ExprKind::ScalarVar(ref name) = expr.kind {
                            let name = name.clone();
                            // Perl: `$_ { k }` means `$_->{k}` (implicit arrow), not the `%_` stash hash.
                            if name == "_" {
                                Expr {
                                    kind: ExprKind::ArrowDeref {
                                        expr: Box::new(Expr {
                                            kind: ExprKind::ScalarVar("_".into()),
                                            line,
                                        }),
                                        index: Box::new(key),
                                        kind: DerefKind::Hash,
                                    },
                                    line,
                                }
                            } else {
                                Expr {
                                    kind: ExprKind::HashElement {
                                        hash: name,
                                        key: Box::new(key),
                                    },
                                    line,
                                }
                            }
                        } else {
                            unreachable!("is_scalar_named_hash implies ScalarVar");
                        }
                    } else {
                        Expr {
                            kind: ExprKind::ArrowDeref {
                                expr: Box::new(expr),
                                index: Box::new(key),
                                kind: DerefKind::Hash,
                            },
                            line,
                        }
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> PerlResult<Expr> {
        let line = self.peek_line();
        match self.peek().clone() {
            Token::Integer(n) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Integer(n),
                    line,
                })
            }
            Token::Float(f) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Float(f),
                    line,
                })
            }
            Token::Star => {
                self.advance();
                if matches!(self.peek(), Token::LBrace) {
                    self.advance();
                    let inner = self.parse_expression()?;
                    self.expect(&Token::RBrace)?;
                    return Ok(Expr {
                        kind: ExprKind::Deref {
                            expr: Box::new(inner),
                            kind: Sigil::Typeglob,
                        },
                        line,
                    });
                }
                // `*$_{$k}`, `*${expr}`, `*$foo` — typeglob from a sigil expression (Perl 5 `*$globref`).
                if matches!(
                    self.peek(),
                    Token::ScalarVar(_)
                        | Token::ArrayVar(_)
                        | Token::HashVar(_)
                        | Token::DerefScalarVar(_)
                        | Token::HashPercent
                ) {
                    let inner = self.parse_postfix()?;
                    return Ok(Expr {
                        kind: ExprKind::TypeglobExpr(Box::new(inner)),
                        line,
                    });
                }
                // `x` tokenizes as `Token::X` (repeat op) — still a valid package/typeglob name.
                let mut full_name = match self.advance() {
                    (Token::Ident(n), _) => n,
                    (Token::X, _) => "x".to_string(),
                    (tok, l) => {
                        return Err(self
                            .syntax_err(format!("Expected identifier after *, got {:?}", tok), l));
                    }
                };
                while self.eat(&Token::PackageSep) {
                    match self.advance() {
                        (Token::Ident(part), _) => {
                            full_name = format!("{}::{}", full_name, part);
                        }
                        (Token::X, _) => {
                            full_name = format!("{}::x", full_name);
                        }
                        (tok, l) => {
                            return Err(self.syntax_err(
                                format!("Expected identifier after :: in typeglob, got {:?}", tok),
                                l,
                            ));
                        }
                    }
                }
                Ok(Expr {
                    kind: ExprKind::Typeglob(full_name),
                    line,
                })
            }
            Token::SingleString(s) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::String(s),
                    line,
                })
            }
            Token::DoubleString(s) => {
                self.advance();
                self.parse_interpolated_string(&s, line)
            }
            Token::BacktickString(s) => {
                self.advance();
                let inner = self.parse_interpolated_string(&s, line)?;
                Ok(Expr {
                    kind: ExprKind::Qx(Box::new(inner)),
                    line,
                })
            }
            Token::HereDoc(_, body) => {
                self.advance();
                self.parse_interpolated_string(&body, line)
            }
            Token::Regex(pattern, flags) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Regex(pattern, flags),
                    line,
                })
            }
            Token::QW(words) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::QW(words),
                    line,
                })
            }
            Token::DerefScalarVar(name) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Deref {
                        expr: Box::new(Expr {
                            kind: ExprKind::ScalarVar(name),
                            line,
                        }),
                        kind: Sigil::Scalar,
                    },
                    line,
                })
            }
            Token::ScalarVar(name) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::ScalarVar(name),
                    line,
                })
            }
            Token::ArrayVar(name) => {
                self.advance();
                // Check for slice: @arr[...] (array slice) or @hash{...} (hash slice)
                match self.peek() {
                    Token::LBracket => {
                        self.advance();
                        let indices = self.parse_arg_list()?;
                        self.expect(&Token::RBracket)?;
                        Ok(Expr {
                            kind: ExprKind::ArraySlice {
                                array: name,
                                indices,
                            },
                            line,
                        })
                    }
                    Token::LBrace if self.suppress_scalar_hash_brace == 0 => {
                        self.advance();
                        let keys = self.parse_arg_list()?;
                        self.expect(&Token::RBrace)?;
                        Ok(Expr {
                            kind: ExprKind::HashSlice { hash: name, keys },
                            line,
                        })
                    }
                    _ => Ok(Expr {
                        kind: ExprKind::ArrayVar(name),
                        line,
                    }),
                }
            }
            Token::HashVar(name) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::HashVar(name),
                    line,
                })
            }
            Token::HashPercent => {
                // `%$href` — hash ref deref; `%{ $expr }` — symbolic / braced form
                self.advance();
                if matches!(self.peek(), Token::ScalarVar(_)) {
                    let n = match self.advance() {
                        (Token::ScalarVar(n), _) => n,
                        (tok, l) => {
                            return Err(self.syntax_err(
                                format!("Expected scalar variable after %%, got {:?}", tok),
                                l,
                            ));
                        }
                    };
                    return Ok(Expr {
                        kind: ExprKind::Deref {
                            expr: Box::new(Expr {
                                kind: ExprKind::ScalarVar(n),
                                line,
                            }),
                            kind: Sigil::Hash,
                        },
                        line,
                    });
                }
                self.expect(&Token::LBrace)?;
                let inner = self.parse_expression()?;
                self.expect(&Token::RBrace)?;
                Ok(Expr {
                    kind: ExprKind::Deref {
                        expr: Box::new(inner),
                        kind: Sigil::Hash,
                    },
                    line,
                })
            }
            Token::ArrayAt => {
                self.advance();
                // `@{ $expr }` / `@{ "Pkg::NAME" }` — symbolic array (e.g. `@{"$pkg\::EXPORT"}` in Exporter.pm)
                if matches!(self.peek(), Token::LBrace) {
                    self.advance();
                    let inner = self.parse_expression()?;
                    self.expect(&Token::RBrace)?;
                    return Ok(Expr {
                        kind: ExprKind::Deref {
                            expr: Box::new(inner),
                            kind: Sigil::Array,
                        },
                        line,
                    });
                }
                // `@$arr` — array dereference; `@$h{k1,k2}` — hash slice via hashref
                let container = match self.peek().clone() {
                    Token::ScalarVar(n) => {
                        self.advance();
                        Expr {
                            kind: ExprKind::ScalarVar(n),
                            line,
                        }
                    }
                    _ => {
                        return Err(self.syntax_err(
                            "Expected `$name` or `{` after `@` (e.g. `@$aref`, `@{expr}`, or `@$href{keys}`)",
                            line,
                        ));
                    }
                };
                if matches!(self.peek(), Token::LBrace) {
                    self.advance();
                    let keys = self.parse_arg_list()?;
                    self.expect(&Token::RBrace)?;
                    return Ok(Expr {
                        kind: ExprKind::HashSliceDeref {
                            container: Box::new(container),
                            keys,
                        },
                        line,
                    });
                }
                Ok(Expr {
                    kind: ExprKind::Deref {
                        expr: Box::new(container),
                        kind: Sigil::Array,
                    },
                    line,
                })
            }
            Token::LParen => {
                self.advance();
                if matches!(self.peek(), Token::RParen) {
                    self.advance();
                    return Ok(Expr {
                        kind: ExprKind::List(vec![]),
                        line,
                    });
                }
                let expr = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::LBracket => {
                self.advance();
                let elems = self.parse_arg_list()?;
                self.expect(&Token::RBracket)?;
                Ok(Expr {
                    kind: ExprKind::ArrayRef(elems),
                    line,
                })
            }
            Token::LBrace => {
                // Could be hash ref or block — disambiguate
                self.advance();
                // Try to parse as hash ref: { key => val, ... }
                let saved = self.pos;
                match self.try_parse_hash_ref() {
                    Ok(pairs) => Ok(Expr {
                        kind: ExprKind::HashRef(pairs),
                        line,
                    }),
                    Err(_) => {
                        self.pos = saved;
                        // Parse as block, wrap in code ref
                        let mut stmts = Vec::new();
                        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                            if self.eat(&Token::Semicolon) {
                                continue;
                            }
                            stmts.push(self.parse_statement()?);
                        }
                        self.expect(&Token::RBrace)?;
                        Ok(Expr {
                            kind: ExprKind::CodeRef {
                                params: vec![],
                                body: stmts,
                            },
                            line,
                        })
                    }
                }
            }
            Token::Diamond => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::ReadLine(None),
                    line,
                })
            }
            Token::ReadLine(handle) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::ReadLine(Some(handle)),
                    line,
                })
            }

            // Named functions / builtins
            Token::Ident(ref name) => {
                let name = name.clone();
                // Handle s///
                if name.starts_with('\x00') {
                    self.advance();
                    let parts: Vec<&str> = name.split('\x00').collect();
                    if parts.len() >= 4 && parts[1] == "s" {
                        return Ok(Expr {
                            kind: ExprKind::Substitution {
                                expr: Box::new(Expr {
                                    kind: ExprKind::ScalarVar("_".into()),
                                    line,
                                }),
                                pattern: parts[2].to_string(),
                                replacement: parts[3].to_string(),
                                flags: parts.get(4).unwrap_or(&"").to_string(),
                            },
                            line,
                        });
                    }
                    if parts.len() >= 4 && parts[1] == "tr" {
                        return Ok(Expr {
                            kind: ExprKind::Transliterate {
                                expr: Box::new(Expr {
                                    kind: ExprKind::ScalarVar("_".into()),
                                    line,
                                }),
                                from: parts[2].to_string(),
                                to: parts[3].to_string(),
                                flags: parts.get(4).unwrap_or(&"").to_string(),
                            },
                            line,
                        });
                    }
                    return Err(self.syntax_err("Unexpected encoded token", line));
                }
                self.parse_named_expr(name)
            }

            tok => Err(self.syntax_err(format!("Unexpected token {:?}", tok), line)),
        }
    }

    fn parse_named_expr(&mut self, mut name: String) -> PerlResult<Expr> {
        let line = self.peek_line();
        self.advance(); // consume the ident
        while self.eat(&Token::PackageSep) {
            match self.advance() {
                (Token::Ident(part), _) => {
                    name = format!("{}::{}", name, part);
                }
                (tok, err_line) => {
                    return Err(self.syntax_err(
                        format!("Expected identifier after `::`, got {:?}", tok),
                        err_line,
                    ));
                }
            }
        }

        match name.as_str() {
            "__FILE__" => Ok(Expr {
                kind: ExprKind::MagicConst(MagicConstKind::File),
                line,
            }),
            "__LINE__" => Ok(Expr {
                kind: ExprKind::MagicConst(MagicConstKind::Line),
                line,
            }),
            "print" => self.parse_print_like(|h, a| ExprKind::Print { handle: h, args: a }),
            "say" => self.parse_print_like(|h, a| ExprKind::Say { handle: h, args: a }),
            "printf" => self.parse_print_like(|h, a| ExprKind::Printf { handle: h, args: a }),
            "die" => {
                let args = self.parse_list_until_terminator()?;
                Ok(Expr {
                    kind: ExprKind::Die(args),
                    line,
                })
            }
            "warn" => {
                let args = self.parse_list_until_terminator()?;
                Ok(Expr {
                    kind: ExprKind::Warn(args),
                    line,
                })
            }
            "chomp" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Chomp(Box::new(a)),
                    line,
                })
            }
            "chop" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Chop(Box::new(a)),
                    line,
                })
            }
            "length" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Length(Box::new(a)),
                    line,
                })
            }
            "defined" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Defined(Box::new(a)),
                    line,
                })
            }
            "ref" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Ref(Box::new(a)),
                    line,
                })
            }
            "undef" => {
                if matches!(
                    self.peek(),
                    Token::ScalarVar(_) | Token::ArrayVar(_) | Token::HashVar(_)
                ) {
                    let _ = self.advance();
                }
                Ok(Expr {
                    kind: ExprKind::Undef,
                    line,
                })
            }
            "scalar" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::ScalarContext(Box::new(a)),
                    line,
                })
            }
            "abs" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Abs(Box::new(a)),
                    line,
                })
            }
            "int" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Int(Box::new(a)),
                    line,
                })
            }
            "sqrt" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Sqrt(Box::new(a)),
                    line,
                })
            }
            "sin" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Sin(Box::new(a)),
                    line,
                })
            }
            "cos" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Cos(Box::new(a)),
                    line,
                })
            }
            "atan2" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(self.syntax_err("atan2 requires two arguments", line));
                }
                Ok(Expr {
                    kind: ExprKind::Atan2 {
                        y: Box::new(args[0].clone()),
                        x: Box::new(args[1].clone()),
                    },
                    line,
                })
            }
            "exp" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Exp(Box::new(a)),
                    line,
                })
            }
            "log" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Log(Box::new(a)),
                    line,
                })
            }
            "rand" => {
                if matches!(
                    self.peek(),
                    Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::Comma
                ) {
                    Ok(Expr {
                        kind: ExprKind::Rand(None),
                        line,
                    })
                } else if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::Rand(None),
                            line,
                        })
                    } else {
                        let a = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        Ok(Expr {
                            kind: ExprKind::Rand(Some(Box::new(a))),
                            line,
                        })
                    }
                } else {
                    let a = self.parse_one_arg()?;
                    Ok(Expr {
                        kind: ExprKind::Rand(Some(Box::new(a))),
                        line,
                    })
                }
            }
            "srand" => {
                if matches!(
                    self.peek(),
                    Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::Comma
                ) {
                    Ok(Expr {
                        kind: ExprKind::Srand(None),
                        line,
                    })
                } else if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::Srand(None),
                            line,
                        })
                    } else {
                        let a = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        Ok(Expr {
                            kind: ExprKind::Srand(Some(Box::new(a))),
                            line,
                        })
                    }
                } else {
                    let a = self.parse_one_arg()?;
                    Ok(Expr {
                        kind: ExprKind::Srand(Some(Box::new(a))),
                        line,
                    })
                }
            }
            "hex" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Hex(Box::new(a)),
                    line,
                })
            }
            "oct" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Oct(Box::new(a)),
                    line,
                })
            }
            "chr" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Chr(Box::new(a)),
                    line,
                })
            }
            "ord" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Ord(Box::new(a)),
                    line,
                })
            }
            "lc" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Lc(Box::new(a)),
                    line,
                })
            }
            "uc" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Uc(Box::new(a)),
                    line,
                })
            }
            "lcfirst" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Lcfirst(Box::new(a)),
                    line,
                })
            }
            "ucfirst" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Ucfirst(Box::new(a)),
                    line,
                })
            }
            "fc" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Fc(Box::new(a)),
                    line,
                })
            }
            "crypt" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(self.syntax_err("crypt requires two arguments", line));
                }
                Ok(Expr {
                    kind: ExprKind::Crypt {
                        plaintext: Box::new(args[0].clone()),
                        salt: Box::new(args[1].clone()),
                    },
                    line,
                })
            }
            "pos" => {
                if matches!(
                    self.peek(),
                    Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::Comma
                ) {
                    Ok(Expr {
                        kind: ExprKind::Pos(None),
                        line,
                    })
                } else if matches!(self.peek(), Token::Assign) {
                    // Perl: `pos = EXPR` is `pos($_) = EXPR` (Text::Balanced `_eb_delims`).
                    self.advance();
                    let rhs = self.parse_assign_expr()?;
                    Ok(Expr {
                        kind: ExprKind::Assign {
                            target: Box::new(Expr {
                                kind: ExprKind::Pos(Some(Box::new(Expr {
                                    kind: ExprKind::ScalarVar("_".into()),
                                    line,
                                }))),
                                line,
                            }),
                            value: Box::new(rhs),
                        },
                        line,
                    })
                } else if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::Pos(None),
                            line,
                        })
                    } else {
                        let a = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        Ok(Expr {
                            kind: ExprKind::Pos(Some(Box::new(a))),
                            line,
                        })
                    }
                } else {
                    let saved = self.pos;
                    let subj = self.parse_unary()?;
                    if matches!(self.peek(), Token::Assign) {
                        self.advance();
                        let rhs = self.parse_assign_expr()?;
                        Ok(Expr {
                            kind: ExprKind::Assign {
                                target: Box::new(Expr {
                                    kind: ExprKind::Pos(Some(Box::new(subj))),
                                    line,
                                }),
                                value: Box::new(rhs),
                            },
                            line,
                        })
                    } else {
                        self.pos = saved;
                        let a = self.parse_one_arg()?;
                        Ok(Expr {
                            kind: ExprKind::Pos(Some(Box::new(a))),
                            line,
                        })
                    }
                }
            }
            "study" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Study(Box::new(a)),
                    line,
                })
            }
            "push" => {
                let args = self.parse_builtin_args()?;
                let (first, rest) = args
                    .split_first()
                    .ok_or_else(|| self.syntax_err("push requires arguments", line))?;
                Ok(Expr {
                    kind: ExprKind::Push {
                        array: Box::new(first.clone()),
                        values: rest.to_vec(),
                    },
                    line,
                })
            }
            "pop" => {
                let a = self.parse_one_arg_or_argv()?;
                Ok(Expr {
                    kind: ExprKind::Pop(Box::new(a)),
                    line,
                })
            }
            "shift" => {
                let a = self.parse_one_arg_or_argv()?;
                Ok(Expr {
                    kind: ExprKind::Shift(Box::new(a)),
                    line,
                })
            }
            "unshift" => {
                let args = self.parse_builtin_args()?;
                let (first, rest) = args
                    .split_first()
                    .ok_or_else(|| self.syntax_err("unshift requires arguments", line))?;
                Ok(Expr {
                    kind: ExprKind::Unshift {
                        array: Box::new(first.clone()),
                        values: rest.to_vec(),
                    },
                    line,
                })
            }
            "splice" => {
                let args = self.parse_builtin_args()?;
                let mut iter = args.into_iter();
                let array = Box::new(
                    iter.next()
                        .ok_or_else(|| self.syntax_err("splice requires arguments", line))?,
                );
                let offset = iter.next().map(Box::new);
                let length = iter.next().map(Box::new);
                let replacement: Vec<Expr> = iter.collect();
                Ok(Expr {
                    kind: ExprKind::Splice {
                        array,
                        offset,
                        length,
                        replacement,
                    },
                    line,
                })
            }
            "delete" => {
                let a = self.parse_postfix()?;
                Ok(Expr {
                    kind: ExprKind::Delete(Box::new(a)),
                    line,
                })
            }
            "exists" => {
                let a = self.parse_postfix()?;
                Ok(Expr {
                    kind: ExprKind::Exists(Box::new(a)),
                    line,
                })
            }
            "keys" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Keys(Box::new(a)),
                    line,
                })
            }
            "values" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Values(Box::new(a)),
                    line,
                })
            }
            "each" => {
                // `each(%hash)` / `each(@array)` — hash/array iterator
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Each(Box::new(a)),
                    line,
                })
            }
            "fore" => {
                // `fore { BLOCK } LIST` — forEach expression (pipe-forward friendly)
                if matches!(self.peek(), Token::LBrace) {
                    let (block, list) = self.parse_block_list()?;
                    Ok(Expr {
                        kind: ExprKind::ForEachExpr {
                            block,
                            list: Box::new(list),
                        },
                        line,
                    })
                } else if self.in_pipe_rhs() {
                    // `|> fore say` — blockless pipe form: wrap EXPR into a synthetic block
                    let expr = self.parse_assign_expr_stop_at_pipe()?;
                    let expr = Self::lift_bareword_to_topic_call(expr);
                    let block = vec![Statement {
                        label: None,
                        kind: StmtKind::Expression(expr),
                        line,
                    }];
                    let list = self.pipe_placeholder_list(line);
                    Ok(Expr {
                        kind: ExprKind::ForEachExpr {
                            block,
                            list: Box::new(list),
                        },
                        line,
                    })
                } else {
                    // `fore EXPR, LIST` — comma form
                    let expr = self.parse_assign_expr()?;
                    let expr = Self::lift_bareword_to_topic_call(expr);
                    self.expect(&Token::Comma)?;
                    let list_parts = self.parse_list_until_terminator()?;
                    let list_expr = if list_parts.len() == 1 {
                        list_parts.into_iter().next().unwrap()
                    } else {
                        Expr {
                            kind: ExprKind::List(list_parts),
                            line,
                        }
                    };
                    let block = vec![Statement {
                        label: None,
                        kind: StmtKind::Expression(expr),
                        line,
                    }];
                    Ok(Expr {
                        kind: ExprKind::ForEachExpr {
                            block,
                            list: Box::new(list_expr),
                        },
                        line,
                    })
                }
            }
            "reverse" | "reversed" => {
                // On the RHS of `|>`, the operand is supplied by the piped LHS.
                let a = if self.in_pipe_rhs()
                    && matches!(
                        self.peek(),
                        Token::Semicolon
                            | Token::RBrace
                            | Token::RParen
                            | Token::Eof
                            | Token::PipeForward
                    ) {
                    self.pipe_placeholder_list(line)
                } else {
                    self.parse_one_arg()?
                };
                Ok(Expr {
                    kind: ExprKind::ReverseExpr(Box::new(a)),
                    line,
                })
            }
            "join" => {
                let args = self.parse_builtin_args()?;
                if args.is_empty() {
                    return Err(self.syntax_err("join requires separator and list", line));
                }
                // `@list |> join(",")` — list slot is filled by the piped LHS.
                if args.len() < 2 && !self.in_pipe_rhs() {
                    return Err(self.syntax_err("join requires separator and list", line));
                }
                Ok(Expr {
                    kind: ExprKind::JoinExpr {
                        separator: Box::new(args[0].clone()),
                        list: Box::new(Expr {
                            kind: ExprKind::List(args[1..].to_vec()),
                            line,
                        }),
                    },
                    line,
                })
            }
            "split" => {
                let args = self.parse_builtin_args()?;
                let pattern = args.first().cloned().unwrap_or(Expr {
                    kind: ExprKind::String(" ".into()),
                    line,
                });
                let string = args.get(1).cloned().unwrap_or(Expr {
                    kind: ExprKind::ScalarVar("_".into()),
                    line,
                });
                let limit = args.get(2).cloned().map(Box::new);
                Ok(Expr {
                    kind: ExprKind::SplitExpr {
                        pattern: Box::new(pattern),
                        string: Box::new(string),
                        limit,
                    },
                    line,
                })
            }
            "substr" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Substr {
                        string: Box::new(args[0].clone()),
                        offset: Box::new(args[1].clone()),
                        length: args.get(2).cloned().map(Box::new),
                        replacement: args.get(3).cloned().map(Box::new),
                    },
                    line,
                })
            }
            "index" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Index {
                        string: Box::new(args[0].clone()),
                        substr: Box::new(args[1].clone()),
                        position: args.get(2).cloned().map(Box::new),
                    },
                    line,
                })
            }
            "rindex" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Rindex {
                        string: Box::new(args[0].clone()),
                        substr: Box::new(args[1].clone()),
                        position: args.get(2).cloned().map(Box::new),
                    },
                    line,
                })
            }
            "sprintf" => {
                let args = self.parse_builtin_args()?;
                let (first, rest) = args
                    .split_first()
                    .ok_or_else(|| self.syntax_err("sprintf requires format", line))?;
                Ok(Expr {
                    kind: ExprKind::Sprintf {
                        format: Box::new(first.clone()),
                        args: rest.to_vec(),
                    },
                    line,
                })
            }
            "map" | "flat_map" => {
                let flatten_array_refs = name == "flat_map";
                if matches!(self.peek(), Token::LBrace) {
                    let (block, list) = self.parse_block_list()?;
                    Ok(Expr {
                        kind: ExprKind::MapExpr {
                            block,
                            list: Box::new(list),
                            flatten_array_refs,
                        },
                        line,
                    })
                } else {
                    let expr = self.parse_assign_expr_stop_at_pipe()?;
                    // Lift bareword to FuncCall($_) so `map sha512, @list`
                    // calls sha512($_) for each element instead of stringifying.
                    let expr = Self::lift_bareword_to_topic_call(expr);
                    let list_expr = if self.in_pipe_rhs()
                        && matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) {
                        self.pipe_placeholder_list(line)
                    } else {
                        self.expect(&Token::Comma)?;
                        let list_parts = self.parse_list_until_terminator()?;
                        if list_parts.len() == 1 {
                            list_parts.into_iter().next().unwrap()
                        } else {
                            Expr {
                                kind: ExprKind::List(list_parts),
                                line,
                            }
                        }
                    };
                    Ok(Expr {
                        kind: ExprKind::MapExprComma {
                            expr: Box::new(expr),
                            list: Box::new(list_expr),
                            flatten_array_refs,
                        },
                        line,
                    })
                }
            }
            "match" => self.parse_algebraic_match_expr(line),
            "grep" | "filter" | "find_all" => {
                if matches!(self.peek(), Token::LBrace) {
                    let (block, list) = self.parse_block_list()?;
                    Ok(Expr {
                        kind: ExprKind::GrepExpr {
                            block,
                            list: Box::new(list),
                        },
                        line,
                    })
                } else {
                    let expr = self.parse_assign_expr_stop_at_pipe()?;
                    if self.in_pipe_rhs()
                        && matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        )
                    {
                        // Pipe-RHS blockless form: `|> grep EXPR`
                        // For literals, desugar to `$_ eq/== EXPR` so
                        // `|> filter 't'` keeps only elements equal to 't'.
                        // For regexes, desugar to `$_ =~ EXPR`.
                        let list = self.pipe_placeholder_list(line);
                        let topic = Expr {
                            kind: ExprKind::ScalarVar("_".into()),
                            line,
                        };
                        let test = match &expr.kind {
                            ExprKind::Integer(_) | ExprKind::Float(_) => Expr {
                                kind: ExprKind::BinOp {
                                    op: BinOp::NumEq,
                                    left: Box::new(topic),
                                    right: Box::new(expr),
                                },
                                line,
                            },
                            ExprKind::String(_) | ExprKind::InterpolatedString(_) => Expr {
                                kind: ExprKind::BinOp {
                                    op: BinOp::StrEq,
                                    left: Box::new(topic),
                                    right: Box::new(expr),
                                },
                                line,
                            },
                            ExprKind::Regex { .. } => Expr {
                                kind: ExprKind::BinOp {
                                    op: BinOp::BindMatch,
                                    left: Box::new(topic),
                                    right: Box::new(expr),
                                },
                                line,
                            },
                            _ => {
                                // Non-literal (e.g. `defined`): lift bareword to call
                                Self::lift_bareword_to_topic_call(expr)
                            }
                        };
                        let block = vec![Statement {
                            label: None,
                            kind: StmtKind::Expression(test),
                            line,
                        }];
                        Ok(Expr {
                            kind: ExprKind::GrepExpr {
                                block,
                                list: Box::new(list),
                            },
                            line,
                        })
                    } else {
                        let expr = Self::lift_bareword_to_topic_call(expr);
                        self.expect(&Token::Comma)?;
                        let list_parts = self.parse_list_until_terminator()?;
                        let list_expr = if list_parts.len() == 1 {
                            list_parts.into_iter().next().unwrap()
                        } else {
                            Expr {
                                kind: ExprKind::List(list_parts),
                                line,
                            }
                        };
                        Ok(Expr {
                            kind: ExprKind::GrepExprComma {
                                expr: Box::new(expr),
                                list: Box::new(list_expr),
                            },
                            line,
                        })
                    }
                }
            }
            "sort" => {
                use crate::ast::SortComparator;
                if matches!(self.peek(), Token::LBrace) {
                    let block = self.parse_block()?;
                    let _ = self.eat(&Token::Comma);
                    let list = if self.in_pipe_rhs()
                        && matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) {
                        self.pipe_placeholder_list(line)
                    } else {
                        self.parse_expression()?
                    };
                    Ok(Expr {
                        kind: ExprKind::SortExpr {
                            cmp: Some(SortComparator::Block(block)),
                            list: Box::new(list),
                        },
                        line,
                    })
                } else if matches!(self.peek(), Token::ScalarVar(ref v) if v == "a" || v == "b") {
                    // Blockless comparator: `sort $a <=> $b, @list`
                    let block = self.parse_block_or_bareword_cmp_block()?;
                    let _ = self.eat(&Token::Comma);
                    let list = if self.in_pipe_rhs()
                        && matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) {
                        self.pipe_placeholder_list(line)
                    } else {
                        self.parse_expression()?
                    };
                    Ok(Expr {
                        kind: ExprKind::SortExpr {
                            cmp: Some(SortComparator::Block(block)),
                            list: Box::new(list),
                        },
                        line,
                    })
                } else if matches!(self.peek(), Token::ScalarVar(_)) {
                    // `sort $coderef (LIST)` — comparator is first; list often parenthesized
                    self.suppress_indirect_paren_call =
                        self.suppress_indirect_paren_call.saturating_add(1);
                    let code = self.parse_assign_expr()?;
                    self.suppress_indirect_paren_call =
                        self.suppress_indirect_paren_call.saturating_sub(1);
                    let list = if matches!(self.peek(), Token::LParen) {
                        self.advance();
                        let e = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        e
                    } else {
                        self.parse_expression()?
                    };
                    Ok(Expr {
                        kind: ExprKind::SortExpr {
                            cmp: Some(SortComparator::Code(Box::new(code))),
                            list: Box::new(list),
                        },
                        line,
                    })
                } else if matches!(self.peek(), Token::Ident(ref name) if !Self::is_perl_keyword(name))
                {
                    // Blockless comparator via bare sub name: `sort my_cmp @list`
                    let block = self.parse_block_or_bareword_cmp_block()?;
                    let _ = self.eat(&Token::Comma);
                    let list = if self.in_pipe_rhs()
                        && matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) {
                        self.pipe_placeholder_list(line)
                    } else {
                        self.parse_expression()?
                    };
                    Ok(Expr {
                        kind: ExprKind::SortExpr {
                            cmp: Some(SortComparator::Block(block)),
                            list: Box::new(list),
                        },
                        line,
                    })
                } else {
                    // Bare `sort` with no comparator and no list: only allowed
                    // as the RHS of `|>`, where the list comes from the LHS.
                    let list = if self.in_pipe_rhs()
                        && matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) {
                        self.pipe_placeholder_list(line)
                    } else {
                        self.parse_expression()?
                    };
                    Ok(Expr {
                        kind: ExprKind::SortExpr {
                            cmp: None,
                            list: Box::new(list),
                        },
                        line,
                    })
                }
            }
            "reduce" | "fold" | "inject" => {
                let (block, list) = self.parse_block_list()?;
                Ok(Expr {
                    kind: ExprKind::ReduceExpr {
                        block,
                        list: Box::new(list),
                    },
                    line,
                })
            }
            // Parallel extensions
            "pmap" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PMapExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                        flat_outputs: false,
                        on_cluster: None,
                    },
                    line,
                })
            }
            "pmap_on" => {
                let (cluster, block, list, progress) =
                    self.parse_cluster_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PMapExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                        flat_outputs: false,
                        on_cluster: Some(Box::new(cluster)),
                    },
                    line,
                })
            }
            "pflat_map" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PMapExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                        flat_outputs: true,
                        on_cluster: None,
                    },
                    line,
                })
            }
            "pflat_map_on" => {
                let (cluster, block, list, progress) =
                    self.parse_cluster_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PMapExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                        flat_outputs: true,
                        on_cluster: Some(Box::new(cluster)),
                    },
                    line,
                })
            }
            "pmap_chunked" => {
                let chunk_size = self.parse_assign_expr()?;
                let block = self.parse_block_or_bareword_block()?;
                self.eat(&Token::Comma);
                let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PMapChunkedExpr {
                        chunk_size: Box::new(chunk_size),
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                    },
                    line,
                })
            }
            "pgrep" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PGrepExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                    },
                    line,
                })
            }
            "pfor" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PForExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                    },
                    line,
                })
            }
            "par_lines" => {
                // Use assign-level parsing so `par_lines $path, $cb` does not treat `$path, $cb`
                // as a single comma-list (`parse_expression` / comma-expr).
                let path = self.parse_assign_expr()?;
                self.expect(&Token::Comma)?;
                let callback = self.parse_assign_expr()?;
                let progress = if self.eat(&Token::Comma) {
                    match self.peek() {
                        Token::Ident(ref kw)
                            if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) =>
                        {
                            self.advance();
                            self.expect(&Token::FatArrow)?;
                            Some(Box::new(self.parse_assign_expr()?))
                        }
                        _ => {
                            return Err(self.syntax_err(
                                "par_lines: expected `progress => EXPR` after comma",
                                line,
                            ));
                        }
                    }
                } else {
                    None
                };
                Ok(Expr {
                    kind: ExprKind::ParLinesExpr {
                        path: Box::new(path),
                        callback: Box::new(callback),
                        progress,
                    },
                    line,
                })
            }
            "par_walk" => {
                let path = self.parse_assign_expr()?;
                self.expect(&Token::Comma)?;
                let callback = self.parse_assign_expr()?;
                let progress = if self.eat(&Token::Comma) {
                    match self.peek() {
                        Token::Ident(ref kw)
                            if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) =>
                        {
                            self.advance();
                            self.expect(&Token::FatArrow)?;
                            Some(Box::new(self.parse_assign_expr()?))
                        }
                        _ => {
                            return Err(self.syntax_err(
                                "par_walk: expected `progress => EXPR` after comma",
                                line,
                            ));
                        }
                    }
                } else {
                    None
                };
                Ok(Expr {
                    kind: ExprKind::ParWalkExpr {
                        path: Box::new(path),
                        callback: Box::new(callback),
                        progress,
                    },
                    line,
                })
            }
            "pwatch" => {
                let path = self.parse_assign_expr()?;
                self.expect(&Token::Comma)?;
                let callback = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::PwatchExpr {
                        path: Box::new(path),
                        callback: Box::new(callback),
                    },
                    line,
                })
            }
            "fan" => {
                // fan { BLOCK }            — no count, block body
                // fan COUNT { BLOCK }      — count + block body
                // fan EXPR;                — no count, blockless body (wrap EXPR as block)
                // fan COUNT EXPR;          — count + blockless body
                // Optional: `, progress => EXPR` or `progress => EXPR` (no comma before progress)
                let (count, block) = self.parse_fan_count_and_block(line)?;
                let progress = self.parse_fan_optional_progress("fan")?;
                Ok(Expr {
                    kind: ExprKind::FanExpr {
                        count,
                        block,
                        progress,
                        capture: false,
                    },
                    line,
                })
            }
            "fan_cap" => {
                let (count, block) = self.parse_fan_count_and_block(line)?;
                let progress = self.parse_fan_optional_progress("fan_cap")?;
                Ok(Expr {
                    kind: ExprKind::FanExpr {
                        count,
                        block,
                        progress,
                        capture: true,
                    },
                    line,
                })
            }
            "async" => {
                if !matches!(self.peek(), Token::LBrace) {
                    return Err(self.syntax_err("async must be followed by { BLOCK }", line));
                }
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::AsyncBlock { body: block },
                    line,
                })
            }
            "spawn" => {
                if !matches!(self.peek(), Token::LBrace) {
                    return Err(self.syntax_err("spawn must be followed by { BLOCK }", line));
                }
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::SpawnBlock { body: block },
                    line,
                })
            }
            "trace" => {
                if !matches!(self.peek(), Token::LBrace) {
                    return Err(self.syntax_err("trace must be followed by { BLOCK }", line));
                }
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::Trace { body: block },
                    line,
                })
            }
            "timer" => {
                let block = self.parse_block_or_bareword_block_no_args()?;
                Ok(Expr {
                    kind: ExprKind::Timer { body: block },
                    line,
                })
            }
            "bench" => {
                let block = self.parse_block_or_bareword_block_no_args()?;
                let times = Box::new(self.parse_expression()?);
                Ok(Expr {
                    kind: ExprKind::Bench { body: block, times },
                    line,
                })
            }
            "retry" => {
                let body = self.parse_block_or_bareword_block_no_args()?;
                match self.peek() {
                    Token::Ident(ref s) if s == "times" => {
                        self.advance();
                    }
                    _ => {
                        return Err(self.syntax_err("retry: expected `times =>` after block", line));
                    }
                }
                self.expect(&Token::FatArrow)?;
                let times = Box::new(self.parse_assign_expr()?);
                let mut backoff = RetryBackoff::None;
                if self.eat(&Token::Comma) {
                    match self.peek() {
                        Token::Ident(ref s) if s == "backoff" => {
                            self.advance();
                        }
                        _ => {
                            return Err(
                                self.syntax_err("retry: expected `backoff =>` after comma", line)
                            );
                        }
                    }
                    self.expect(&Token::FatArrow)?;
                    let Token::Ident(mode) = self.peek().clone() else {
                        return Err(self.syntax_err(
                            "retry: expected backoff mode (none, linear, exponential)",
                            line,
                        ));
                    };
                    backoff = match mode.as_str() {
                        "none" => RetryBackoff::None,
                        "linear" => RetryBackoff::Linear,
                        "exponential" => RetryBackoff::Exponential,
                        _ => {
                            return Err(
                                self.syntax_err(format!("retry: invalid backoff `{mode}`"), line)
                            );
                        }
                    };
                    self.advance();
                }
                Ok(Expr {
                    kind: ExprKind::RetryBlock {
                        body,
                        times,
                        backoff,
                    },
                    line,
                })
            }
            "rate_limit" => {
                self.expect(&Token::LParen)?;
                let max = Box::new(self.parse_expression()?);
                self.expect(&Token::Comma)?;
                let window = Box::new(self.parse_expression()?);
                self.expect(&Token::RParen)?;
                let body = self.parse_block_or_bareword_block_no_args()?;
                let slot = self.alloc_rate_limit_slot();
                Ok(Expr {
                    kind: ExprKind::RateLimitBlock {
                        slot,
                        max,
                        window,
                        body,
                    },
                    line,
                })
            }
            "every" => {
                self.expect(&Token::LParen)?;
                let interval = Box::new(self.parse_expression()?);
                self.expect(&Token::RParen)?;
                let body = self.parse_block_or_bareword_block_no_args()?;
                Ok(Expr {
                    kind: ExprKind::EveryBlock { interval, body },
                    line,
                })
            }
            "gen" => {
                if !matches!(self.peek(), Token::LBrace) {
                    return Err(self.syntax_err("gen must be followed by { BLOCK }", line));
                }
                let body = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::GenBlock { body },
                    line,
                })
            }
            "yield" => {
                let e = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::Yield(Box::new(e)),
                    line,
                })
            }
            "await" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Await(Box::new(a)),
                    line,
                })
            }
            "slurp" => {
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Slurp(Box::new(a)),
                    line,
                })
            }
            "capture" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Capture(Box::new(a)),
                    line,
                })
            }
            "fetch_url" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::FetchUrl(Box::new(a)),
                    line,
                })
            }
            "pchannel" => {
                let capacity = if self.eat(&Token::LParen) {
                    if matches!(self.peek(), Token::RParen) {
                        self.advance();
                        None
                    } else {
                        let e = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        Some(Box::new(e))
                    }
                } else {
                    None
                };
                Ok(Expr {
                    kind: ExprKind::Pchannel { capacity },
                    line,
                })
            }
            "psort" => {
                if matches!(self.peek(), Token::LBrace)
                    || matches!(self.peek(), Token::ScalarVar(ref v) if v == "a" || v == "b")
                    || matches!(self.peek(), Token::Ident(ref name) if !Self::is_perl_keyword(name))
                {
                    let block = self.parse_block_or_bareword_cmp_block()?;
                    self.eat(&Token::Comma);
                    let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                    Ok(Expr {
                        kind: ExprKind::PSortExpr {
                            cmp: Some(block),
                            list: Box::new(list),
                            progress: progress.map(Box::new),
                        },
                        line,
                    })
                } else {
                    let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                    Ok(Expr {
                        kind: ExprKind::PSortExpr {
                            cmp: None,
                            list: Box::new(list),
                            progress: progress.map(Box::new),
                        },
                        line,
                    })
                }
            }
            "preduce" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PReduceExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                    },
                    line,
                })
            }
            "preduce_init" => {
                let (init, block, list, progress) =
                    self.parse_init_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PReduceInitExpr {
                        init: Box::new(init),
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                    },
                    line,
                })
            }
            "pmap_reduce" => {
                let map_block = self.parse_block_or_bareword_block()?;
                // After the map block, expect either a `{ REDUCE }` block, or
                // after an eaten comma, a blockless reduce expr (`$a + $b`).
                let reduce_block = if matches!(self.peek(), Token::LBrace) {
                    self.parse_block()?
                } else {
                    // comma separates blockless map from blockless reduce
                    self.expect(&Token::Comma)?;
                    self.parse_block_or_bareword_cmp_block()?
                };
                self.eat(&Token::Comma);
                let line = self.peek_line();
                if let Token::Ident(ref kw) = self.peek().clone() {
                    if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                        self.advance();
                        self.expect(&Token::FatArrow)?;
                        let prog = self.parse_assign_expr()?;
                        return Ok(Expr {
                            kind: ExprKind::PMapReduceExpr {
                                map_block,
                                reduce_block,
                                list: Box::new(Expr {
                                    kind: ExprKind::List(vec![]),
                                    line,
                                }),
                                progress: Some(Box::new(prog)),
                            },
                            line,
                        });
                    }
                }
                if matches!(
                    self.peek(),
                    Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof
                ) {
                    return Ok(Expr {
                        kind: ExprKind::PMapReduceExpr {
                            map_block,
                            reduce_block,
                            list: Box::new(Expr {
                                kind: ExprKind::List(vec![]),
                                line,
                            }),
                            progress: None,
                        },
                        line,
                    });
                }
                let mut parts = vec![self.parse_assign_expr()?];
                loop {
                    if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                        break;
                    }
                    if matches!(
                        self.peek(),
                        Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof
                    ) {
                        break;
                    }
                    if let Token::Ident(ref kw) = self.peek().clone() {
                        if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                            self.advance();
                            self.expect(&Token::FatArrow)?;
                            let prog = self.parse_assign_expr()?;
                            return Ok(Expr {
                                kind: ExprKind::PMapReduceExpr {
                                    map_block,
                                    reduce_block,
                                    list: Box::new(merge_expr_list(parts)),
                                    progress: Some(Box::new(prog)),
                                },
                                line,
                            });
                        }
                    }
                    parts.push(self.parse_assign_expr()?);
                }
                Ok(Expr {
                    kind: ExprKind::PMapReduceExpr {
                        map_block,
                        reduce_block,
                        list: Box::new(merge_expr_list(parts)),
                        progress: None,
                    },
                    line,
                })
            }
            "puniq" => {
                if self.pipe_supplies_slurped_list_operand() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "puniq".to_string(),
                            args: vec![],
                        },
                        line,
                    });
                }
                let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                let mut args = vec![list];
                if let Some(p) = progress {
                    args.push(p);
                }
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "puniq".to_string(),
                        args,
                    },
                    line,
                })
            }
            "pfirst" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                let cr = Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body: block,
                    },
                    line,
                };
                let mut args = vec![cr, list];
                if let Some(p) = progress {
                    args.push(p);
                }
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "pfirst".to_string(),
                        args,
                    },
                    line,
                })
            }
            "pany" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                let cr = Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body: block,
                    },
                    line,
                };
                let mut args = vec![cr, list];
                if let Some(p) = progress {
                    args.push(p);
                }
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "pany".to_string(),
                        args,
                    },
                    line,
                })
            }
            "uniq" | "distinct" => {
                if self.pipe_supplies_slurped_list_operand() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: name.clone(),
                            args: vec![],
                        },
                        line,
                    });
                }
                let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                if progress.is_some() {
                    return Err(self.syntax_err(
                        "`progress =>` is not supported for uniq (use puniq for parallel + progress)",
                        line,
                    ));
                }
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: name.clone(),
                        args: vec![list],
                    },
                    line,
                })
            }
            "flatten" => {
                if self.pipe_supplies_slurped_list_operand() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "flatten".to_string(),
                            args: vec![],
                        },
                        line,
                    });
                }
                let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                if progress.is_some() {
                    return Err(self.syntax_err("`progress =>` is not supported for flatten", line));
                }
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "flatten".to_string(),
                        args: vec![list],
                    },
                    line,
                })
            }
            "set" => {
                if self.pipe_supplies_slurped_list_operand() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "set".to_string(),
                            args: vec![],
                        },
                        line,
                    });
                }
                let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                if progress.is_some() {
                    return Err(self.syntax_err("`progress =>` is not supported for set", line));
                }
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "set".to_string(),
                        args: vec![list],
                    },
                    line,
                })
            }
            "list_count" | "list_size" | "count" | "size" | "cnt" => {
                if self.pipe_supplies_slurped_list_operand() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: name.clone(),
                            args: vec![],
                        },
                        line,
                    });
                }
                let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                if progress.is_some() {
                    return Err(self.syntax_err(
                        "`progress =>` is not supported for list_count / list_size / count / size / cnt",
                        line,
                    ));
                }
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: name.clone(),
                        args: vec![list],
                    },
                    line,
                })
            }
            "shuffle" => {
                if self.pipe_supplies_slurped_list_operand() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "shuffle".to_string(),
                            args: vec![],
                        },
                        line,
                    });
                }
                let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                if progress.is_some() {
                    return Err(self.syntax_err("`progress =>` is not supported for shuffle", line));
                }
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "shuffle".to_string(),
                        args: vec![list],
                    },
                    line,
                })
            }
            "chunked" => {
                let mut parts = Vec::new();
                if self.eat(&Token::LParen) {
                    if !matches!(self.peek(), Token::RParen) {
                        parts.push(self.parse_assign_expr()?);
                        while self.eat(&Token::Comma) {
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            parts.push(self.parse_assign_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                } else {
                    // Paren-less `chunked N`: `|>` is a hard terminator, not
                    // an operator inside the arg (see
                    // `parse_assign_expr_stop_at_pipe`).
                    parts.push(self.parse_assign_expr_stop_at_pipe()?);
                    loop {
                        if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                            break;
                        }
                        if matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) {
                            break;
                        }
                        if self.peek_is_postfix_stmt_modifier_keyword() {
                            break;
                        }
                        parts.push(self.parse_assign_expr_stop_at_pipe()?);
                    }
                }
                if parts.len() == 1 {
                    let n = parts.pop().unwrap();
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "chunked".to_string(),
                            args: vec![n],
                        },
                        line,
                    });
                }
                if parts.is_empty() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "chunked".to_string(),
                            args: parts,
                        },
                        line,
                    });
                }
                if parts.len() == 2 {
                    let n = parts.pop().unwrap();
                    let list = parts.pop().unwrap();
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "chunked".to_string(),
                            args: vec![list, n],
                        },
                        line,
                    });
                }
                Err(self.syntax_err(
                    "chunked: use LIST |> chunked(N) or chunked((1,2,3), 2)",
                    line,
                ))
            }
            "windowed" => {
                let mut parts = Vec::new();
                if self.eat(&Token::LParen) {
                    if !matches!(self.peek(), Token::RParen) {
                        parts.push(self.parse_assign_expr()?);
                        while self.eat(&Token::Comma) {
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            parts.push(self.parse_assign_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                } else {
                    // Paren-less `windowed N`: same `|>`-terminator rule as
                    // `chunked` above.
                    parts.push(self.parse_assign_expr_stop_at_pipe()?);
                    loop {
                        if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                            break;
                        }
                        if matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) {
                            break;
                        }
                        if self.peek_is_postfix_stmt_modifier_keyword() {
                            break;
                        }
                        parts.push(self.parse_assign_expr_stop_at_pipe()?);
                    }
                }
                if parts.len() == 1 {
                    let n = parts.pop().unwrap();
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "windowed".to_string(),
                            args: vec![n],
                        },
                        line,
                    });
                }
                if parts.is_empty() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "windowed".to_string(),
                            args: parts,
                        },
                        line,
                    });
                }
                if parts.len() == 2 {
                    let n = parts.pop().unwrap();
                    let list = parts.pop().unwrap();
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "windowed".to_string(),
                            args: vec![list, n],
                        },
                        line,
                    });
                }
                Err(self.syntax_err(
                    "windowed: use LIST |> windowed(N) or windowed((1,2,3), 2)",
                    line,
                ))
            }
            "any" | "all" | "none" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                if progress.is_some() {
                    return Err(self.syntax_err(
                        "`progress =>` is not supported for any/all/none (use pany for parallel + progress)",
                        line,
                    ));
                }
                let cr = Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body: block,
                    },
                    line,
                };
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: name.clone(),
                        args: vec![cr, list],
                    },
                    line,
                })
            }
            // Ruby `detect` / `find` — same as `List::Util::first` (first element matching block).
            "first" | "detect" | "find" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                if progress.is_some() {
                    return Err(self.syntax_err(
                        "`progress =>` is not supported for first/detect/find (use pfirst for parallel + progress)",
                        line,
                    ));
                }
                let cr = Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body: block,
                    },
                    line,
                };
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "first".to_string(),
                        args: vec![cr, list],
                    },
                    line,
                })
            }
            "take_while" | "drop_while" | "tap" | "peek" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                if progress.is_some() {
                    return Err(self.syntax_err(
                        "`progress =>` is not supported for take_while/drop_while/tap/peek",
                        line,
                    ));
                }
                let cr = Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body: block,
                    },
                    line,
                };
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: name.to_string(),
                        args: vec![cr, list],
                    },
                    line,
                })
            }
            "group_by" | "chunk_by" => {
                if matches!(self.peek(), Token::LBrace) {
                    let (block, list) = self.parse_block_list()?;
                    let cr = Expr {
                        kind: ExprKind::CodeRef {
                            params: vec![],
                            body: block,
                        },
                        line,
                    };
                    Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: name.to_string(),
                            args: vec![cr, list],
                        },
                        line,
                    })
                } else {
                    let key_expr = self.parse_assign_expr()?;
                    self.expect(&Token::Comma)?;
                    let list_parts = self.parse_list_until_terminator()?;
                    let list_expr = if list_parts.len() == 1 {
                        list_parts.into_iter().next().unwrap()
                    } else {
                        Expr {
                            kind: ExprKind::List(list_parts),
                            line,
                        }
                    };
                    Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: name.to_string(),
                            args: vec![key_expr, list_expr],
                        },
                        line,
                    })
                }
            }
            "with_index" => {
                if self.pipe_supplies_slurped_list_operand() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "with_index".to_string(),
                            args: vec![],
                        },
                        line,
                    });
                }
                let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                if progress.is_some() {
                    return Err(
                        self.syntax_err("`progress =>` is not supported for with_index", line)
                    );
                }
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "with_index".to_string(),
                        args: vec![list],
                    },
                    line,
                })
            }
            "pcache" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PcacheExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                    },
                    line,
                })
            }
            "pselect" => {
                let paren = self.eat(&Token::LParen);
                let (receivers, timeout) = self.parse_comma_expr_list_with_timeout_tail(paren)?;
                if paren {
                    self.expect(&Token::RParen)?;
                }
                if receivers.is_empty() {
                    return Err(self.syntax_err("pselect needs at least one receiver", line));
                }
                Ok(Expr {
                    kind: ExprKind::PselectExpr {
                        receivers,
                        timeout: timeout.map(Box::new),
                    },
                    line,
                })
            }
            "watch" => {
                let path = self.parse_assign_expr()?;
                self.expect(&Token::Comma)?;
                let callback = self.parse_assign_expr()?;
                Ok(Expr {
                    kind: ExprKind::PwatchExpr {
                        path: Box::new(path),
                        callback: Box::new(callback),
                    },
                    line,
                })
            }
            "open" => {
                let paren = matches!(self.peek(), Token::LParen);
                if paren {
                    self.advance();
                }
                if matches!(self.peek(), Token::Ident(ref s) if s == "my") {
                    self.advance();
                    let name = self.parse_scalar_var_name()?;
                    self.expect(&Token::Comma)?;
                    let mode = self.parse_assign_expr()?;
                    let file = if self.eat(&Token::Comma) {
                        Some(self.parse_assign_expr()?)
                    } else {
                        None
                    };
                    if paren {
                        self.expect(&Token::RParen)?;
                    }
                    Ok(Expr {
                        kind: ExprKind::Open {
                            handle: Box::new(Expr {
                                kind: ExprKind::OpenMyHandle { name },
                                line,
                            }),
                            mode: Box::new(mode),
                            file: file.map(Box::new),
                        },
                        line,
                    })
                } else {
                    let args = if paren {
                        self.parse_arg_list()?
                    } else {
                        self.parse_list_until_terminator()?
                    };
                    if paren {
                        self.expect(&Token::RParen)?;
                    }
                    if args.len() < 2 {
                        return Err(self.syntax_err("open requires at least 2 arguments", line));
                    }
                    Ok(Expr {
                        kind: ExprKind::Open {
                            handle: Box::new(args[0].clone()),
                            mode: Box::new(args[1].clone()),
                            file: args.get(2).cloned().map(Box::new),
                        },
                        line,
                    })
                }
            }
            "close" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Close(Box::new(a)),
                    line,
                })
            }
            "opendir" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(self.syntax_err("opendir requires two arguments", line));
                }
                Ok(Expr {
                    kind: ExprKind::Opendir {
                        handle: Box::new(args[0].clone()),
                        path: Box::new(args[1].clone()),
                    },
                    line,
                })
            }
            "readdir" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Readdir(Box::new(a)),
                    line,
                })
            }
            "closedir" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Closedir(Box::new(a)),
                    line,
                })
            }
            "rewinddir" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Rewinddir(Box::new(a)),
                    line,
                })
            }
            "telldir" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Telldir(Box::new(a)),
                    line,
                })
            }
            "seekdir" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(self.syntax_err("seekdir requires two arguments", line));
                }
                Ok(Expr {
                    kind: ExprKind::Seekdir {
                        handle: Box::new(args[0].clone()),
                        position: Box::new(args[1].clone()),
                    },
                    line,
                })
            }
            "eof" => {
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::Eof(None),
                            line,
                        })
                    } else {
                        let a = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        Ok(Expr {
                            kind: ExprKind::Eof(Some(Box::new(a))),
                            line,
                        })
                    }
                } else {
                    Ok(Expr {
                        kind: ExprKind::Eof(None),
                        line,
                    })
                }
            }
            "system" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::System(args),
                    line,
                })
            }
            "exec" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Exec(args),
                    line,
                })
            }
            "eval" => {
                let a = if matches!(self.peek(), Token::LBrace) {
                    let block = self.parse_block()?;
                    Expr {
                        kind: ExprKind::CodeRef {
                            params: vec![],
                            body: block,
                        },
                        line,
                    }
                } else {
                    self.parse_one_arg()?
                };
                Ok(Expr {
                    kind: ExprKind::Eval(Box::new(a)),
                    line,
                })
            }
            "do" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Do(Box::new(a)),
                    line,
                })
            }
            "require" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Require(Box::new(a)),
                    line,
                })
            }
            "exit" => {
                if matches!(self.peek(), Token::Semicolon | Token::RBrace | Token::Eof) {
                    Ok(Expr {
                        kind: ExprKind::Exit(None),
                        line,
                    })
                } else {
                    let a = self.parse_one_arg()?;
                    Ok(Expr {
                        kind: ExprKind::Exit(Some(Box::new(a))),
                        line,
                    })
                }
            }
            "chdir" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Chdir(Box::new(a)),
                    line,
                })
            }
            "mkdir" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Mkdir {
                        path: Box::new(args[0].clone()),
                        mode: args.get(1).cloned().map(Box::new),
                    },
                    line,
                })
            }
            "unlink" | "rm" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Unlink(args),
                    line,
                })
            }
            "rename" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(self.syntax_err("rename requires two arguments", line));
                }
                Ok(Expr {
                    kind: ExprKind::Rename {
                        old: Box::new(args[0].clone()),
                        new: Box::new(args[1].clone()),
                    },
                    line,
                })
            }
            "chmod" => {
                let args = self.parse_builtin_args()?;
                if args.len() < 2 {
                    return Err(self.syntax_err("chmod requires mode and at least one file", line));
                }
                Ok(Expr {
                    kind: ExprKind::Chmod(args),
                    line,
                })
            }
            "chown" => {
                let args = self.parse_builtin_args()?;
                if args.len() < 3 {
                    return Err(
                        self.syntax_err("chown requires uid, gid, and at least one file", line)
                    );
                }
                Ok(Expr {
                    kind: ExprKind::Chown(args),
                    line,
                })
            }
            "stat" => {
                let args = self.parse_builtin_args()?;
                let arg = if args.len() == 1 {
                    args[0].clone()
                } else if args.is_empty() {
                    Expr { kind: ExprKind::ScalarVar("_".into()), line }
                } else {
                    return Err(self.syntax_err("stat requires zero or one argument", line));
                };
                Ok(Expr {
                    kind: ExprKind::Stat(Box::new(arg)),
                    line,
                })
            }
            "lstat" => {
                let args = self.parse_builtin_args()?;
                let arg = if args.len() == 1 {
                    args[0].clone()
                } else if args.is_empty() {
                    Expr { kind: ExprKind::ScalarVar("_".into()), line }
                } else {
                    return Err(self.syntax_err("lstat requires zero or one argument", line));
                };
                Ok(Expr {
                    kind: ExprKind::Lstat(Box::new(arg)),
                    line,
                })
            }
            "link" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(self.syntax_err("link requires two arguments", line));
                }
                Ok(Expr {
                    kind: ExprKind::Link {
                        old: Box::new(args[0].clone()),
                        new: Box::new(args[1].clone()),
                    },
                    line,
                })
            }
            "symlink" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(self.syntax_err("symlink requires two arguments", line));
                }
                Ok(Expr {
                    kind: ExprKind::Symlink {
                        old: Box::new(args[0].clone()),
                        new: Box::new(args[1].clone()),
                    },
                    line,
                })
            }
            "readlink" => {
                let args = self.parse_builtin_args()?;
                let arg = if args.len() == 1 {
                    args[0].clone()
                } else if args.is_empty() {
                    Expr { kind: ExprKind::ScalarVar("_".into()), line }
                } else {
                    return Err(self.syntax_err("readlink requires zero or one argument", line));
                };
                Ok(Expr {
                    kind: ExprKind::Readlink(Box::new(arg)),
                    line,
                })
            }
            "files" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Files(args),
                    line,
                })
            }
            "filesf" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Filesf(args),
                    line,
                })
            }
            "dirs" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Dirs(args),
                    line,
                })
            }
            "sym_links" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::SymLinks(args),
                    line,
                })
            }
            "sockets" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Sockets(args),
                    line,
                })
            }
            "pipes" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Pipes(args),
                    line,
                })
            }
            "block_devices" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::BlockDevices(args),
                    line,
                })
            }
            "char_devices" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::CharDevices(args),
                    line,
                })
            }
            "glob" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Glob(args),
                    line,
                })
            }
            "glob_par" => {
                let (args, progress) = self.parse_glob_par_or_par_sed_args()?;
                Ok(Expr {
                    kind: ExprKind::GlobPar { args, progress },
                    line,
                })
            }
            "par_sed" => {
                let (args, progress) = self.parse_glob_par_or_par_sed_args()?;
                Ok(Expr {
                    kind: ExprKind::ParSed { args, progress },
                    line,
                })
            }
            "bless" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Bless {
                        ref_expr: Box::new(args[0].clone()),
                        class: args.get(1).cloned().map(Box::new),
                    },
                    line,
                })
            }
            "caller" => {
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::Caller(None),
                            line,
                        })
                    } else {
                        let a = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        Ok(Expr {
                            kind: ExprKind::Caller(Some(Box::new(a))),
                            line,
                        })
                    }
                } else {
                    Ok(Expr {
                        kind: ExprKind::Caller(None),
                        line,
                    })
                }
            }
            "wantarray" => Ok(Expr {
                kind: ExprKind::Wantarray,
                line,
            }),
            "sub" => {
                // Anonymous sub — optional prototype `sub () { }` (e.g. Carp.pm `*X = sub () { 1 }`)
                let (params, _prototype) = self.parse_sub_sig_or_prototype_opt()?;
                let body = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::CodeRef { params, body },
                    line,
                })
            }
            _ => {
                // Generic function call
                // Check for fat arrow (bareword string in hash)
                if matches!(self.peek(), Token::FatArrow) {
                    return Ok(Expr {
                        kind: ExprKind::String(name),
                        line,
                    });
                }
                // Function call with optional parens
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&Token::RParen)?;
                    Ok(Expr {
                        kind: ExprKind::FuncCall { name, args },
                        line,
                    })
                } else if self.peek().is_term_start() {
                    // Perl allows func arg without parens
                    let args = self.parse_list_until_terminator()?;
                    Ok(Expr {
                        kind: ExprKind::FuncCall { name, args },
                        line,
                    })
                } else {
                    // No parens, no visible arguments — emit a Bareword.
                    // At runtime, Bareword tries sub resolution first (zero-arg
                    // call) and falls back to a string value.  perlrs extension
                    // contexts (pipe-forward, map/fore) lift Bareword → FuncCall
                    // with `$_` injection separately.
                    Ok(Expr {
                        kind: ExprKind::Bareword(name),
                        line,
                    })
                }
            }
        }
    }

    fn parse_print_like(
        &mut self,
        make: impl FnOnce(Option<String>, Vec<Expr>) -> ExprKind,
    ) -> PerlResult<Expr> {
        let line = self.peek_line();
        // Check for filehandle: print STDERR "msg"
        let handle = if let Token::Ident(ref h) = self.peek().clone() {
            if h.chars().all(|c| c.is_uppercase() || c == '_')
                && !matches!(self.peek(), Token::LParen)
            {
                let h = h.clone();
                let saved = self.pos;
                self.advance();
                // Verify next token is a term start (not operator)
                if self.peek().is_term_start()
                    || matches!(
                        self.peek(),
                        Token::DoubleString(_) | Token::BacktickString(_) | Token::SingleString(_)
                    )
                {
                    Some(h)
                } else {
                    self.pos = saved;
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let args = self.parse_list_until_terminator()?;
        Ok(Expr {
            kind: make(handle, args),
            line,
        })
    }

    fn parse_block_list(&mut self) -> PerlResult<(Block, Expr)> {
        let block = self.parse_block()?;
        self.eat(&Token::Comma);
        // On the RHS of `|>`, the list operand is supplied by the piped LHS
        // and will be substituted at desugar time — accept a placeholder when
        // we're at a terminator here.
        if self.in_pipe_rhs()
            && matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
            )
        {
            let line = self.peek_line();
            return Ok((block, self.pipe_placeholder_list(line)));
        }
        let list = self.parse_expression()?;
        Ok((block, list))
    }

    /// Comma-separated expressions with optional trailing `timeout => SECS` (for `pselect`).
    /// When `paren` is true, stops at `)` as well as normal terminators.
    fn parse_comma_expr_list_with_timeout_tail(
        &mut self,
        paren: bool,
    ) -> PerlResult<(Vec<Expr>, Option<Expr>)> {
        let mut parts = vec![self.parse_assign_expr()?];
        loop {
            if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                break;
            }
            if paren && matches!(self.peek(), Token::RParen) {
                break;
            }
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof
            ) {
                break;
            }
            if self.peek_is_postfix_stmt_modifier_keyword() {
                break;
            }
            if let Token::Ident(ref kw) = self.peek().clone() {
                if kw == "timeout" && matches!(self.peek_at(1), Token::FatArrow) {
                    self.advance();
                    self.expect(&Token::FatArrow)?;
                    let t = self.parse_assign_expr()?;
                    return Ok((parts, Some(t)));
                }
            }
            parts.push(self.parse_assign_expr()?);
        }
        Ok((parts, None))
    }

    /// `preduce_init EXPR, BLOCK, LIST` with optional `, progress => EXPR`.
    fn parse_init_block_then_list_optional_progress(
        &mut self,
    ) -> PerlResult<(Expr, Block, Expr, Option<Expr>)> {
        let init = self.parse_assign_expr()?;
        self.expect(&Token::Comma)?;
        let block = self.parse_block_or_bareword_block()?;
        self.eat(&Token::Comma);
        let line = self.peek_line();
        if let Token::Ident(ref kw) = self.peek().clone() {
            if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                self.advance();
                self.expect(&Token::FatArrow)?;
                let prog = self.parse_assign_expr()?;
                return Ok((
                    init,
                    block,
                    Expr {
                        kind: ExprKind::List(vec![]),
                        line,
                    },
                    Some(prog),
                ));
            }
        }
        if matches!(
            self.peek(),
            Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof
        ) {
            return Ok((
                init,
                block,
                Expr {
                    kind: ExprKind::List(vec![]),
                    line,
                },
                None,
            ));
        }
        let mut parts = vec![self.parse_assign_expr()?];
        loop {
            if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                break;
            }
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof
            ) {
                break;
            }
            if self.peek_is_postfix_stmt_modifier_keyword() {
                break;
            }
            if let Token::Ident(ref kw) = self.peek().clone() {
                if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                    self.advance();
                    self.expect(&Token::FatArrow)?;
                    let prog = self.parse_assign_expr()?;
                    return Ok((init, block, merge_expr_list(parts), Some(prog)));
                }
            }
            parts.push(self.parse_assign_expr()?);
        }
        Ok((init, block, merge_expr_list(parts), None))
    }

    /// `pmap_on CLUSTER { BLOCK } LIST [, progress => EXPR]` — cluster expr, then same tail as [`Self::parse_block_then_list_optional_progress`].
    fn parse_cluster_block_then_list_optional_progress(
        &mut self,
    ) -> PerlResult<(Expr, Block, Expr, Option<Expr>)> {
        let cluster = self.parse_assign_expr()?;
        let block = self.parse_block_or_bareword_block()?;
        self.eat(&Token::Comma);
        let line = self.peek_line();
        if let Token::Ident(ref kw) = self.peek().clone() {
            if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                self.advance();
                self.expect(&Token::FatArrow)?;
                let prog = self.parse_assign_expr_stop_at_pipe()?;
                return Ok((
                    cluster,
                    block,
                    Expr {
                        kind: ExprKind::List(vec![]),
                        line,
                    },
                    Some(prog),
                ));
            }
        }
        let empty_list_ok = matches!(
            self.peek(),
            Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
        ) || (self.in_pipe_rhs() && matches!(self.peek(), Token::Comma));
        if empty_list_ok {
            return Ok((
                cluster,
                block,
                Expr {
                    kind: ExprKind::List(vec![]),
                    line,
                },
                None,
            ));
        }
        let mut parts = vec![self.parse_assign_expr_stop_at_pipe()?];
        loop {
            if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                break;
            }
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
            ) {
                break;
            }
            if self.peek_is_postfix_stmt_modifier_keyword() {
                break;
            }
            if let Token::Ident(ref kw) = self.peek().clone() {
                if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                    self.advance();
                    self.expect(&Token::FatArrow)?;
                    let prog = self.parse_assign_expr_stop_at_pipe()?;
                    return Ok((cluster, block, merge_expr_list(parts), Some(prog)));
                }
            }
            parts.push(self.parse_assign_expr_stop_at_pipe()?);
        }
        Ok((cluster, block, merge_expr_list(parts), None))
    }

    /// Like [`parse_block_list`] but supports a trailing `, progress => EXPR`
    /// (`pmap`, `pgrep`, `preduce`, `pfor`, `pcache`, `psort`, …).
    ///
    /// Always invoked for paren-less trailing forms (`pmap { … } LIST`,
    /// `pmap { … } LIST, progress => EXPR`), so `|>` must terminate the whole
    /// stage — individual list parts and the progress value parse through
    /// [`Self::parse_assign_expr_stop_at_pipe`] to keep pipe-forward
    /// left-associative in `@a |> pmap { $_ * 2 }, progress => 0 |> join ','`.
    fn parse_block_then_list_optional_progress(
        &mut self,
    ) -> PerlResult<(Block, Expr, Option<Expr>)> {
        let block = self.parse_block_or_bareword_block()?;
        self.eat(&Token::Comma);
        let line = self.peek_line();
        if let Token::Ident(ref kw) = self.peek().clone() {
            if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                self.advance();
                self.expect(&Token::FatArrow)?;
                let prog = self.parse_assign_expr_stop_at_pipe()?;
                return Ok((
                    block,
                    Expr {
                        kind: ExprKind::List(vec![]),
                        line,
                    },
                    Some(prog),
                ));
            }
        }
        // An empty list operand is allowed when the next token terminates the
        // enclosing context. Inside a pipe-forward RHS, a trailing `,` also
        // counts — `foo(bar, @a |> pmap { $_ * 2 }, baz)`. `|>` is also a
        // terminator — left-associative chaining leaves the outer `|>` for
        // the enclosing pipe-forward loop.
        let empty_list_ok = matches!(
            self.peek(),
            Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
        ) || (self.in_pipe_rhs() && matches!(self.peek(), Token::Comma));
        if empty_list_ok {
            return Ok((
                block,
                Expr {
                    kind: ExprKind::List(vec![]),
                    line,
                },
                None,
            ));
        }
        let mut parts = vec![self.parse_assign_expr_stop_at_pipe()?];
        loop {
            if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                break;
            }
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
            ) {
                break;
            }
            if self.peek_is_postfix_stmt_modifier_keyword() {
                break;
            }
            if let Token::Ident(ref kw) = self.peek().clone() {
                if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                    self.advance();
                    self.expect(&Token::FatArrow)?;
                    let prog = self.parse_assign_expr_stop_at_pipe()?;
                    return Ok((block, merge_expr_list(parts), Some(prog)));
                }
            }
            parts.push(self.parse_assign_expr_stop_at_pipe()?);
        }
        Ok((block, merge_expr_list(parts), None))
    }

    /// Parse fan/fan_cap arguments: optional count + block or blockless expression.
    fn parse_fan_count_and_block(
        &mut self,
        _line: usize,
    ) -> PerlResult<(Option<Box<Expr>>, Block)> {
        // `fan { BLOCK }` — no count
        if matches!(self.peek(), Token::LBrace) {
            let block = self.parse_block()?;
            return Ok((None, block));
        }
        // Not a brace — first expr could be count or body
        let first = self.parse_postfix()?;
        if matches!(self.peek(), Token::LBrace) {
            // `fan COUNT { BLOCK }`
            let block = self.parse_block()?;
            Ok((Some(Box::new(first)), block))
        } else if matches!(self.peek(), Token::Semicolon | Token::RBrace | Token::Eof)
            || (matches!(self.peek(), Token::Comma)
                && matches!(self.peek_at(1), Token::Ident(ref kw) if kw == "progress"))
        {
            // `fan EXPR;` — no count, first is the body
            let block = self.bareword_to_no_arg_block(first);
            Ok((None, block))
        } else {
            // `fan COUNT EXPR;`
            let body = self.parse_block_or_bareword_block_no_args()?;
            Ok((Some(Box::new(first)), body))
        }
    }

    /// Wrap a parsed expression as a single-statement block, converting bare
    /// identifiers to zero-arg calls (`work` → `work()`).
    fn bareword_to_no_arg_block(&self, expr: Expr) -> Block {
        let line = expr.line;
        let body = match &expr.kind {
            ExprKind::Bareword(name) => Expr {
                kind: ExprKind::FuncCall {
                    name: name.clone(),
                    args: vec![],
                },
                line,
            },
            _ => expr,
        };
        vec![Statement::new(StmtKind::Expression(body), line)]
    }

    /// Parse either a `{ BLOCK }` or a bare expression and wrap it as a synthetic block.
    ///
    /// When the next token is `{`, delegates to [`Self::parse_block`].
    /// Otherwise parses a single postfix expression and wraps it as a call
    /// with `$_` as argument (for barewords) or a plain expression statement:
    ///
    /// - Bareword `foo` → `{ foo($_) }`
    /// - Other expr     → `{ EXPR }`
    fn parse_block_or_bareword_block(&mut self) -> PerlResult<Block> {
        if matches!(self.peek(), Token::LBrace) {
            return self.parse_block();
        }
        let line = self.peek_line();
        // A lone identifier followed by a list-terminator is a bare sub name:
        // `pmap double, @list` → block is `{ double($_) }`, rest is list.
        if let Token::Ident(ref name) = self.peek().clone() {
            if matches!(
                self.peek_at(1),
                Token::Comma | Token::Semicolon | Token::RBrace | Token::Eof | Token::PipeForward
            ) {
                let name = name.clone();
                self.advance();
                let body = Expr {
                    kind: ExprKind::FuncCall {
                        name,
                        args: vec![Expr {
                            kind: ExprKind::ScalarVar("_".to_string()),
                            line,
                        }],
                    },
                    line,
                };
                return Ok(vec![Statement::new(StmtKind::Expression(body), line)]);
            }
        }
        // Not a simple bareword — parse as expression (e.g. `$_ * 2`, `uc $_`)
        let expr = self.parse_assign_expr_stop_at_pipe()?;
        Ok(vec![Statement::new(StmtKind::Expression(expr), line)])
    }

    /// Like [`parse_block_or_bareword_block`] but for fan/timer/bench where the
    /// bare function takes no args (body runs stand-alone, not per-element).
    /// Only consumes a single bareword identifier — does NOT let `parse_primary`
    /// greedily swallow subsequent tokens as function arguments.
    fn parse_block_or_bareword_block_no_args(&mut self) -> PerlResult<Block> {
        if matches!(self.peek(), Token::LBrace) {
            return self.parse_block();
        }
        let line = self.peek_line();
        if let Token::Ident(ref name) = self.peek().clone() {
            if matches!(
                self.peek_at(1),
                Token::Comma
                    | Token::Semicolon
                    | Token::RBrace
                    | Token::Eof
                    | Token::PipeForward
                    | Token::Integer(_)
            ) {
                let name = name.clone();
                self.advance();
                let body = Expr {
                    kind: ExprKind::FuncCall { name, args: vec![] },
                    line,
                };
                return Ok(vec![Statement::new(StmtKind::Expression(body), line)]);
            }
        }
        let expr = self.parse_postfix()?;
        Ok(vec![Statement::new(StmtKind::Expression(expr), line)])
    }

    /// Returns true if `name` is a Perl keyword/builtin that should NOT be
    /// treated as a bare sub name (e.g. inside `sort`).
    fn is_perl_keyword(name: &str) -> bool {
        matches!(
            name,
            "keys" | "values" | "reverse" | "map" | "grep" | "sort" | "join"
            | "split" | "push" | "pop" | "shift" | "unshift" | "splice"
            | "chomp" | "chop" | "chr" | "ord" | "hex" | "oct" | "lc" | "uc"
            | "lcfirst" | "ucfirst" | "length" | "substr" | "index" | "rindex"
            | "sprintf" | "printf" | "print" | "say" | "die" | "warn" | "exit"
            | "return" | "my" | "our" | "local" | "do" | "eval" | "ref" | "defined"
            | "undef" | "scalar" | "wantarray" | "caller" | "delete" | "exists"
            | "each" | "pack" | "unpack" | "abs" | "int" | "sqrt" | "sin" | "cos"
            | "atan2" | "exp" | "log" | "rand" | "srand" | "time" | "localtime"
            | "gmtime" | "open" | "close" | "read" | "write" | "seek" | "tell"
            | "eof" | "binmode" | "stat" | "lstat" | "rename" | "unlink" | "mkdir"
            | "rmdir" | "chdir" | "chmod" | "chown" | "glob" | "opendir"
            | "readdir" | "closedir" | "system" | "exec" | "fork" | "wait"
            | "waitpid" | "kill" | "alarm" | "sleep" | "tie" | "untie" | "tied"
            | "bless" | "no" | "use" | "require" | "BEGIN" | "END" | "sub"
            | "if" | "unless" | "while" | "until" | "for" | "foreach" | "last"
            | "next" | "redo" | "goto" | "not" | "and" | "or" | "qw" | "qq" | "q"
            | "pos" | "quotemeta" | "study" | "chroot" | "fcntl" | "flock"
            | "ioctl" | "link" | "readlink" | "symlink" | "truncate"
            | "format" | "formline" | "getc" | "fileno" | "pipe" | "socket"
            | "connect" | "listen" | "accept" | "shutdown" | "send" | "recv"
            | "bind" | "setsockopt" | "getsockopt" | "select" | "vec"
            | "dump" | "dbmopen" | "dbmclose" | "getpwnam" | "getpwuid"
            | "getgrnam" | "getgrgid" | "gethostbyname" | "getnetbyname"
            | "getprotobyname" | "getservbyname" | "sethostent" | "setnetent"
            | "setprotoent" | "setservent" | "endpwent" | "endgrent"
            | "endhostent" | "endnetent" | "endprotoent" | "endservent"
            // perlrs extensions that produce lists or have special syntax:
            | "filter" | "fore" | "flat_map" | "reduce" | "fold" | "uniq"
            | "distinct" | "any" | "all" | "none" | "first" | "min" | "max"
            | "sum" | "product" | "zip" | "chunk" | "sliding_window" | "enumerate"
            | "reject" | "detect" | "find" | "find_all" | "collect" | "inject"
            | "compact" | "min_by" | "max_by" | "sort_by" | "tally"
            | "find_index" | "each_with_index" | "count" | "cnt" | "group_by" | "chunk_by"
            | "fan" | "fan_cap" | "pmap" | "pgrep" | "pfor" | "psort" | "preduce"
            | "pcache" | "timer" | "bench" | "eval_timeout" | "retry"
            | "rate_limit" | "every" | "pmap_chunked" | "pmap_on"
            | "pflat_map_on" | "preduce_init" | "pmap_reduce" | "heap"
        )
    }

    /// Parse a block OR a blockless comparison expression for sort/psort/heap.
    /// Blockless: `$a <=> $b` or `$a cmp $b` or any expression → wrapped as a Block.
    /// Also accepts a bare function name: `psort my_cmp, @list`.
    fn parse_block_or_bareword_cmp_block(&mut self) -> PerlResult<Block> {
        if matches!(self.peek(), Token::LBrace) {
            return self.parse_block();
        }
        let line = self.peek_line();
        // Bare sub name: `psort my_cmp, @list`
        if let Token::Ident(ref name) = self.peek().clone() {
            if matches!(
                self.peek_at(1),
                Token::Comma | Token::Semicolon | Token::RBrace | Token::Eof | Token::PipeForward
            ) {
                let name = name.clone();
                self.advance();
                let body = Expr {
                    kind: ExprKind::FuncCall {
                        name,
                        args: vec![
                            Expr {
                                kind: ExprKind::ScalarVar("a".to_string()),
                                line,
                            },
                            Expr {
                                kind: ExprKind::ScalarVar("b".to_string()),
                                line,
                            },
                        ],
                    },
                    line,
                };
                return Ok(vec![Statement::new(StmtKind::Expression(body), line)]);
            }
        }
        // Blockless expression: `$a <=> $b`, `$b cmp $a`, etc.
        let expr = self.parse_assign_expr_stop_at_pipe()?;
        Ok(vec![Statement::new(StmtKind::Expression(expr), line)])
    }

    /// After `fan` / `fan_cap` `{ BLOCK }`, optional `, progress => EXPR` or `progress => EXPR` (no comma).
    fn parse_fan_optional_progress(
        &mut self,
        which: &'static str,
    ) -> PerlResult<Option<Box<Expr>>> {
        let line = self.peek_line();
        if self.eat(&Token::Comma) {
            match self.peek() {
                Token::Ident(ref kw)
                    if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) =>
                {
                    self.advance();
                    self.expect(&Token::FatArrow)?;
                    return Ok(Some(Box::new(self.parse_assign_expr()?)));
                }
                _ => {
                    return Err(self.syntax_err(
                        format!("{which}: expected `progress => EXPR` after comma"),
                        line,
                    ));
                }
            }
        }
        if let Token::Ident(ref kw) = self.peek().clone() {
            if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                self.advance();
                self.expect(&Token::FatArrow)?;
                return Ok(Some(Box::new(self.parse_assign_expr()?)));
            }
        }
        Ok(None)
    }

    /// Comma-separated assign expressions with optional trailing `, progress => EXPR`
    /// (for `pmap_chunked`, `psort`, etc.).
    ///
    /// Paren-less — individual parts parse through
    /// [`Self::parse_assign_expr_stop_at_pipe`] so a trailing `|>` is left for
    /// the enclosing pipe-forward loop (left-associative chaining).
    fn parse_assign_expr_list_optional_progress(&mut self) -> PerlResult<(Expr, Option<Expr>)> {
        // On the RHS of `|>`, list-taking builtins may be written bare with no
        // operand — `@a |> uniq`, `@a |> flatten`, `foo(bar, @a |> psort)`, etc.
        // When the next token is a list-terminator, yield an empty placeholder
        // list; [`Self::pipe_forward_apply`] substitutes the piped LHS at
        // desugar time, so the placeholder is never evaluated.
        if self.in_pipe_rhs()
            && matches!(
                self.peek(),
                Token::Semicolon
                    | Token::RBrace
                    | Token::RParen
                    | Token::Eof
                    | Token::PipeForward
                    | Token::Comma
            )
        {
            return Ok((self.pipe_placeholder_list(self.peek_line()), None));
        }
        let mut parts = vec![self.parse_assign_expr_stop_at_pipe()?];
        loop {
            if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                break;
            }
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
            ) {
                break;
            }
            if self.peek_is_postfix_stmt_modifier_keyword() {
                break;
            }
            if let Token::Ident(ref kw) = self.peek().clone() {
                if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) {
                    self.advance();
                    self.expect(&Token::FatArrow)?;
                    let prog = self.parse_assign_expr_stop_at_pipe()?;
                    return Ok((merge_expr_list(parts), Some(prog)));
                }
            }
            parts.push(self.parse_assign_expr_stop_at_pipe()?);
        }
        Ok((merge_expr_list(parts), None))
    }

    fn parse_one_arg(&mut self) -> PerlResult<Expr> {
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            let expr = self.parse_expression()?;
            self.expect(&Token::RParen)?;
            Ok(expr)
        } else {
            self.parse_assign_expr_stop_at_pipe()
        }
    }

    fn parse_one_arg_or_default(&mut self) -> PerlResult<Expr> {
        if matches!(
            self.peek(),
            Token::Semicolon
                | Token::RBrace
                | Token::RParen
                | Token::Eof
                | Token::Comma
                | Token::PipeForward
        ) {
            Ok(Expr {
                kind: ExprKind::ScalarVar("_".into()),
                line: self.peek_line(),
            })
        } else {
            self.parse_one_arg()
        }
    }

    /// Array operand for `shift` / `pop`: default `@_`, or `shift(@a)` / `shift()` (empty parens = `@_`).
    fn parse_one_arg_or_argv(&mut self) -> PerlResult<Expr> {
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            if matches!(self.peek(), Token::RParen) {
                self.advance();
                return Ok(Expr {
                    kind: ExprKind::ArrayVar("_".into()),
                    line: self.peek_line(),
                });
            }
            let expr = self.parse_expression()?;
            self.expect(&Token::RParen)?;
            return Ok(expr);
        }
        if matches!(
            self.peek(),
            Token::Semicolon
                | Token::RBrace
                | Token::RParen
                | Token::Eof
                | Token::Comma
                | Token::PipeForward
        ) {
            Ok(Expr {
                kind: ExprKind::ArrayVar("_".into()),
                line: self.peek_line(),
            })
        } else {
            self.parse_assign_expr()
        }
    }

    fn parse_builtin_args(&mut self) -> PerlResult<Vec<Expr>> {
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            let args = self.parse_arg_list()?;
            self.expect(&Token::RParen)?;
            Ok(args)
        } else {
            self.parse_list_until_terminator()
        }
    }

    /// `progress` introducing the optional `progress => EXPR` suffix for `glob_par` / `par_sed`.
    #[inline]
    fn peek_is_glob_par_progress_kw(&self) -> bool {
        matches!(self.peek(), Token::Ident(ref kw) if kw == "progress")
            && matches!(self.peek_at(1), Token::FatArrow)
    }

    /// Pattern list for `glob_par` / `par_sed` inside `(...)`, stopping before `)` or `progress =>`.
    fn parse_pattern_list_until_rparen_or_progress(&mut self) -> PerlResult<Vec<Expr>> {
        let mut args = Vec::new();
        loop {
            if matches!(self.peek(), Token::RParen | Token::Eof) {
                break;
            }
            if self.peek_is_glob_par_progress_kw() {
                break;
            }
            args.push(self.parse_assign_expr()?);
            match self.peek() {
                Token::RParen => break,
                Token::Comma => {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        break;
                    }
                    if self.peek_is_glob_par_progress_kw() {
                        break;
                    }
                }
                _ => {
                    return Err(self.syntax_err(
                        "expected `,`, `)`, or `progress =>` after argument in `glob_par` / `par_sed`",
                        self.peek_line(),
                    ));
                }
            }
        }
        Ok(args)
    }

    /// Paren-less pattern list for `glob_par` / `par_sed`, stopping before stmt end or `progress =>`.
    fn parse_pattern_list_glob_par_bare(&mut self) -> PerlResult<Vec<Expr>> {
        let mut args = Vec::new();
        loop {
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof
            ) {
                break;
            }
            if self.peek_is_postfix_stmt_modifier_keyword() {
                break;
            }
            if self.peek_is_glob_par_progress_kw() {
                break;
            }
            args.push(self.parse_assign_expr()?);
            if !self.eat(&Token::Comma) {
                break;
            }
            if self.peek_is_glob_par_progress_kw() {
                break;
            }
        }
        Ok(args)
    }

    /// `glob_pat EXPR, ...` or `glob_pat(...)` plus optional `, progress => EXPR` / inner `progress =>`.
    fn parse_glob_par_or_par_sed_args(&mut self) -> PerlResult<(Vec<Expr>, Option<Box<Expr>>)> {
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            let args = self.parse_pattern_list_until_rparen_or_progress()?;
            let progress = if self.peek_is_glob_par_progress_kw() {
                self.advance();
                self.expect(&Token::FatArrow)?;
                Some(Box::new(self.parse_assign_expr()?))
            } else {
                None
            };
            self.expect(&Token::RParen)?;
            Ok((args, progress))
        } else {
            let args = self.parse_pattern_list_glob_par_bare()?;
            // Comma after the last pattern was consumed inside `parse_pattern_list_glob_par_bare`.
            let progress = if self.peek_is_glob_par_progress_kw() {
                self.advance();
                self.expect(&Token::FatArrow)?;
                Some(Box::new(self.parse_assign_expr()?))
            } else {
                None
            };
            Ok((args, progress))
        }
    }

    pub(crate) fn parse_arg_list(&mut self) -> PerlResult<Vec<Expr>> {
        let mut args = Vec::new();
        // Inside `(...)`, `|>` is a normal operator again (e.g. `f(2 |> g, 3)`),
        // so shadow any outer paren-less-arg suppression from
        // `no_pipe_forward_depth`. Saturating so nested mixes are safe.
        let saved_no_pf = self.no_pipe_forward_depth;
        self.no_pipe_forward_depth = 0;
        while !matches!(
            self.peek(),
            Token::RParen | Token::RBracket | Token::RBrace | Token::Eof
        ) {
            let arg = match self.parse_assign_expr() {
                Ok(e) => e,
                Err(err) => {
                    self.no_pipe_forward_depth = saved_no_pf;
                    return Err(err);
                }
            };
            args.push(arg);
            if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                break;
            }
        }
        self.no_pipe_forward_depth = saved_no_pf;
        Ok(args)
    }

    /// Arguments for `->name` / `->SUPER::name` **without** `(...)`. Unlike `die foo + 1`
    /// (unary `+` on `1` passed to `foo`), Perl treats `$o->meth + 5` as infix `+` after a
    /// no-arg method call; we must not consume that `+` as the start of a first argument.
    fn parse_method_arg_list_no_paren(&mut self) -> PerlResult<Vec<Expr>> {
        let mut args = Vec::new();
        loop {
            // `$g->next { ... }` — `{` starts the enclosing statement's block, not an anonymous
            // hash argument to `next` (paren-less method call has no args here).
            if args.is_empty() && matches!(self.peek(), Token::LBrace) {
                break;
            }
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof
            ) {
                break;
            }
            if let Token::Ident(ref kw) = self.peek().clone() {
                if matches!(
                    kw.as_str(),
                    "if" | "unless" | "while" | "until" | "for" | "foreach"
                ) {
                    break;
                }
            }
            // `foo($obj->meth, $x)` — comma separates *outer* args; it is not the start of a
            // paren-less method argument (those use spaces: `$obj->meth $a, $b`).
            if args.is_empty()
                && (self.peek_method_arg_infix_terminator() || matches!(self.peek(), Token::Comma))
            {
                break;
            }
            args.push(self.parse_assign_expr()?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(args)
    }

    /// Tokens that end a paren-less method arg list when no comma-separated args yet (infix on
    /// the whole `->meth` expression).
    fn peek_method_arg_infix_terminator(&self) -> bool {
        matches!(
            self.peek(),
            Token::Plus
                | Token::Minus
                | Token::Star
                | Token::Slash
                | Token::Percent
                | Token::Power
                | Token::Dot
                | Token::X
                | Token::NumEq
                | Token::NumNe
                | Token::NumLt
                | Token::NumGt
                | Token::NumLe
                | Token::NumGe
                | Token::Spaceship
                | Token::StrEq
                | Token::StrNe
                | Token::StrLt
                | Token::StrGt
                | Token::StrLe
                | Token::StrGe
                | Token::StrCmp
                | Token::LogAnd
                | Token::LogOr
                | Token::LogAndWord
                | Token::LogOrWord
                | Token::DefinedOr
                | Token::BitAnd
                | Token::BitOr
                | Token::BitXor
                | Token::ShiftLeft
                | Token::ShiftRight
                | Token::Range
                | Token::RangeExclusive
                | Token::BindMatch
                | Token::BindNotMatch
                | Token::Arrow
                // `($a->b) ? $a->c : $a->d` — `->c` must not slurp the ternary `:` / `?`.
                | Token::Question
                | Token::Colon
        )
    }

    fn parse_list_until_terminator(&mut self) -> PerlResult<Vec<Expr>> {
        let mut args = Vec::new();
        loop {
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
            ) {
                break;
            }
            // Check for postfix modifiers — stop before `expr for LIST` / `expr if COND` etc.
            if let Token::Ident(ref kw) = self.peek().clone() {
                if matches!(
                    kw.as_str(),
                    "if" | "unless" | "while" | "until" | "for" | "foreach"
                ) {
                    break;
                }
            }
            // Paren-less builtin args: `|>` terminates the whole call list, so
            // individual args must not absorb a following `|>`.
            args.push(self.parse_assign_expr_stop_at_pipe()?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(args)
    }

    fn try_parse_hash_ref(&mut self) -> PerlResult<Vec<(Expr, Expr)>> {
        let mut pairs = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            // Perl autoquotes a bareword immediately before `=>` (hash key), even for keywords like
            // `pos`, `bless`, `return` — see Text::Balanced `_failmsg` (`pos => $pos`).
            let line = self.peek_line();
            let key = if let Token::Ident(ref name) = self.peek().clone() {
                if matches!(self.peek_at(1), Token::FatArrow) {
                    self.advance();
                    Expr {
                        kind: ExprKind::String(name.clone()),
                        line,
                    }
                } else {
                    self.parse_assign_expr()?
                }
            } else {
                self.parse_assign_expr()?
            };
            // Expect => or , after key
            if self.eat(&Token::FatArrow) || self.eat(&Token::Comma) {
                let val = self.parse_assign_expr()?;
                pairs.push((key, val));
                self.eat(&Token::Comma);
            } else {
                return Err(self.syntax_err("Expected => or , in hash ref", key.line));
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(pairs)
    }

    fn parse_interpolated_string(&self, s: &str, line: usize) -> PerlResult<Expr> {
        // Parse $var and @var inside double-quoted strings
        let mut parts = Vec::new();
        let mut literal = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;

        'istr: while i < chars.len() {
            if chars[i] == LITERAL_DOLLAR_IN_DQUOTE {
                literal.push('$');
                i += 1;
                continue;
            }
            // "\\$x" in source: one backslash in the string, then interpolate $x (Perl double-quoted string).
            if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '$' {
                literal.push('\\');
                i += 1;
                // i now points at '$' — fall through to $ handling below
            }
            if chars[i] == '$' && i + 1 < chars.len() {
                if !literal.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                }
                i += 1; // past `$`
                        // Perl allows whitespace between `$` and the variable name (`$ foo` → `$foo`).
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(self.syntax_err("Final $ should be \\$ or $name", line));
                }
                // `$#name` — last index of `@name` (Perl `$#array`).
                if chars[i] == '#' {
                    i += 1;
                    let mut sname = String::from("#");
                    while i < chars.len()
                        && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == ':')
                    {
                        sname.push(chars[i]);
                        i += 1;
                    }
                    while i + 1 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' {
                        sname.push_str("::");
                        i += 2;
                        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                            sname.push(chars[i]);
                            i += 1;
                        }
                    }
                    parts.push(StringPart::ScalarVar(sname));
                    continue;
                }
                // `$$` — process id (Perl `$$`), only when the two `$` are adjacent (no whitespace
                // between) and the second `$` is not followed by a word character or digit (`$$x`
                // / `$$_` / `$$0` are `$` + `$x` / `$_` / `$0`).
                if chars[i] == '$' {
                    let next_c = chars.get(i + 1).copied();
                    let is_pid = match next_c {
                        None => true,
                        Some(c)
                            if !c.is_ascii_digit() && !matches!(c, 'A'..='Z' | 'a'..='z' | '_') =>
                        {
                            true
                        }
                        _ => false,
                    };
                    if is_pid {
                        parts.push(StringPart::ScalarVar("$$".to_string()));
                        i += 1; // consume second `$`
                        continue;
                    }
                    i += 1; // skip second `$` — same as a single `$` before the identifier
                }
                if chars[i] == '{' {
                    // ${expr}
                    i += 1;
                    let mut name = String::new();
                    while i < chars.len() && chars[i] != '}' {
                        name.push(chars[i]);
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 1;
                    }
                    parts.push(StringPart::ScalarVar(name));
                } else if chars[i] == '^' {
                    // `$^V`, `$^O`, … — name stored as `^V`, `^O`, … (see [`Interpreter::get_special_var`]).
                    let mut name = String::from("^");
                    i += 1;
                    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        name.push(chars[i]);
                        i += 1;
                    }
                    if i < chars.len() && chars[i] == '{' {
                        i += 1; // skip {
                        let mut key = String::new();
                        let mut depth = 1;
                        while i < chars.len() && depth > 0 {
                            if chars[i] == '{' {
                                depth += 1;
                            } else if chars[i] == '}' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            key.push(chars[i]);
                            i += 1;
                        }
                        if i < chars.len() {
                            i += 1;
                        }
                        let key_expr = if let Some(rest) = key.strip_prefix('$') {
                            Expr {
                                kind: ExprKind::ScalarVar(rest.to_string()),
                                line,
                            }
                        } else {
                            Expr {
                                kind: ExprKind::String(key),
                                line,
                            }
                        };
                        parts.push(StringPart::Expr(Expr {
                            kind: ExprKind::HashElement {
                                hash: name,
                                key: Box::new(key_expr),
                            },
                            line,
                        }));
                    } else if i < chars.len() && chars[i] == '[' {
                        i += 1;
                        let mut idx_str = String::new();
                        while i < chars.len() && chars[i] != ']' {
                            idx_str.push(chars[i]);
                            i += 1;
                        }
                        if i < chars.len() {
                            i += 1;
                        }
                        let idx_expr = if let Some(rest) = idx_str.strip_prefix('$') {
                            Expr {
                                kind: ExprKind::ScalarVar(rest.to_string()),
                                line,
                            }
                        } else if let Ok(n) = idx_str.parse::<i64>() {
                            Expr {
                                kind: ExprKind::Integer(n),
                                line,
                            }
                        } else {
                            Expr {
                                kind: ExprKind::String(idx_str),
                                line,
                            }
                        };
                        parts.push(StringPart::Expr(Expr {
                            kind: ExprKind::ArrayElement {
                                array: name,
                                index: Box::new(idx_expr),
                            },
                            line,
                        }));
                    } else {
                        parts.push(StringPart::ScalarVar(name));
                    }
                } else if chars[i].is_alphabetic() || chars[i] == '_' {
                    let mut name = String::new();
                    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        name.push(chars[i]);
                        i += 1;
                    }
                    // Check for hash access: $name{key} or array access: $name[idx]
                    if i < chars.len() && chars[i] == '{' {
                        // Hash element access in string: $hash{key}
                        i += 1; // skip {
                        let mut key = String::new();
                        let mut depth = 1;
                        while i < chars.len() && depth > 0 {
                            if chars[i] == '{' {
                                depth += 1;
                            } else if chars[i] == '}' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            key.push(chars[i]);
                            i += 1;
                        }
                        if i < chars.len() {
                            i += 1;
                        } // skip }
                          // Build a HashElement expression for the interpreter
                        let key_expr = if let Some(rest) = key.strip_prefix('$') {
                            Expr {
                                kind: ExprKind::ScalarVar(rest.to_string()),
                                line,
                            }
                        } else {
                            Expr {
                                kind: ExprKind::String(key),
                                line,
                            }
                        };
                        parts.push(StringPart::Expr(Expr {
                            kind: ExprKind::HashElement {
                                hash: name,
                                key: Box::new(key_expr),
                            },
                            line,
                        }));
                    } else if i < chars.len() && chars[i] == '[' {
                        // Array element access in string: $array[idx]
                        i += 1;
                        let mut idx_str = String::new();
                        while i < chars.len() && chars[i] != ']' {
                            idx_str.push(chars[i]);
                            i += 1;
                        }
                        if i < chars.len() {
                            i += 1;
                        }
                        let idx_expr = if let Some(rest) = idx_str.strip_prefix('$') {
                            Expr {
                                kind: ExprKind::ScalarVar(rest.to_string()),
                                line,
                            }
                        } else if let Ok(n) = idx_str.parse::<i64>() {
                            Expr {
                                kind: ExprKind::Integer(n),
                                line,
                            }
                        } else {
                            Expr {
                                kind: ExprKind::String(idx_str),
                                line,
                            }
                        };
                        parts.push(StringPart::Expr(Expr {
                            kind: ExprKind::ArrayElement {
                                array: name,
                                index: Box::new(idx_expr),
                            },
                            line,
                        }));
                    } else {
                        parts.push(StringPart::ScalarVar(name));
                    }
                } else if chars[i].is_ascii_digit() {
                    // $0 (program name), $1…$n (regexp captures). Perl disallows $01, $02, …
                    if chars[i] == '0' {
                        i += 1;
                        if i < chars.len() && chars[i].is_ascii_digit() {
                            return Err(self.syntax_err(
                                "Numeric variables with more than one digit may not start with '0'",
                                line,
                            ));
                        }
                        parts.push(StringPart::ScalarVar("0".into()));
                    } else {
                        let start = i;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                        parts.push(StringPart::ScalarVar(chars[start..i].iter().collect()));
                    }
                } else {
                    let c = chars[i];
                    let probe = c.to_string();
                    if Interpreter::is_special_scalar_name_for_get(&probe)
                        || matches!(c, '\'' | '`')
                    {
                        parts.push(StringPart::ScalarVar(probe));
                        i += 1;
                    } else {
                        literal.push('$');
                        literal.push(c);
                        i += 1;
                    }
                }
            } else if chars[i] == '@' && i + 1 < chars.len() {
                let next = chars[i + 1];
                // `@$aref` / `@${expr}` — array dereference in interpolation (Perl `"@$r"` → elements of @$r).
                if next == '$' {
                    if !literal.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                    }
                    i += 1; // past `@`
                    debug_assert_eq!(chars[i], '$');
                    i += 1; // past `$`
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    if i >= chars.len() {
                        return Err(self.syntax_err(
                            "Expected variable or block after `@$` in double-quoted string",
                            line,
                        ));
                    }
                    let inner_expr = if chars[i] == '{' {
                        i += 1;
                        let start = i;
                        let mut depth = 1usize;
                        while i < chars.len() && depth > 0 {
                            match chars[i] {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            i += 1;
                        }
                        if depth != 0 {
                            return Err(self.syntax_err(
                                "Unterminated `${ ... }` after `@` in double-quoted string",
                                line,
                            ));
                        }
                        let inner: String = chars[start..i].iter().collect();
                        i += 1; // closing `}`
                        parse_expression_from_str(inner.trim(), "-e")?
                    } else {
                        let mut name = String::new();
                        if chars[i] == '^' {
                            name.push('^');
                            i += 1;
                            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_')
                            {
                                name.push(chars[i]);
                                i += 1;
                            }
                        } else {
                            while i < chars.len()
                                && (chars[i].is_alphanumeric()
                                    || chars[i] == '_'
                                    || chars[i] == ':')
                            {
                                name.push(chars[i]);
                                i += 1;
                            }
                            while i + 1 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' {
                                name.push_str("::");
                                i += 2;
                                while i < chars.len()
                                    && (chars[i].is_alphanumeric() || chars[i] == '_')
                                {
                                    name.push(chars[i]);
                                    i += 1;
                                }
                            }
                        }
                        if name.is_empty() {
                            return Err(self.syntax_err(
                                "Expected identifier after `@$` in double-quoted string",
                                line,
                            ));
                        }
                        Expr {
                            kind: ExprKind::ScalarVar(name),
                            line,
                        }
                    };
                    parts.push(StringPart::Expr(Expr {
                        kind: ExprKind::Deref {
                            expr: Box::new(inner_expr),
                            kind: Sigil::Array,
                        },
                        line,
                    }));
                    continue 'istr;
                }
                if next == '{' {
                    if !literal.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                    }
                    i += 2; // `@{`
                    let start = i;
                    let mut depth = 1usize;
                    while i < chars.len() && depth > 0 {
                        match chars[i] {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    if depth != 0 {
                        return Err(
                            self.syntax_err("Unterminated @{ ... } in double-quoted string", line)
                        );
                    }
                    let inner: String = chars[start..i].iter().collect();
                    i += 1; // closing `}`
                    let inner_expr = parse_expression_from_str(inner.trim(), "-e")?;
                    parts.push(StringPart::Expr(Expr {
                        kind: ExprKind::Deref {
                            expr: Box::new(inner_expr),
                            kind: Sigil::Array,
                        },
                        line,
                    }));
                    continue 'istr;
                }
                if !(next.is_alphabetic() || next == '_' || next == '+' || next == '-') {
                    literal.push(chars[i]);
                    i += 1;
                } else {
                    if !literal.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                    }
                    i += 1;
                    let mut name = String::new();
                    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                        name.push(chars[i]);
                        i += 1;
                    } else {
                        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                            name.push(chars[i]);
                            i += 1;
                        }
                        while i + 1 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' {
                            name.push_str("::");
                            i += 2;
                            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_')
                            {
                                name.push(chars[i]);
                                i += 1;
                            }
                        }
                    }
                    if i < chars.len() && chars[i] == '[' {
                        i += 1;
                        let start_inner = i;
                        let mut depth = 1usize;
                        while i < chars.len() && depth > 0 {
                            match chars[i] {
                                '[' => depth += 1,
                                ']' => depth -= 1,
                                _ => {}
                            }
                            if depth == 0 {
                                let inner: String = chars[start_inner..i].iter().collect();
                                i += 1; // closing ]
                                let indices = parse_slice_indices_from_str(inner.trim(), "-e")?;
                                parts.push(StringPart::Expr(Expr {
                                    kind: ExprKind::ArraySlice {
                                        array: name.clone(),
                                        indices,
                                    },
                                    line,
                                }));
                                continue 'istr;
                            }
                            i += 1;
                        }
                        return Err(self.syntax_err(
                            "Unterminated [ in array slice inside quoted string",
                            line,
                        ));
                    }
                    parts.push(StringPart::ArrayVar(name));
                }
            } else {
                literal.push(chars[i]);
                i += 1;
            }
        }
        if !literal.is_empty() {
            parts.push(StringPart::Literal(literal));
        }

        if parts.len() == 1 {
            if let StringPart::Literal(s) = &parts[0] {
                return Ok(Expr {
                    kind: ExprKind::String(s.clone()),
                    line,
                });
            }
        }
        if parts.is_empty() {
            return Ok(Expr {
                kind: ExprKind::String(String::new()),
                line,
            });
        }

        Ok(Expr {
            kind: ExprKind::InterpolatedString(parts),
            line,
        })
    }

    fn expr_to_overload_key(&self, e: &Expr) -> PerlResult<String> {
        match &e.kind {
            ExprKind::String(s) => Ok(s.clone()),
            _ => Err(self.syntax_err(
                "overload key must be a string literal (e.g. '\"\"' or '+')",
                e.line,
            )),
        }
    }

    fn expr_to_overload_sub(&self, e: &Expr) -> PerlResult<String> {
        match &e.kind {
            ExprKind::String(s) => Ok(s.clone()),
            ExprKind::Integer(n) => Ok(n.to_string()),
            ExprKind::SubroutineRef(s) | ExprKind::SubroutineCodeRef(s) => Ok(s.clone()),
            _ => Err(self.syntax_err(
                "overload handler must be a string literal, number (e.g. fallback => 1), or \\&subname (method in current package)",
                e.line,
            )),
        }
    }
}

fn merge_expr_list(parts: Vec<Expr>) -> Expr {
    if parts.len() == 1 {
        parts.into_iter().next().unwrap()
    } else {
        let line = parts.first().map(|e| e.line).unwrap_or(0);
        Expr {
            kind: ExprKind::List(parts),
            line,
        }
    }
}

/// Parse a single expression from `s` (e.g. contents of `@{ ... }` inside a double-quoted string).
pub fn parse_expression_from_str(s: &str, file: &str) -> PerlResult<Expr> {
    let mut lexer = Lexer::new_with_file(s, file);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new_with_file(tokens, file);
    let e = parser.parse_expression()?;
    if !parser.at_eof() {
        return Err(parser.syntax_err(
            "Extra tokens in embedded string expression",
            parser.peek_line(),
        ));
    }
    Ok(e)
}

/// Comma-separated expressions on a `format` value line (below a picture line).
/// Parse `[ ... ]` contents for `@a[...]` (same rules as `parse_arg_list` / comma-separated indices).
pub fn parse_slice_indices_from_str(s: &str, file: &str) -> PerlResult<Vec<Expr>> {
    let mut lexer = Lexer::new_with_file(s, file);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new_with_file(tokens, file);
    parser.parse_arg_list()
}

pub fn parse_format_value_line(line: &str) -> PerlResult<Vec<Expr>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let mut lexer = Lexer::new(trimmed);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    let mut exprs = Vec::new();
    loop {
        if parser.at_eof() {
            break;
        }
        // Assignment-level expressions so `a, b` yields two fields (not one comma list).
        exprs.push(parser.parse_assign_expr()?);
        if parser.eat(&Token::Comma) {
            continue;
        }
        if !parser.at_eof() {
            return Err(parser.syntax_err("Extra tokens in format value line", parser.peek_line()));
        }
        break;
    }
    Ok(exprs)
}
