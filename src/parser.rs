use crate::ast::*;
use crate::error::{PerlError, PerlResult};
use crate::lexer::Lexer;
use crate::token::Token;

pub struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<(Token, usize)>) -> Self {
        Self { tokens, pos: 0 }
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
            Err(PerlError::syntax(
                format!("Expected {:?}, got {:?}", expected, tok),
                line,
            ))
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

        // Check for label
        let label = if let Token::Label(_) = self.peek() {
            if let (Token::Label(l), _) = self.advance() {
                Some(l)
            } else {
                None
            }
        } else {
            None
        };

        let mut stmt = match self.peek().clone() {
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
                            return Err(PerlError::syntax(
                                "Expected 'my' after 'frozen'",
                                self.peek_line(),
                            ));
                        }
                    } else {
                        return Err(PerlError::syntax(
                            "Expected 'my' after 'frozen'",
                            self.peek_line(),
                        ));
                    }
                }
                "typed" => {
                    self.advance();
                    if let Token::Ident(ref kw) = self.peek().clone() {
                        if kw == "my" {
                            self.parse_my_our_local("my", true)?
                        } else {
                            return Err(PerlError::syntax(
                                "Expected 'my' after 'typed'",
                                self.peek_line(),
                            ));
                        }
                    } else {
                        return Err(PerlError::syntax(
                            "Expected 'my' after 'typed'",
                            self.peek_line(),
                        ));
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
                    self.eat(&Token::Semicolon);
                    Statement {
                        label: None,
                        kind: StmtKind::Goto {
                            target: Box::new(target),
                        },
                        line,
                    }
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
                                self.eat(&Token::Semicolon);
                                Statement {
                                    label: label.clone(),
                                    kind: StmtKind::Expression(expr),
                                    line,
                                }
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
                            self.eat(&Token::Semicolon);
                            Statement {
                                label: label.clone(),
                                kind: StmtKind::Expression(expr),
                                line,
                            }
                        }
                    } else {
                        if let Some(expr) = self.try_parse_bareword_stmt_call() {
                            let stmt = self.maybe_postfix_modifier(expr)?;
                            self.eat(&Token::Semicolon);
                            stmt
                        } else {
                            let expr = self.parse_expression()?;
                            let stmt = self.maybe_postfix_modifier(expr)?;
                            self.eat(&Token::Semicolon);
                            stmt
                        }
                    }
                }
                _ => {
                    // `foo;` or `{ foo }` — bareword statement is a zero-arg call (topic `$_` at runtime).
                    if let Some(expr) = self.try_parse_bareword_stmt_call() {
                        let stmt = self.maybe_postfix_modifier(expr)?;
                        self.eat(&Token::Semicolon);
                        stmt
                    } else {
                        let expr = self.parse_expression()?;
                        let stmt = self.maybe_postfix_modifier(expr)?;
                        self.eat(&Token::Semicolon);
                        stmt
                    }
                }
            },
            Token::LBrace => {
                let block = self.parse_block()?;
                Statement {
                    label: None,
                    kind: StmtKind::Block(block),
                    line,
                }
            }
            _ => {
                let expr = self.parse_expression()?;
                let stmt = self.maybe_postfix_modifier(expr)?;
                self.eat(&Token::Semicolon);
                stmt
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
                | "cos"
                | "crypt"
                | "defined"
                | "delete"
                | "die"
                | "deque"
                | "do"
                | "each"
                | "eof"
                | "eval"
                | "exec"
                | "exists"
                | "exit"
                | "exp"
                | "fan"
                | "fan_cap"
                | "fc"
                | "fetch_url"
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
                | "match"
                | "mkdir"
                | "oct"
                | "open"
                | "opendir"
                | "ord"
                | "par_lines"
                | "par_walk"
                | "pcache"
                | "pchannel"
                | "pfor"
                | "pgrep"
                | "pipeline"
                | "pmap_chunked"
                | "pmap_reduce"
                | "pmap"
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
                | "ref"
                | "rename"
                | "require"
                | "reverse"
                | "rewinddir"
                | "rindex"
                | "say"
                | "scalar"
                | "seekdir"
                | "shift"
                | "sin"
                | "slurp"
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
                | "system"
                | "telldir"
                | "timer"
                | "trace"
                | "ucfirst"
                | "uc"
                | "undef"
                | "unlink"
                | "unshift"
                | "values"
                | "wantarray"
                | "warn"
                | "watch"
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
                return Err(PerlError::syntax(
                    "expected 'catch' after try block",
                    self.peek_line(),
                ));
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
                return Err(PerlError::syntax(
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
            self.expect(&Token::FatArrow)?;
            // Use assign-level parsing so commas separate arms, not `List` elements.
            let body = self.parse_assign_expr()?;
            arms.push(MatchArm { pattern, body });
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

    fn parse_match_array_pattern(&mut self) -> PerlResult<MatchPattern> {
        self.expect(&Token::LBracket)?;
        let mut elems = Vec::new();
        if self.eat(&Token::RBracket) {
            return Ok(MatchPattern::Array(vec![]));
        }
        loop {
            if matches!(self.peek(), Token::Star) {
                self.advance();
                elems.push(MatchArrayElem::Rest);
                self.eat(&Token::Comma);
                if !matches!(self.peek(), Token::RBracket) {
                    return Err(PerlError::syntax(
                        "`*` must be the last element in an array match pattern",
                        self.peek_line(),
                    ));
                }
                self.expect(&Token::RBracket)?;
                return Ok(MatchPattern::Array(elems));
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
                    return Err(PerlError::syntax(
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
        let timeout = self.parse_expression()?;
        let body = self.parse_block()?;
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
            (tok, line) => Err(PerlError::syntax(
                format!("Expected scalar variable, got {:?}", tok),
                line,
            )),
        }
    }

    fn parse_sub_decl(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'sub'
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => {
                return Err(PerlError::syntax(
                    format!("Expected sub name, got {:?}", tok),
                    line,
                ))
            }
        };
        // Optional prototype — capture text for `prototype` builtin
        let prototype = if matches!(self.peek(), Token::LParen) {
            self.advance();
            let mut s = String::new();
            while !matches!(self.peek(), Token::RParen | Token::Eof) {
                let (tok, _) = self.advance();
                match tok {
                    Token::Ident(i) => s.push_str(&i),
                    Token::Semicolon => s.push(';'),
                    Token::LParen => s.push('('),
                    Token::LBracket => s.push('['),
                    Token::RBracket => s.push(']'),
                    Token::Backslash => s.push('\\'),
                    Token::Comma => s.push(','),
                    Token::ScalarVar(v) => {
                        s.push('$');
                        s.push_str(&v);
                    }
                    Token::ArrayVar(v) => {
                        s.push('@');
                        s.push_str(&v);
                    }
                    Token::HashVar(v) => {
                        s.push('%');
                        s.push_str(&v);
                    }
                    Token::Plus => s.push('+'),
                    Token::Minus => s.push('-'),
                    _ => {}
                }
            }
            self.expect(&Token::RParen)?;
            Some(s)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(Statement {
            label: None,
            kind: StmtKind::SubDecl {
                name,
                params: vec![],
                body,
                prototype,
            },
            line,
        })
    }

    /// `struct Name { field => Type, ... }`
    fn parse_struct_decl(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // struct
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, err_line) => {
                return Err(PerlError::syntax(
                    format!("Expected struct name, got {:?}", tok),
                    err_line,
                ))
            }
        };
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let field_name = match self.advance() {
                (Token::Ident(n), _) => n,
                (tok, err_line) => {
                    return Err(PerlError::syntax(
                        format!("Expected field name, got {:?}", tok),
                        err_line,
                    ))
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

    fn parse_my_our_local(
        &mut self,
        keyword: &str,
        allow_type_annotation: bool,
    ) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'my'/'our'/'local'

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

        // Optional initializer: my $x = expr
        if self.eat(&Token::Assign) {
            let val = self.parse_expression()?;
            if decls.len() == 1 {
                decls[0].initializer = Some(val);
            } else {
                // List assignment: distribute to each decl from the list
                for decl in &mut decls {
                    decl.initializer = Some(val.clone());
                }
            }
        }

        self.eat(&Token::Semicolon);
        let kind = match keyword {
            "my" => StmtKind::My(decls),
            "mysync" => StmtKind::MySync(decls),
            "our" => StmtKind::Our(decls),
            "local" => StmtKind::Local(decls),
            _ => unreachable!(),
        };
        Ok(Statement {
            label: None,
            kind,
            line,
        })
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
                        return Err(PerlError::syntax(
                            format!("Expected identifier after *, got {:?}", tok),
                            l,
                        ));
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
                return Err(PerlError::syntax(
                    format!("Expected variable in declaration, got {:?}", tok),
                    line,
                ));
            }
        };
        if allow_type_annotation && self.eat(&Token::Colon) {
            let ty = self.parse_type_name()?;
            if decl.sigil != Sigil::Scalar {
                return Err(PerlError::syntax(
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
                _ => Err(PerlError::syntax(
                    format!("unknown type `{name}` (supported: Int, Str, Float)"),
                    line,
                )),
            },
            (tok, line) => Err(PerlError::syntax(
                format!("Expected type name after `:`, got {:?}", tok),
                line,
            )),
        }
    }

    fn parse_package(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'package'
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => {
                return Err(PerlError::syntax(
                    format!("Expected package name, got {:?}", tok),
                    line,
                ))
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
        let module = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => {
                return Err(PerlError::syntax(
                    format!("Expected module name after use, got {:?}", tok),
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
        if full_name == "overload" {
            let mut pairs = Vec::new();
            if !matches!(self.peek(), Token::Semicolon | Token::Eof) {
                loop {
                    if matches!(self.peek(), Token::Semicolon | Token::Eof) {
                        break;
                    }
                    let key_e = self.parse_assign_expr()?;
                    self.expect(&Token::FatArrow)?;
                    let val_e = self.parse_assign_expr()?;
                    let key = Self::expr_to_overload_key(&key_e)?;
                    let val = Self::expr_to_overload_sub(&val_e)?;
                    pairs.push((key, val));
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
            }
            self.eat(&Token::Semicolon);
            return Ok(Statement {
                label: None,
                kind: StmtKind::UseOverload { pairs },
                line,
            });
        }
        // Optional version or import list
        let mut imports = Vec::new();
        if !matches!(self.peek(), Token::Semicolon | Token::Eof) {
            // Could be a version number or import list
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

    fn parse_no(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'no'
        let module = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => {
                return Err(PerlError::syntax(
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
            _ => Ok(expr),
        }
    }

    fn parse_ternary(&mut self) -> PerlResult<Expr> {
        let expr = self.parse_or_word()?;
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
        self.parse_log_or()
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
        let left = self.parse_range()?;
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
                            Err(PerlError::syntax("Invalid regex binding", line))
                        }
                    }
                    _ => {
                        let rhs = self.parse_range()?;
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
                            Err(PerlError::syntax("Invalid regex binding after !~", line))
                        }
                    }
                    _ => {
                        let rhs = self.parse_range()?;
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

    fn parse_range(&mut self) -> PerlResult<Expr> {
        let left = self.parse_unary()?;
        if self.eat(&Token::Range) {
            let line = left.line;
            let right = self.parse_unary()?;
            return Ok(Expr {
                kind: ExprKind::Range {
                    from: Box::new(left),
                    to: Box::new(right),
                },
                line,
            });
        }
        Ok(left)
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
                // Unary `&name` (subroutine invocation / coderef); binary `&` is handled in `parse_bit_and`.
                self.advance();
                let name = match self.advance() {
                    (Token::Ident(n), _) => n,
                    (tok, l) => {
                        return Err(PerlError::syntax(
                            format!("Expected subroutine name after &, got {:?}", tok),
                            l,
                        ));
                    }
                };
                Ok(Expr {
                    kind: ExprKind::SubroutineRef(name),
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
                Ok(Expr {
                    kind: ExprKind::ScalarRef(Box::new(expr)),
                    line,
                })
            }
            Token::FileTest(op) => {
                self.advance();
                let expr = self.parse_unary()?;
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
                                        return Err(PerlError::syntax(
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
                                            return Err(PerlError::syntax(
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
                Token::LBracket if matches!(expr.kind, ExprKind::ScalarVar(_)) => {
                    // $array[index]
                    let line = expr.line;
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
                }
                Token::LBrace if matches!(expr.kind, ExprKind::ScalarVar(_)) => {
                    // $hash{key}
                    let line = expr.line;
                    if let ExprKind::ScalarVar(ref name) = expr.kind {
                        let name = name.clone();
                        self.advance();
                        let key = self.parse_expression()?;
                        self.expect(&Token::RBrace)?;
                        expr = Expr {
                            kind: ExprKind::HashElement {
                                hash: name,
                                key: Box::new(key),
                            },
                            line,
                        };
                    }
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
                // `x` tokenizes as `Token::X` (repeat op) — still a valid package/typeglob name.
                let mut full_name = match self.advance() {
                    (Token::Ident(n), _) => n,
                    (Token::X, _) => "x".to_string(),
                    (tok, l) => {
                        return Err(PerlError::syntax(
                            format!("Expected identifier after *, got {:?}", tok),
                            l,
                        ));
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
                            return Err(PerlError::syntax(
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
                Ok(self.parse_interpolated_string(&s, line))
            }
            Token::HereDoc(_, body) => {
                self.advance();
                Ok(self.parse_interpolated_string(&body, line))
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
                // Check for slice: @arr[...] or @hash{...}
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
                    return Err(PerlError::syntax("Unexpected encoded token", line));
                }
                self.parse_named_expr(name)
            }

            tok => Err(PerlError::syntax(
                format!("Unexpected token {:?}", tok),
                line,
            )),
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
                    return Err(PerlError::syntax(
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
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Defined(Box::new(a)),
                    line,
                })
            }
            "ref" => {
                let a = self.parse_one_arg()?;
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
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Sqrt(Box::new(a)),
                    line,
                })
            }
            "sin" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Sin(Box::new(a)),
                    line,
                })
            }
            "cos" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Cos(Box::new(a)),
                    line,
                })
            }
            "atan2" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(PerlError::syntax("atan2 requires two arguments", line));
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
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Exp(Box::new(a)),
                    line,
                })
            }
            "log" => {
                let a = self.parse_one_arg()?;
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
                    return Err(PerlError::syntax("crypt requires two arguments", line));
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
                    let a = self.parse_one_arg()?;
                    Ok(Expr {
                        kind: ExprKind::Pos(Some(Box::new(a))),
                        line,
                    })
                }
            }
            "study" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Study(Box::new(a)),
                    line,
                })
            }
            "push" => {
                let args = self.parse_builtin_args()?;
                let (first, rest) = args
                    .split_first()
                    .ok_or_else(|| PerlError::syntax("push requires arguments", line))?;
                Ok(Expr {
                    kind: ExprKind::Push {
                        array: Box::new(first.clone()),
                        values: rest.to_vec(),
                    },
                    line,
                })
            }
            "pop" => {
                let a = self.parse_one_arg()?;
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
                    .ok_or_else(|| PerlError::syntax("unshift requires arguments", line))?;
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
                        .ok_or_else(|| PerlError::syntax("splice requires arguments", line))?,
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
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Each(Box::new(a)),
                    line,
                })
            }
            "reverse" => {
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::ReverseExpr(Box::new(a)),
                    line,
                })
            }
            "join" => {
                let args = self.parse_builtin_args()?;
                if args.len() < 2 {
                    return Err(PerlError::syntax("join requires separator and list", line));
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
                    .ok_or_else(|| PerlError::syntax("sprintf requires format", line))?;
                Ok(Expr {
                    kind: ExprKind::Sprintf {
                        format: Box::new(first.clone()),
                        args: rest.to_vec(),
                    },
                    line,
                })
            }
            "map" => {
                let (block, list) = self.parse_block_list()?;
                Ok(Expr {
                    kind: ExprKind::MapExpr {
                        block,
                        list: Box::new(list),
                    },
                    line,
                })
            }
            "match" => self.parse_algebraic_match_expr(line),
            "grep" => {
                let (block, list) = self.parse_block_list()?;
                Ok(Expr {
                    kind: ExprKind::GrepExpr {
                        block,
                        list: Box::new(list),
                    },
                    line,
                })
            }
            "sort" => {
                // sort may have optional cmp block
                if matches!(self.peek(), Token::LBrace) {
                    let block = self.parse_block()?;
                    let _ = self.eat(&Token::Comma);
                    let list = self.parse_expression()?;
                    Ok(Expr {
                        kind: ExprKind::SortExpr {
                            cmp: Some(block),
                            list: Box::new(list),
                        },
                        line,
                    })
                } else {
                    let list = self.parse_expression()?;
                    Ok(Expr {
                        kind: ExprKind::SortExpr {
                            cmp: None,
                            list: Box::new(list),
                        },
                        line,
                    })
                }
            }
            "reduce" => {
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
                    },
                    line,
                })
            }
            "pmap_chunked" => {
                let chunk_size = self.parse_assign_expr()?;
                let block = self.parse_block()?;
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
                            return Err(PerlError::syntax(
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
                            return Err(PerlError::syntax(
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
                // fan COUNT { BLOCK }  |  fan { BLOCK }  (COUNT defaults to rayon thread pool size)
                // Optional: `, progress => EXPR` or `progress => EXPR` (no comma before progress)
                let count = if matches!(self.peek(), Token::LBrace) {
                    None
                } else {
                    Some(Box::new(self.parse_postfix()?))
                };
                let block = self.parse_block()?;
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
                let count = if matches!(self.peek(), Token::LBrace) {
                    None
                } else {
                    Some(Box::new(self.parse_postfix()?))
                };
                let block = self.parse_block()?;
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
                    return Err(PerlError::syntax(
                        "async must be followed by { BLOCK }",
                        line,
                    ));
                }
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::AsyncBlock { body: block },
                    line,
                })
            }
            "spawn" => {
                if !matches!(self.peek(), Token::LBrace) {
                    return Err(PerlError::syntax(
                        "spawn must be followed by { BLOCK }",
                        line,
                    ));
                }
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::SpawnBlock { body: block },
                    line,
                })
            }
            "trace" => {
                if !matches!(self.peek(), Token::LBrace) {
                    return Err(PerlError::syntax(
                        "trace must be followed by { BLOCK }",
                        line,
                    ));
                }
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::Trace { body: block },
                    line,
                })
            }
            "timer" => {
                if !matches!(self.peek(), Token::LBrace) {
                    return Err(PerlError::syntax(
                        "timer must be followed by { BLOCK }",
                        line,
                    ));
                }
                let block = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::Timer { body: block },
                    line,
                })
            }
            "bench" => {
                if !matches!(self.peek(), Token::LBrace) {
                    return Err(PerlError::syntax(
                        "bench must be followed by { BLOCK }",
                        line,
                    ));
                }
                let body = self.parse_block()?;
                let times = Box::new(self.parse_expression()?);
                Ok(Expr {
                    kind: ExprKind::Bench { body, times },
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
                let a = self.parse_one_arg()?;
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
                if matches!(self.peek(), Token::LBrace) {
                    let block = self.parse_block()?;
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
                let map_block = self.parse_block()?;
                let reduce_block = self.parse_block()?;
                self.eat(&Token::Comma);
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
                    return Err(PerlError::syntax(
                        "pselect needs at least one receiver",
                        line,
                    ));
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
                let args = self.parse_builtin_args()?;
                if args.len() < 2 {
                    return Err(PerlError::syntax(
                        "open requires at least 2 arguments",
                        line,
                    ));
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
                    return Err(PerlError::syntax("opendir requires two arguments", line));
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
                    return Err(PerlError::syntax("seekdir requires two arguments", line));
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
            "unlink" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Unlink(args),
                    line,
                })
            }
            "rename" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(PerlError::syntax("rename requires two arguments", line));
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
                    return Err(PerlError::syntax(
                        "chmod requires mode and at least one file",
                        line,
                    ));
                }
                Ok(Expr {
                    kind: ExprKind::Chmod(args),
                    line,
                })
            }
            "chown" => {
                let args = self.parse_builtin_args()?;
                if args.len() < 3 {
                    return Err(PerlError::syntax(
                        "chown requires uid, gid, and at least one file",
                        line,
                    ));
                }
                Ok(Expr {
                    kind: ExprKind::Chown(args),
                    line,
                })
            }
            "stat" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 1 {
                    return Err(PerlError::syntax("stat requires one argument", line));
                }
                Ok(Expr {
                    kind: ExprKind::Stat(Box::new(args[0].clone())),
                    line,
                })
            }
            "lstat" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 1 {
                    return Err(PerlError::syntax("lstat requires one argument", line));
                }
                Ok(Expr {
                    kind: ExprKind::Lstat(Box::new(args[0].clone())),
                    line,
                })
            }
            "link" => {
                let args = self.parse_builtin_args()?;
                if args.len() != 2 {
                    return Err(PerlError::syntax("link requires two arguments", line));
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
                    return Err(PerlError::syntax("symlink requires two arguments", line));
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
                if args.len() != 1 {
                    return Err(PerlError::syntax("readlink requires one argument", line));
                }
                Ok(Expr {
                    kind: ExprKind::Readlink(Box::new(args[0].clone())),
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
                let args = self.parse_builtin_args()?;
                let progress = if self.eat(&Token::Comma) {
                    match self.peek().clone() {
                        Token::Ident(ref kw)
                            if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) =>
                        {
                            self.advance();
                            self.expect(&Token::FatArrow)?;
                            Some(Box::new(self.parse_assign_expr()?))
                        }
                        _ => {
                            return Err(PerlError::syntax(
                                "glob_par: expected `progress => EXPR` after comma",
                                line,
                            ));
                        }
                    }
                } else {
                    None
                };
                Ok(Expr {
                    kind: ExprKind::GlobPar { args, progress },
                    line,
                })
            }
            "par_sed" => {
                let args = self.parse_builtin_args()?;
                let progress = if self.eat(&Token::Comma) {
                    match self.peek().clone() {
                        Token::Ident(ref kw)
                            if kw == "progress" && matches!(self.peek_at(1), Token::FatArrow) =>
                        {
                            self.advance();
                            self.expect(&Token::FatArrow)?;
                            Some(Box::new(self.parse_assign_expr()?))
                        }
                        _ => {
                            return Err(PerlError::syntax(
                                "par_sed: expected `progress => EXPR` after comma",
                                line,
                            ));
                        }
                    }
                } else {
                    None
                };
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
                // Anonymous sub
                let body = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body,
                    },
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
                    // Bareword — treat as string (like hash key)
                    Ok(Expr {
                        kind: ExprKind::String(name),
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
                    || matches!(self.peek(), Token::DoubleString(_) | Token::SingleString(_))
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
        let block = self.parse_block()?;
        self.eat(&Token::Comma);
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
                    return Ok((init, block, merge_expr_list(parts), Some(prog)));
                }
            }
            parts.push(self.parse_assign_expr()?);
        }
        Ok((init, block, merge_expr_list(parts), None))
    }

    /// Like [`parse_block_list`] but supports a trailing `, progress => EXPR`
    /// (`pmap`, `pgrep`, `preduce`, `pfor`, `pcache`, `psort`, …).
    fn parse_block_then_list_optional_progress(
        &mut self,
    ) -> PerlResult<(Block, Expr, Option<Expr>)> {
        let block = self.parse_block()?;
        self.eat(&Token::Comma);
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
                    return Ok((block, merge_expr_list(parts), Some(prog)));
                }
            }
            parts.push(self.parse_assign_expr()?);
        }
        Ok((block, merge_expr_list(parts), None))
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
                    return Err(PerlError::syntax(
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
    fn parse_assign_expr_list_optional_progress(&mut self) -> PerlResult<(Expr, Option<Expr>)> {
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
                    return Ok((merge_expr_list(parts), Some(prog)));
                }
            }
            parts.push(self.parse_assign_expr()?);
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
            self.parse_assign_expr()
        }
    }

    fn parse_one_arg_or_default(&mut self) -> PerlResult<Expr> {
        if matches!(
            self.peek(),
            Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::Comma
        ) {
            Ok(Expr {
                kind: ExprKind::ScalarVar("_".into()),
                line: self.peek_line(),
            })
        } else {
            self.parse_one_arg()
        }
    }

    fn parse_one_arg_or_argv(&mut self) -> PerlResult<Expr> {
        if matches!(
            self.peek(),
            Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::Comma
        ) {
            Ok(Expr {
                kind: ExprKind::ArrayVar("_".into()),
                line: self.peek_line(),
            })
        } else {
            self.parse_one_arg()
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

    fn parse_arg_list(&mut self) -> PerlResult<Vec<Expr>> {
        let mut args = Vec::new();
        while !matches!(
            self.peek(),
            Token::RParen | Token::RBracket | Token::RBrace | Token::Eof
        ) {
            args.push(self.parse_assign_expr()?);
            if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                break;
            }
        }
        Ok(args)
    }

    /// Arguments for `->name` / `->SUPER::name` **without** `(...)`. Unlike `die foo + 1`
    /// (unary `+` on `1` passed to `foo`), Perl treats `$o->meth + 5` as infix `+` after a
    /// no-arg method call; we must not consume that `+` as the start of a first argument.
    fn parse_method_arg_list_no_paren(&mut self) -> PerlResult<Vec<Expr>> {
        let mut args = Vec::new();
        loop {
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
            if args.is_empty() && self.peek_method_arg_infix_terminator() {
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
                | Token::BindMatch
                | Token::BindNotMatch
                | Token::Arrow
        )
    }

    fn parse_list_until_terminator(&mut self) -> PerlResult<Vec<Expr>> {
        let mut args = Vec::new();
        loop {
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof
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
            args.push(self.parse_assign_expr()?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(args)
    }

    fn try_parse_hash_ref(&mut self) -> PerlResult<Vec<(Expr, Expr)>> {
        let mut pairs = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let key = self.parse_assign_expr()?;
            // Expect => or , after key
            if self.eat(&Token::FatArrow) || self.eat(&Token::Comma) {
                let val = self.parse_assign_expr()?;
                pairs.push((key, val));
                self.eat(&Token::Comma);
            } else {
                return Err(PerlError::syntax("Expected => or , in hash ref", key.line));
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(pairs)
    }

    fn parse_interpolated_string(&self, s: &str, line: usize) -> Expr {
        // Parse $var and @var inside double-quoted strings
        let mut parts = Vec::new();
        let mut literal = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                if !literal.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                }
                i += 1;
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
                } else {
                    // Special var like $! or literal $
                    literal.push('$');
                    literal.push(chars[i]);
                    i += 1;
                }
            } else if chars[i] == '@'
                && i + 1 < chars.len()
                && (chars[i + 1].is_alphabetic() || chars[i + 1] == '_')
            {
                if !literal.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                }
                i += 1;
                let mut name = String::new();
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    name.push(chars[i]);
                    i += 1;
                }
                parts.push(StringPart::ArrayVar(name));
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
                return Expr {
                    kind: ExprKind::String(s.clone()),
                    line,
                };
            }
        }
        if parts.is_empty() {
            return Expr {
                kind: ExprKind::String(String::new()),
                line,
            };
        }

        Expr {
            kind: ExprKind::InterpolatedString(parts),
            line,
        }
    }

    fn expr_to_overload_key(e: &Expr) -> PerlResult<String> {
        match &e.kind {
            ExprKind::String(s) => Ok(s.clone()),
            _ => Err(PerlError::syntax(
                "overload key must be a string literal (e.g. '\"\"' or '+')",
                e.line,
            )),
        }
    }

    fn expr_to_overload_sub(e: &Expr) -> PerlResult<String> {
        match &e.kind {
            ExprKind::String(s) => Ok(s.clone()),
            ExprKind::Integer(n) => Ok(n.to_string()),
            ExprKind::SubroutineRef(s) | ExprKind::SubroutineCodeRef(s) => Ok(s.clone()),
            _ => Err(PerlError::syntax(
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

/// Comma-separated expressions on a `format` value line (below a picture line).
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
            return Err(PerlError::syntax(
                "Extra tokens in format value line",
                parser.peek_line(),
            ));
        }
        break;
    }
    Ok(exprs)
}
